use crate::{app::model::*, storage::Store};
use std::collections::BTreeMap;
use tokio::time::{Duration, Instant};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) const CAPACITY: usize = 128;
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Context {
    pub account: Option<AccountId>,
    pub reading: Option<ChatId>,
    pub enabled: bool,
    pub previews: bool,
}
pub(crate) struct Inbox {
    context: tokio::sync::watch::Sender<Context>,
    started_ms: i64,
    keys: Vec<MessageKey>,
    overflow: Option<Overflow>,
    due: Option<Instant>,
    retry_at: Option<Instant>,
    in_flight: bool,
    failure_reported: bool,
}
impl Inbox {
    pub fn new(now_ms: i64) -> Self {
        Self {
            context: tokio::sync::watch::channel(Context::default()).0,
            started_ms: now_ms / 1000 * 1000,
            keys: vec![],
            overflow: None,
            due: None,
            retry_at: None,
            in_flight: false,
            failure_reported: false,
        }
    }
    pub fn context(&self) -> tokio::sync::watch::Receiver<Context> {
        self.context.subscribe()
    }
    pub fn update_context(&self, next: Context) {
        self.context.send_if_modified(|current| {
            if *current == next {
                false
            } else {
                *current = next;
                true
            }
        });
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
                let overflow = self.overflow.get_or_insert(Overflow {
                    account: m.key.account.clone(),
                    since_ms: m.created_at_ms,
                    until_ms: m.created_at_ms,
                });
                overflow.since_ms = overflow.since_ms.min(m.created_at_ms);
                overflow.until_ms = overflow.until_ms.max(m.created_at_ms);
            }
        }
    }
    pub fn retain(&mut self, account: Option<&AccountId>, reading: Option<&ChatId>) {
        self.keys
            .retain(|k| account.is_none_or(|a| a == &k.account) && reading != Some(&k.chat));
        if self
            .overflow
            .as_ref()
            .is_some_and(|o| account.is_some_and(|a| a != &o.account))
        {
            self.overflow = None;
        }
        if self.keys.is_empty() && self.overflow.is_none() {
            self.clear();
        }
    }
    pub fn clear(&mut self) {
        self.keys.clear();
        self.due = None;
        self.overflow = None;
    }
    pub fn take(&mut self, now: Instant, previews: bool) -> Option<Request> {
        if self.in_flight
            || self.retry_at.is_some_and(|at| now < at)
            || self.due.is_none_or(|at| now < at)
            || (self.keys.is_empty() && self.overflow.is_none())
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
pub struct Overflow {
    pub account: AccountId,
    pub since_ms: i64,
    pub until_ms: i64,
}
#[derive(Clone, Debug)]
pub struct Request {
    pub keys: Vec<MessageKey>,
    pub overflow: Option<Overflow>,
    pub previews: bool,
}
impl Request {
    pub(crate) fn account(&self) -> Option<&AccountId> {
        self.keys
            .first()
            .map(|key| &key.account)
            .or_else(|| self.overflow.as_ref().map(|o| &o.account))
    }
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
#[cfg(unix)]
mod native;
#[cfg(windows)]
#[path = "notifications/windows.rs"]
mod native;
pub use native::NativeNotifier;
pub async fn deliver(popup: &Popup, notifier: &impl Notifier) -> Result<(), String> {
    notifier.show(popup).await
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
    prepare_scoped(request, store, now_ms, None).await
}
pub(crate) async fn prepare_scoped(
    mut request: Request,
    store: &Store,
    now_ms: i64,
    reading: Option<ChatId>,
) -> Result<Option<Popup>, String> {
    request
        .keys
        .retain(|key| reading.as_ref() != Some(&key.chat));
    if let Some(overflow) = request.overflow
        && store
            .notification_overflow(overflow, reading.clone(), now_ms)
            .await
            .map_err(|e| e.to_string())?
    {
        return Ok(Some(Popup {
            title: "whatsapp-tui".into(),
            body: "You have new messages".into(),
        }));
    }
    let messages = store
        .notification_messages(request.keys, now_ms, reading)
        .await
        .map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Ok(None);
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
