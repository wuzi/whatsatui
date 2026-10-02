use super::{AttachmentKind, Downloader, acquire, cache, check_cancel, current};
use crate::{
    app::model::{MessageBody, MessageRecord},
    storage::Store,
};
use tokio::sync::watch;

pub async fn prepare(
    message: &MessageRecord,
    store: &Store,
    downloader: &dyn Downloader,
    cancel: watch::Receiver<bool>,
) -> Result<tempfile::NamedTempFile, String> {
    check_cancel(&cancel)?;
    if !matches!(&message.body, MessageBody::Media(a) if matches!(a.kind, AttachmentKind::Audio | AttachmentKind::Video) || a.is_gif())
    {
        return Err("Select a voice message, audio file or video".into());
    }
    let attachment = current(message, store).await?;
    let (_cache, path, _) = acquire(message, store, downloader, true, cancel.clone()).await?;
    // A separate private file pins verified bytes without keeping the media
    // cache locked or letting its partial-file cleanup remove active playback.
    let snapshot = tempfile::Builder::new()
        .prefix("whatsapp-tui-audio-")
        .tempfile()
        .map_err(|_| "Cannot prepare media file")?;
    let snapshot = copy_snapshot(path, snapshot).await?;
    cache::verify(snapshot.path().to_owned(), &attachment).await?;
    current(message, store).await?;
    check_cancel(&cancel)?;
    Ok(snapshot)
}

async fn copy_snapshot(
    path: std::path::PathBuf,
    mut snapshot: tempfile::NamedTempFile,
) -> Result<tempfile::NamedTempFile, String> {
    // The blocking operation may outlive its awaiting future. Keep ownership
    // and the open destination inside it so cancellation cannot recreate a
    // file after its cleanup guard has already removed the path.
    tokio::task::spawn_blocking(move || -> std::io::Result<_> {
        let mut source = std::fs::File::open(path)?;
        std::io::copy(&mut source, snapshot.as_file_mut())?;
        Ok(snapshot)
    })
    .await
    .map_err(|_| "Media file preparation stopped")?
    .map_err(|_| "Cannot prepare media file".into())
}

#[cfg(test)]
mod tests {
    use super::copy_snapshot;

    #[test]
    fn cancellation_before_a_queued_copy_runs_cannot_leave_a_snapshot() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .max_blocking_threads(1)
            .build()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source");
        std::fs::write(&source, b"synthetic private audio").unwrap();
        let snapshot = tempfile::NamedTempFile::new_in(dir.path()).unwrap();
        let destination = snapshot.path().to_owned();
        let (started, ready) = std::sync::mpsc::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let blocker = runtime.spawn_blocking(move || {
            started.send(()).unwrap();
            blocked.recv().unwrap();
        });
        ready.recv().unwrap();
        let queued = runtime.block_on(async {
            let copying = copy_snapshot(source, snapshot);
            tokio::pin!(copying);
            futures_util::poll!(copying).is_pending()
            // Drop the awaiting future while the copy is still queued.
        });
        release.send(()).unwrap();
        runtime.block_on(blocker).unwrap();
        runtime.block_on(runtime.spawn_blocking(|| ())).unwrap();
        drop(runtime);
        assert!(queued);
        assert!(
            !destination.exists(),
            "canceled copy recreated an unowned snapshot"
        );
    }
}
