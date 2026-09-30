use super::*;
use outgoing::LocalImage;

/// Snapshot a chosen sticker before queuing an outbound message. The original
/// must still exist and match at both ends of the download.
pub async fn import(
    message: MessageRecord,
    store: Store,
    downloader: &dyn Downloader,
    mut cancel: watch::Receiver<bool>,
) -> Result<LocalImage, String> {
    check_cancel(&cancel)?;
    if let MessageBody::LocalImage { image, .. } = &message.body {
        if image.sticker.is_none() {
            return Err("Choose a sticker".into());
        }
        check_local(&message, &store).await?;
        let local = *image.clone();
        let root = store.data_dir().to_owned();
        let permit = tokio::select! {
            permit = preview::DECODERS.acquire() => permit.map_err(|_| "Image decoder stopped")?,
            _ = cancel.changed() => return Err("Sticker preparation cancelled".into()),
        };
        let result = tokio::task::spawn_blocking(move || {
            let _permit = permit;
            outgoing::read(&local, &root)?;
            Ok::<_, String>(local)
        })
        .await
        .map_err(|_| "Cannot prepare sticker")??;
        check_cancel(&cancel)?;
        check_local(&message, &store).await?;
        return Ok(result);
    }
    let attachment = current(&message, &store).await?;
    attachment.validate()?;
    if attachment.kind != AttachmentKind::Sticker || attachment.size > 500 * 1024 {
        return Err("Choose a WebP sticker up to 500 KiB".into());
    }
    let cache = cache::wait_open(store.data_dir(), true, cancel.clone())
        .await?
        .ok_or("Sticker download is busy; try again")?;
    let id = cache::token(&message.key, &attachment);
    let path = cache.path(&id, &attachment);
    if !cache.manifest(&id).is_some_and(|m| m.key == message.key)
        || cache::verify(path.clone(), &attachment).await.is_err()
    {
        cache.remove(&id)?;
        cache.reserve(attachment.size)?;
        let temporary = cache.temporary()?;
        downloader
            .download(&attachment, temporary.path(), cancel.clone())
            .await?;
        check_cancel(&cancel)?;
        cache::verify(temporary.path().to_owned(), &attachment).await?;
        current(&message, &store).await?;
        cache.publish(temporary, &message.key, &attachment)?;
    }
    let permit = tokio::select! {
        permit = preview::DECODERS.acquire() => permit.map_err(|_| "Image decoder stopped")?,
        _ = cancel.changed() => return Err("Sticker preparation cancelled".into()),
    };
    let root = store.data_dir().to_owned();
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let bytes = preview::read_bounded(&path)?;
        use sha2::{Digest, Sha256};
        if bytes.len() as u64 != attachment.size
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != attachment.sha256
        {
            return Err("Sticker changed during preparation".into());
        }
        let result = outgoing::import_sticker(&bytes, &root, true);
        drop(cache);
        result
    })
    .await
    .map_err(|_| "Cannot prepare sticker")?;
    check_cancel(&cancel)?;
    current(&message, &store).await?;
    result
}

async fn check_local(message: &MessageRecord, store: &Store) -> Result<(), String> {
    let current = store
        .get_message(message.key.clone())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Sticker is no longer available")?;
    if current.key != message.key
        || current.body != message.body
        || current
            .expires_at_ms
            .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
    {
        return Err("Sticker changed or expired".into());
    }
    Ok(())
}
