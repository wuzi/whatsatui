//! Bounded, verified image loading, independent of the terminal renderer.
use super::{AttachmentKind, Downloader, cache, check_cancel, current};
use crate::{app::model::MessageRecord, storage::Store};
use image::{DynamicImage, ImageDecoder, ImageReader};
use std::io::{Cursor, Read};
use tokio::sync::watch;

pub const MAX_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_PIXELS: u64 = 16_000_000;

pub fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Image exceeds the 16 MiB limit".into());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Cannot identify image")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits.max_image_width = Some(16_000);
    limits.max_image_height = Some(16_000);
    reader.limits(limits);
    let decoder = reader
        .into_decoder()
        .map_err(|_| "Image format is unsupported or damaged")?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("Image exceeds the 16 megapixel limit".into());
    }
    DynamicImage::from_decoder(decoder).map_err(|_| "Cannot decode image".into())
}

pub fn read_bounded(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|_| "Image file is unavailable")?;
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES)
    {
        return Err("Choose a regular image file up to 16 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Cannot read image")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Image exceeds the 16 MiB limit".into());
    }
    Ok(bytes)
}

pub async fn load(
    message: MessageRecord,
    store: Store,
    downloader: &dyn Downloader,
    cancel: watch::Receiver<bool>,
) -> Result<DynamicImage, String> {
    check_cancel(&cancel)?;
    let attachment = current(&message, &store).await?;
    if !matches!(
        attachment.kind,
        AttachmentKind::Image | AttachmentKind::Sticker
    ) {
        return Err("No inline preview for this attachment".into());
    }
    attachment.validate()?;
    if attachment.size > MAX_BYTES {
        return Err("Preview exceeds the 16 MiB limit; use download".into());
    }
    let cache =
        cache::Cache::open(store.data_dir(), true)?.ok_or("Media busy; preview will retry")?;
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
    check_cancel(&cancel)?;
    // Read under the cache lock; decode off the UI/runtime worker and retain only a thumbnail.
    let result = tokio::task::spawn_blocking(move || {
        let bytes = read_bounded(&path)?;
        use sha2::{Digest, Sha256};
        if bytes.len() as u64 != attachment.size
            || <[u8; 32]>::from(Sha256::digest(&bytes)) != attachment.sha256
        {
            return Err("Image changed during preview loading".into());
        }
        let image = decode(&bytes)?.thumbnail(640, 640);
        drop(cache);
        Ok(image)
    })
    .await
    .map_err(|_| "Image decoder stopped")?;
    check_cancel(&cancel)?;
    current(&message, &store).await?;
    result
}
