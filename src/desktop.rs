pub mod clipboard;
#[cfg(windows)]
mod windows;
use crate::{
    app::model::MessageRecord,
    message_actions::{self, DesktopAction},
    storage::Store,
};
#[cfg(unix)]
use std::{io, process::Stdio, time::Duration};
#[cfg(unix)]
use tokio::{io::AsyncWriteExt, process::Command};

#[async_trait::async_trait]
pub trait Desktop: Send + Sync {
    async fn copy(&self, text: &str) -> Result<(), String>;
    async fn open(&self, url: &str) -> Result<(), String>;
    async fn open_file(&self, _path: &std::path::Path) -> Result<(), String> {
        Err("File viewer is unavailable".into())
    }
}
pub async fn execute(
    message: MessageRecord,
    action: DesktopAction,
    store: Store,
    integration: &impl Desktop,
) -> Result<String, String> {
    let current = store
        .get_message(message.key.clone())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Message is no longer available")?;
    if current.key != message.key || current.body != message.body {
        return Err("Message changed; reopen its actions".into());
    }
    let text = message_actions::text(&current, chrono::Utc::now().timestamp_millis())
        .ok_or("Message text is no longer available")?;
    let open_link = matches!(action, DesktopAction::OpenLink(_));
    match action {
        DesktopAction::CopyText => {
            check_copy_size(text)?;
            integration.copy(text).await?;
            Ok("Copied message text".into())
        }
        DesktopAction::OpenLink(url) | DesktopAction::CopyLink(url) => {
            if !message_actions::web_links(text).contains(&url) {
                return Err("Link is no longer available in this message".into());
            }
            if open_link {
                integration.open(&url).await?;
                Ok("Browser request sent".into())
            } else {
                integration.copy(&url).await?;
                Ok("Copied link".into())
            }
        }
    }
}

pub struct NativeDesktop;
#[cfg(unix)]
#[async_trait::async_trait]
impl Desktop for NativeDesktop {
    async fn open_file(&self, path: &std::path::Path) -> Result<(), String> {
        if !path.is_absolute() || !std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file()) {
            return Err("Downloaded file is unavailable".into());
        }
        let mut command = Command::new("xdg-open");
        command.arg(path);
        run(command, None, Duration::from_secs(3))
            .await
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    "Viewer unavailable: install xdg-utils and configure a default viewer".into()
                } else {
                    helper_error("Viewer", e)
                }
            })
    }
    async fn copy(&self, text: &str) -> Result<(), String> {
        check_copy_size(text)?;
        let mut helpers: Vec<(&str, &[&str])> = vec![];
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            helpers.push(("wl-copy", &["--type", "text/plain;charset=utf-8"]));
        }
        if std::env::var_os("DISPLAY").is_some() {
            helpers.push(("xclip", &["-selection", "clipboard", "-in"]));
            helpers.push(("xsel", &["--clipboard", "--input"]));
        }
        for (program, args) in helpers {
            let mut command = Command::new(program);
            command.args(args);
            match run(command, Some(text.as_bytes()), Duration::from_secs(3)).await {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(helper_error("Clipboard", e)),
            }
        }
        Err("Clipboard unavailable: install wl-clipboard (Wayland) or xclip/xsel (X11) in a desktop session".into())
    }
    async fn open(&self, url: &str) -> Result<(), String> {
        if !message_actions::valid_web_link(url) {
            return Err("Only HTTP/HTTPS links can be opened".into());
        }
        let mut command = Command::new("xdg-open");
        command.arg(url);
        run(command, None, Duration::from_secs(3))
            .await
            .map_err(|e| {
                if e.kind() == io::ErrorKind::NotFound {
                    "Browser unavailable: install xdg-utils and configure a default browser".into()
                } else {
                    helper_error("Browser", e)
                }
            })
    }
}
fn check_copy_size(text: &str) -> Result<(), String> {
    if text.len() > 1024 * 1024 {
        Err("Text exceeds the 1 MiB clipboard limit".into())
    } else {
        Ok(())
    }
}
#[cfg(unix)]
fn helper_error(name: &str, error: io::Error) -> String {
    format!(
        "{name} helper {}",
        if error.kind() == io::ErrorKind::TimedOut {
            "timed out"
        } else {
            "failed; check your desktop session"
        }
    )
}

#[cfg(unix)]
pub(crate) async fn run(
    mut command: Command,
    input: Option<&[u8]>,
    deadline: Duration,
) -> io::Result<()> {
    let mut child = command
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let result = tokio::time::timeout(deadline, async {
        if let Some(input) = input {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| io::Error::other("Missing helper input"))?;
            stdin.write_all(input).await?;
            stdin.shutdown().await?;
        }
        child.wait().await
    })
    .await;
    match result {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(_)) => Err(io::Error::other("Helper failed")),
        result => {
            let _ = child.kill().await;
            Err(match result {
                Ok(Err(e)) => e,
                _ => io::Error::new(io::ErrorKind::TimedOut, "Helper timed out"),
            })
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        time::{Duration, Instant},
    };
    use tokio::process::Command;
    fn script(dir: &std::path::Path, body: &str) -> Command {
        let file = dir.join("helper");
        std::fs::write(&file, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut c = Command::new(file);
        c.env("TRACE_PATH", dir.join("result"));
        c
    }
    #[tokio::test]
    async fn helper_receives_literal_stdin_and_single_url_argument() {
        let dir = tempfile::tempdir().unwrap();
        let source = "*café*\n$(touch nope)\u{1b} raw";
        run(
            script(dir.path(), "/bin/cat > \"$TRACE_PATH\""),
            Some(source.as_bytes()),
            Duration::from_secs(1),
        )
        .await
        .unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("result")).unwrap(),
            source.as_bytes()
        );
        let mut command = script(dir.path(), "printf '%s\\n' \"$#\" \"$1\" > \"$TRACE_PATH\"");
        command.arg("https://example.org/?q=$(touch_nope)&x=1");
        run(command, None, Duration::from_secs(1)).await.unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("result")).unwrap(),
            "1\nhttps://example.org/?q=$(touch_nope)&x=1\n"
        );
    }
    #[tokio::test]
    async fn helper_errors_and_unread_stdin_have_bounded_lifetimes() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            run(
                Command::new(dir.path().join("absent")),
                None,
                Duration::from_secs(1)
            )
            .await
            .is_err()
        );
        assert!(
            run(script(dir.path(), "exit 7"), None, Duration::from_secs(1))
                .await
                .is_err()
        );
        let start = Instant::now();
        let error = run(
            script(dir.path(), "exec /bin/sleep 10"),
            Some(&vec![b'x'; 1024 * 1024]),
            Duration::from_millis(30),
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(1));
    }
}
