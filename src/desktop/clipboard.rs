//! Explicit, bounded clipboard reads. Clipboard contents never pass through a shell.
use std::{path::PathBuf, process::Stdio, time::Duration};
use tokio::{io::AsyncReadExt, process::Command, sync::watch};

pub enum Content {
    Image { bytes: Vec<u8>, filename: String },
    File(PathBuf),
    Text(String),
}

#[derive(Clone, Debug)]
pub enum Paste {
    Image(Box<crate::media::outgoing::LocalImage>),
    Text(String),
}

pub async fn read(cancel: watch::Receiver<bool>) -> Result<Content, String> {
    let (program, list, prefix): (&str, &[&str], &[&str]) =
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            ("wl-paste", &["--list-types"], &["--no-newline", "--type"])
        } else if std::env::var_os("DISPLAY").is_some() {
            (
                "xclip",
                &["-selection", "clipboard", "-out", "-target", "TARGETS"],
                &["-selection", "clipboard", "-out", "-target"],
            )
        } else {
            return Err("Clipboard unavailable: use a local Wayland or X11 session".into());
        };
    let mut command = Command::new(program);
    command.args(list);
    let types = capture(command, 16 * 1024, cancel.clone()).await?;
    let types = String::from_utf8_lossy(&types);
    let mime = [
        "image/png",
        "image/jpeg",
        "image/webp",
        "text/uri-list",
        "x-special/gnome-copied-files",
        "text/plain;charset=utf-8",
        "text/plain",
        "UTF8_STRING",
    ]
    .into_iter()
    .find(|mime| types.lines().any(|line| line.trim() == *mime))
    .ok_or("Clipboard has no supported image, copied file, or text")?;
    let mut command = Command::new(program);
    command.args(prefix).arg(mime);
    let limit = if mime.starts_with("image/") {
        16 * 1024 * 1024
    } else {
        1024 * 1024
    };
    let bytes = capture(command, limit, cancel).await?;
    if let Some(extension) = mime.strip_prefix("image/") {
        return Ok(Content::Image {
            bytes,
            filename: format!("clipboard.{extension}"),
        });
    }
    let text = String::from_utf8(bytes).map_err(|_| "Clipboard text is not UTF-8")?;
    if matches!(mime, "text/uri-list" | "x-special/gnome-copied-files") {
        return copied_file(&text).map(Content::File);
    }
    Ok(Content::Text(text))
}

fn copied_file(text: &str) -> Result<PathBuf, String> {
    let mut lines = text.lines().map(str::trim).filter(|line| {
        !line.is_empty() && !line.starts_with('#') && !matches!(*line, "copy" | "cut")
    });
    let uri = lines.next().ok_or("No copied file in clipboard")?;
    if lines.next().is_some() {
        return Err("Copy one image at a time".into());
    }
    reqwest::Url::parse(uri)
        .ok()
        .and_then(|url| url.to_file_path().ok())
        .ok_or_else(|| "Clipboard file must be a local image".into())
}

async fn capture(
    mut command: Command,
    limit: usize,
    mut cancel: watch::Receiver<bool>,
) -> Result<Vec<u8>, String> {
    if *cancel.borrow() {
        return Err("Clipboard read cancelled".into());
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                "Clipboard helper unavailable: install wl-clipboard (Wayland) or xclip (X11)".into()
            } else {
                "Cannot read clipboard; check your desktop session".to_owned()
            }
        })?;
    let result = tokio::select! {
        _ = cancel.changed() => Err("Clipboard read cancelled".to_owned()),
        result = tokio::time::timeout(Duration::from_secs(3), async {
            let mut bytes = Vec::new();
            child.stdout.take().ok_or("Clipboard helper has no output")?
                .take(limit as u64 + 1).read_to_end(&mut bytes).await
                .map_err(|_| "Cannot read clipboard")?;
            if bytes.len() > limit { return Err("Clipboard content exceeds the size limit"); }
            if !child.wait().await.map_err(|_| "Clipboard helper failed")?.success() {
                return Err("Clipboard is empty or unavailable");
            }
            Ok(bytes)
        }) => match result {
            Ok(result) => result.map_err(str::to_owned),
            Err(_) => Err("Clipboard helper timed out".to_owned()),
        },
    };
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn copied_files_accept_spaces_and_reject_remote_or_multiple_files() {
        assert_eq!(
            copied_file("copy\nfile:///tmp/my%20photo.png\r\n").unwrap(),
            PathBuf::from("/tmp/my photo.png")
        );
        assert!(copied_file("https://example.org/a.png").is_err());
        assert!(copied_file("file://remote/tmp/a.png").is_err());
        assert!(copied_file("file:///tmp/a.png\nfile:///tmp/b.png").is_err());
    }
    #[tokio::test]
    async fn helper_output_is_bounded_and_cancellable() {
        let (_tx, rx) = watch::channel(false);
        let mut command = Command::new("/bin/printf");
        command.arg("too many bytes");
        assert!(
            capture(command, 4, rx)
                .await
                .unwrap_err()
                .contains("size limit")
        );
        let (tx, rx) = watch::channel(false);
        let mut command = Command::new("/bin/sleep");
        command.arg("10");
        let work = capture(command, 16, rx);
        let cancel = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            tx.send(true).unwrap();
        };
        let (result, _) = tokio::join!(work, cancel);
        assert!(result.unwrap_err().contains("cancelled"));
    }
}
