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
        // Keep the proc link unresolved: current_exe() resolves it to a
        // nonexistent "... (deleted)" path after an in-place app update.
        #[cfg(target_os = "linux")]
        let executable = PathBuf::from("/proc/self/exe");
        #[cfg(not(target_os = "linux"))]
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
        .map_err(|error| {
            format!(
                "Could not start the download worker: {error}. Restart WhatsAppTUI and try again."
            )
        })?;
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

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};
    fn input(dir: &Path) -> WorkerInput {
        WorkerInput {
            attachment: Attachment {
                audio: None,
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
    async fn spawn_errors_explain_the_failure_without_private_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let input = input(dir.path());
        let blocked = dir.path().join("private-worker-name");
        fs::write(&blocked, "not executable").unwrap();
        fs::set_permissions(&blocked, fs::Permissions::from_mode(0o600)).unwrap();
        for (path, code) in [(dir.path().join("missing"), 2), (blocked, 13)] {
            let (_stop, cancel) = watch::channel(false);
            let error = run_worker(&path, &input, cancel, Duration::from_secs(1))
                .await
                .unwrap_err();
            assert!(error.contains(&format!("os error {code}")), "{error}");
            assert!(error.contains("Restart WhatsAppTUI"), "{error}");
            for private in [
                "secret=fake",
                "private-worker-name",
                "received file",
                "$(noop)",
            ] {
                assert!(!error.contains(private), "private input leaked: {error}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn native_worker_starts_after_its_running_executable_is_replaced_or_removed() {
        for change in ["unchanged", "replaced", "removed"] {
            let dir = tempfile::tempdir().unwrap();
            let executable = dir.path().join("running-client");
            let original = std::env::current_exe().unwrap();
            // Unlinking this extra name cannot alter Cargo's own executable.
            if fs::hard_link(&original, &executable).is_err() {
                fs::copy(&original, &executable).unwrap();
            }
            let child = Command::new(&executable)
                .args([
                    "--exact",
                    "media::download::tests::reexec_download_child",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env("WHATSAPP_TUI_TEST_WORKER_REEXEC_DIR", dir.path())
                .env("WHATSAPP_TUI_TEST_WORKER_REEXEC_CHANGE", change)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(10), async {
                while !dir.path().join("ready").exists() {
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("child did not become ready");
            match change {
                "replaced" => {
                    let replacement = dir.path().join("replacement");
                    fs::write(&replacement, "new release: deliberately not executable").unwrap();
                    fs::rename(replacement, &executable).unwrap();
                }
                "removed" => fs::remove_file(&executable).unwrap(),
                _ => {}
            }
            fs::write(dir.path().join("go"), "continue").unwrap();
            let result = tokio::time::timeout(Duration::from_secs(10), child.wait_with_output())
                .await
                .expect("child did not finish")
                .unwrap();
            assert!(
                result.status.success(),
                "{change}: {}{}",
                String::from_utf8_lossy(&result.stdout),
                String::from_utf8_lossy(&result.stderr)
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn reexec_download_child() {
        let Some(directory) = std::env::var_os("WHATSAPP_TUI_TEST_WORKER_REEXEC_DIR") else {
            return;
        };
        let directory = PathBuf::from(directory);
        fs::write(directory.join("ready"), "ready").unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            while !directory.join("go").exists() {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("parent did not release child");
        if std::env::var("WHATSAPP_TUI_TEST_WORKER_REEXEC_CHANGE").unwrap() != "unchanged" {
            assert!(
                !std::env::current_exe().unwrap().exists(),
                "fixture must reproduce the deleted executable path"
            );
        }
        let input = input(&directory);
        let (_stop, cancel) = watch::channel(false);
        let error = NativeDownloader
            .download(&input.attachment, &input.destination, cancel)
            .await
            .unwrap_err();
        // The reexecuted libtest binary rejects --media-worker, proving it
        // started without allowing any real network or account-store access.
        // A stale pathname instead fails earlier with a worker-spawn error.
        assert_eq!(
            error,
            "Download failed; the attachment may no longer be available"
        );
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
