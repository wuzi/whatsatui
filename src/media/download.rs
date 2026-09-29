use super::{Attachment, Downloader};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::watch;
use tokio::{io::AsyncWriteExt, process::Command};

#[derive(Serialize, Deserialize)]
pub(super) struct WorkerInput {
    pub attachment: Attachment,
    pub destination: PathBuf,
}

pub struct NativeDownloader;
#[async_trait::async_trait]
impl Downloader for NativeDownloader {
    async fn download(
        &self,
        attachment: &Attachment,
        destination: &Path,
        cancel: watch::Receiver<bool>,
    ) -> Result<(), String> {
        let mut attachment = attachment.clone();
        attachment.filename = None;
        attachment.caption = None;
        attachment.mime = None;
        let input = WorkerInput {
            attachment,
            destination: destination.to_owned(),
        };
        let executable =
            std::env::current_exe().map_err(|_| "Cannot locate the download worker")?;
        run_worker(&executable, &input, cancel, Duration::from_secs(60)).await
    }
}

async fn run_worker(
    executable: &Path,
    input: &WorkerInput,
    mut cancel: watch::Receiver<bool>,
    deadline: Duration,
) -> Result<(), String> {
    super::check_cancel(&cancel)?;
    let data = serde_json::to_vec(input).map_err(|_| "Unsupported attachment path")?;
    if data.len() > 64 * 1024 {
        return Err("Attachment reference is too large".into());
    }
    let mut child = Command::new(executable)
        .arg("--media-worker")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "Could not start the download worker")?;
    let result = tokio::select! {
        _ = cancel.changed() => Err("Attachment download canceled".to_owned()),
        result = tokio::time::timeout(deadline, async {
            let mut stdin = child.stdin.take().ok_or_else(|| std::io::Error::other("Missing input"))?;
            stdin.write_all(&data).await?;
            stdin.shutdown().await?;
            drop(stdin);
            child.wait().await
        }) => match result {
            Ok(Ok(status)) if status.success() => Ok(()),
            Ok(_) => Err("Download failed; the attachment may no longer be available".into()),
            Err(_) => Err("Download timed out; try again".into()),
        }
    };
    if result.is_err() {
        let _ = child.kill().await;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn input(dir: &Path) -> WorkerInput {
        WorkerInput {
            attachment: Attachment {
                kind: super::super::AttachmentKind::Image,
                filename: None,
                mime: None,
                caption: None,
                size: 6,
                direct_path: "/v/media?secret=fake".into(),
                media_key: [1; 32],
                sha256: [2; 32],
                encrypted_sha256: [3; 32],
            },
            destination: dir.join("received file;$(noop).jpg"),
        }
    }
    fn executable(dir: &Path, script: &str) -> PathBuf {
        let path = dir.join("worker");
        fs::write(&path, format!("#!/bin/sh\n{script}")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
        path
    }
    #[tokio::test]
    async fn worker_receives_private_parameters_only_through_stdin() {
        let dir = tempfile::tempdir().unwrap();
        let args = dir.path().join("args");
        let data = dir.path().join("data");
        let worker = executable(
            dir.path(),
            &format!(
                "printf '%s\\n' \"$#\" \"$1\" > '{}'\n/bin/cat > '{}'\n",
                args.display(),
                data.display()
            ),
        );
        let input = input(dir.path());
        let (_stop, cancel) = watch::channel(false);
        run_worker(&worker, &input, cancel, Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(fs::read_to_string(args).unwrap(), "1\n--media-worker\n");
        let received: WorkerInput = serde_json::from_slice(&fs::read(data).unwrap()).unwrap();
        assert_eq!(received.destination, input.destination);
        assert_eq!(received.attachment, input.attachment);
    }
    #[tokio::test]
    async fn stalled_and_canceled_workers_are_killed_and_reaped() {
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("pid");
        let worker = executable(
            dir.path(),
            &format!(
                "printf '%s' \"$$\" > '{}'\nexec /bin/sleep 10\n",
                pid_file.display()
            ),
        );
        for cancel_early in [false, true] {
            let (stop, cancel) = watch::channel(false);
            let input = input(dir.path());
            let started = std::time::Instant::now();
            let task = run_worker(&worker, &input, cancel, Duration::from_millis(100));
            let canceler = async {
                if cancel_early {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    stop.send(true).unwrap();
                }
            };
            let (result, ()) = tokio::join!(task, canceler);
            assert!(result.is_err());
            assert!(started.elapsed() < Duration::from_secs(1));
            let pid = fs::read_to_string(&pid_file).unwrap();
            assert!(
                !Path::new(&format!("/proc/{pid}")).exists(),
                "worker still alive or unreaped"
            );
        }
    }
}
