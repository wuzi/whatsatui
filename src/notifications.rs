use crate::{app::model::*, storage::Store};
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) const CAPACITY: usize = 128;
pub(crate) struct Inbox {
    started_ms: i64,
    keys: Vec<MessageKey>,
    overflow: bool,
    due: Option<Instant>,
    retry_at: Option<Instant>,
    in_flight: bool,
    failure_reported: bool,
}
impl Inbox {
    pub fn new(now_ms: i64) -> Self {
        Self {
            started_ms: now_ms / 1000 * 1000,
            keys: vec![],
            overflow: false,
            due: None,
            retry_at: None,
            in_flight: false,
            failure_reported: false,
        }
    }
    pub fn push(&mut self, messages: Vec<MessageRecord>, now: Instant) {
        for m in messages {
            if m.key.from_me
                || m.created_at_ms < self.started_ms
                || matches!(m.body, MessageBody::Deleted | MessageBody::Expired)
                || m.expires_at_ms
                    .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
                || self.keys.contains(&m.key)
            {
                continue;
            }
            self.due.get_or_insert(now + Duration::from_secs(2));
            if self.keys.len() < CAPACITY {
                self.keys.push(m.key);
            } else {
                self.overflow = true;
            }
        }
    }
    pub fn retain(&mut self, account: Option<&AccountId>, reading: Option<&ChatId>) {
        self.keys
            .retain(|k| account.is_none_or(|a| a == &k.account) && reading != Some(&k.chat));
        if self.keys.is_empty() {
            self.clear();
        }
    }
    pub fn clear(&mut self) {
        self.keys.clear();
        self.due = None;
        self.overflow = false;
    }
    pub fn take(&mut self, now: Instant, previews: bool) -> Option<Request> {
        if self.in_flight
            || self.retry_at.is_some_and(|at| now < at)
            || self.due.is_none_or(|at| now < at)
            || self.keys.is_empty()
        {
            return None;
        }
        self.due = None;
        self.in_flight = true;
        Some(Request {
            keys: std::mem::take(&mut self.keys),
            overflow: std::mem::take(&mut self.overflow),
            previews,
        })
    }
    pub fn completed(&mut self, result: &Result<(), String>, now: Instant) -> bool {
        self.in_flight = false;
        if result.is_err() {
            self.retry_at = Some(now + Duration::from_secs(60));
            !std::mem::replace(&mut self.failure_reported, true)
        } else {
            self.retry_at = None;
            false
        }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub keys: Vec<MessageKey>,
    pub overflow: bool,
    pub previews: bool,
}
#[derive(Clone, Debug)]
pub struct Popup {
    pub title: String,
    pub body: String,
}
#[async_trait::async_trait]
pub trait Notifier: Send + Sync {
    async fn show(&self, popup: &Popup) -> Result<(), String>;
}
pub struct NativeNotifier;
#[async_trait::async_trait]
impl Notifier for NativeNotifier {
    async fn show(&self, popup: &Popup) -> Result<(), String> {
        send_with(
            std::path::Path::new("notify-send"),
            popup,
            Duration::from_secs(3),
        )
        .await
    }
}
pub async fn deliver(popup: &Popup, notifier: &impl Notifier) -> Result<(), String> {
    notifier.show(popup).await
}
async fn send_with(
    program: &std::path::Path,
    popup: &Popup,
    deadline: Duration,
) -> Result<(), String> {
    let mut command = tokio::process::Command::new(program);
    let body = popup
        .body
        .lines()
        .take(4)
        .map(|line| {
            plain(line, 240)
                .replace('&', "&amp;")
                .replace('<', "&lt;")
                .replace('>', "&gt;")
        })
        .collect::<Vec<_>>()
        .join("\n");
    command
        .args([
            "--app-name=whatsapp-tui",
            "--icon=mail-message-new",
            "--category=im.received",
            "--urgency=normal",
            "--expire-time=7000",
            "--hint=boolean:suppress-sound:true",
            "--",
        ])
        .arg(plain(&popup.title, 96))
        .arg(body);
    crate::desktop::run(command, None, deadline).await.map_err(|e| {
        match e.kind() {
            std::io::ErrorKind::NotFound => "Desktop notifications unavailable: install libnotify (notify-send); retrying on later messages".into(),
            std::io::ErrorKind::TimedOut => "Desktop notification helper timed out; retrying after 60 seconds".into(),
            _ => "Desktop notifications unavailable: check your desktop session; retrying after 60 seconds".into(),
        }
    })
}
pub(crate) struct Message {
    pub record: MessageRecord,
    pub chat_name: String,
    pub sender_name: String,
}
pub async fn prepare(
    request: Request,
    store: &Store,
    now_ms: i64,
) -> Result<Option<Popup>, String> {
    let messages = store
        .notification_messages(request.keys, now_ms)
        .await
        .map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Ok(None);
    }
    if request.overflow {
        return Ok(Some(Popup {
            title: "whatsapp-tui".into(),
            body: "You have new messages".into(),
        }));
    }
    let count = messages.len();
    if !request.previews {
        return Ok(Some(Popup {
            title: "whatsapp-tui".into(),
            body: if count == 1 {
                "New message".into()
            } else {
                format!("{count} new messages")
            },
        }));
    }
    let mut chats: BTreeMap<ChatId, Vec<Message>> = BTreeMap::new();
    for m in messages {
        chats.entry(m.record.key.chat.clone()).or_default().push(m);
    }
    for group in chats.values_mut() {
        group.sort_by_key(|m| m.record.created_at_ms);
    }
    if chats.len() == 1 {
        let group = chats.values().next().expect("nonempty messages");
        let m = group.last().expect("nonempty group");
        let name = plain(&m.chat_name, 64);
        return Ok(Some(Popup {
            title: if count == 1 {
                name
            } else {
                format!("{name} · {count} new messages")
            },
            body: preview(m, 160),
        }));
    }
    let mut lines = chats
        .values()
        .take(3)
        .map(|group| {
            let m = group.last().expect("nonempty group");
            format!("{}: {}", plain(&m.chat_name, 40), preview(m, 80))
        })
        .collect::<Vec<_>>();
    if chats.len() > 3 {
        lines.push(format!("and {} more chats", chats.len() - 3));
    }
    Ok(Some(Popup {
        title: format!("{count} new messages in {} chats", chats.len()),
        body: lines.join("\n"),
    }))
}
fn preview(m: &Message, limit: usize) -> String {
    let text = match &m.record.body {
        MessageBody::Text(text) => plain(text, limit),
        MessageBody::Media(a) => {
            let label = if a.audio.as_ref().is_some_and(|a| a.voice) {
                "voice message"
            } else {
                a.kind.label()
            };
            match a
                .caption
                .as_deref()
                .or(a.filename.as_deref())
                .filter(|s| !s.is_empty())
            {
                Some(caption) => format!("[{label}] {}", plain(caption, limit)),
                None => format!("[{label}]"),
            }
        }
        MessageBody::Unsupported { .. } => "[message]".into(),
        _ => "New message".into(),
    };
    if m.record.key.chat.0.ends_with("@g.us") {
        format!("{}: {text}", plain(&m.sender_name, 40))
    } else {
        text
    }
}
fn plain(text: &str, limit: usize) -> String {
    // Bound work before grapheme segmentation (including adversarial combining sequences).
    let text = text
        .chars()
        .take(2048)
        .filter_map(|c| {
            if c.is_whitespace() {
                Some(' ')
            } else if c.is_control()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                None
            } else {
                Some(c)
            }
        })
        .collect::<String>();
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result: String = text.graphemes(true).take(limit).collect();
    if result.len() < text.len() {
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[tokio::test]
    #[ignore = "requires the isolated synthetic notification D-Bus server"]
    async fn installed_notify_send_private_bus_smoke() {
        assert_eq!(
            std::env::var("WHATSAPP_TUI_PRIVATE_NOTIFICATION_BUS").as_deref(),
            Ok("1"),
            "Run only with the private-bus smoke harness, never on the real desktop"
        );
        deliver(
            &Popup {
                title: "Synthetic notification".into(),
                body: "<tag> & literal".into(),
            },
            &NativeNotifier,
        )
        .await
        .unwrap();
    }
    fn script(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("notify-test");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[tokio::test]
    async fn native_helper_uses_silent_literal_bounded_arguments_and_escaped_markup() {
        let dir = tempfile::tempdir().unwrap();
        let program = script(dir.path(), "printf '%s\\0' \"$@\" > \"$0.args\"");
        let popup = Popup {
            title: "--malicious\u{1b}title".into(),
            body: "<img src='secret'/> & $(echo bad)\u{0}\nhello".into(),
        };
        send_with(&program, &popup, Duration::from_secs(1))
            .await
            .unwrap();
        let bytes = std::fs::read(program.with_extension("args")).expect("helper executed");
        let args = bytes
            .split(|b| *b == 0)
            .filter(|a| !a.is_empty())
            .map(|a| String::from_utf8(a.to_vec()).unwrap())
            .collect::<Vec<_>>();
        assert!(
            args.iter()
                .any(|a| a == "--hint=boolean:suppress-sound:true")
        );
        assert!(args.iter().any(|a| a == "--urgency=normal"));
        let delimiter = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[delimiter + 1], "--malicioustitle");
        assert!(args[delimiter + 2].contains("&lt;img src='secret'/&gt; &amp; $(echo bad)"));
        assert!(!args[delimiter + 2].contains('\0'));
        assert_eq!(args.len(), delimiter + 3);
        let huge = Popup {
            title: "x".repeat(100_000),
            body: "👩‍💻".repeat(100_000),
        };
        send_with(&program, &huge, Duration::from_secs(1))
            .await
            .unwrap();
        assert!(
            std::fs::metadata(program.with_extension("args"))
                .unwrap()
                .len()
                < 16_384
        );
    }
    #[tokio::test]
    async fn missing_failed_and_hung_helpers_are_bounded_errors() {
        let dir = tempfile::tempdir().unwrap();
        let popup = Popup {
            title: "test".into(),
            body: "synthetic".into(),
        };
        assert!(
            send_with(&dir.path().join("missing"), &popup, Duration::from_secs(1))
                .await
                .unwrap_err()
                .contains("notify-send")
        );
        let program = script(dir.path(), "exit 7");
        assert!(
            send_with(&program, &popup, Duration::from_secs(1))
                .await
                .is_err()
        );
        let program = script(dir.path(), "exec /bin/sleep 30");
        let now = std::time::Instant::now();
        assert!(
            send_with(&program, &popup, Duration::from_millis(40))
                .await
                .unwrap_err()
                .contains("timed out")
        );
        assert!(now.elapsed() < Duration::from_secs(1));
    }
}
