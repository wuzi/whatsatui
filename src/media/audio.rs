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
    if !matches!(&message.body, MessageBody::Media(a) if a.kind == AttachmentKind::Audio) {
        return Err("Select a voice message or audio file".into());
    }
    let attachment = current(message, store).await?;
    let (_cache, path, _) = acquire(message, store, downloader, true, cancel.clone()).await?;
    // A separate private file pins verified bytes without keeping the media
    // cache locked or letting its partial-file cleanup remove active playback.
    let snapshot = tempfile::Builder::new()
        .prefix("whatsapp-tui-audio-")
        .tempfile()
        .map_err(|_| "Cannot prepare audio file")?;
    tokio::fs::copy(path, snapshot.path())
        .await
        .map_err(|_| "Cannot prepare audio file")?;
    cache::verify(snapshot.path().to_owned(), &attachment).await?;
    current(message, store).await?;
    check_cancel(&cancel)?;
    Ok(snapshot)
}
