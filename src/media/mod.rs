mod cache;
mod download;
mod worker;
pub use download::NativeDownloader;
pub use worker::run as run_worker;
mod model;
pub mod outgoing;
pub mod preview;
use crate::{
    app::model::{MessageBody, MessageRecord},
    desktop::Desktop,
    storage::Store,
};
pub use model::{Attachment, AttachmentKind};
use tokio::sync::watch;

pub const MAX_FILE_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub enum MediaAction {
    Download,
    Open,
}

#[async_trait::async_trait]
pub trait Downloader: Send + Sync {
    async fn download(
        &self,
        attachment: &Attachment,
        destination: &std::path::Path,
        cancel: tokio::sync::watch::Receiver<bool>,
    ) -> Result<(), String>;
}

pub async fn execute(
    message: MessageRecord,
    action: MediaAction,
    store: Store,
    downloader: &dyn Downloader,
    desktop: &impl Desktop,
    cancel: watch::Receiver<bool>,
) -> Result<String, String> {
    check_cancel(&cancel)?;
    let attachment = current(&message, &store).await?;
    attachment.validate()?;
    if attachment.size > MAX_FILE_BYTES {
        return Err("Attachment exceeds the 50 MiB download limit".into());
    }
    let cache = cache::wait_open(
        store.data_dir(),
        matches!(action, MediaAction::Download),
        cancel.clone(),
    )
    .await?
    .ok_or("Download the attachment first, or wait for the current media action")?;
    cache.remove_orphans()?;
    let id = cache::token(&message.key, &attachment);
    let path = cache.path(&id, &attachment);
    let mut ready = false;
    if cache.manifest(&id).is_some_and(|m| m.key == message.key) {
        if cache::verify(path.clone(), &attachment).await.is_ok() {
            ready = true;
        } else {
            cache.remove(&id)?;
            if matches!(action, MediaAction::Open) {
                return Err("Downloaded file changed; download it again".into());
            }
        }
    } else {
        cache.remove(&id)?;
    }
    match action {
        MediaAction::Open => {
            if !ready {
                return Err("Download this attachment before opening it".into());
            }
            if attachment.extension().is_none() {
                return Err("Downloaded; opening this file type is not supported".into());
            }
            check_cancel(&cancel)?;
            current(&message, &store).await?;
            desktop.open_file(&path).await?;
            Ok("Viewer request sent".into())
        }
        MediaAction::Download => {
            if ready {
                check_cancel(&cancel)?;
                current(&message, &store).await?;
                return Ok(format!("Already downloaded: {}", path.display()));
            }
            cache.reserve(attachment.size)?;
            let temporary = cache.temporary()?;
            downloader
                .download(&attachment, temporary.path(), cancel.clone())
                .await?;
            check_cancel(&cancel)?;
            cache::verify(temporary.path().to_owned(), &attachment).await?;
            current(&message, &store).await?;
            check_cancel(&cancel)?;
            let path = cache.publish(temporary, &message.key, &attachment)?;
            Ok(format!("Downloaded attachment: {}", path.display()))
        }
    }
}

pub(super) fn check_cancel(cancel: &watch::Receiver<bool>) -> Result<(), String> {
    if *cancel.borrow() || cancel.has_changed().is_err() {
        Err("Attachment download canceled".into())
    } else {
        Ok(())
    }
}

async fn current(message: &MessageRecord, store: &Store) -> Result<Attachment, String> {
    let current = store
        .get_message(message.key.clone())
        .await
        .map_err(|e| e.to_string())?
        .ok_or("Message is no longer available")?;
    if current.key != message.key
        || current.body != message.body
        || current
            .expires_at_ms
            .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
    {
        return Err("Attachment changed or expired; reopen its actions".into());
    }
    match current.body {
        MessageBody::Media(attachment) => Ok(*attachment),
        _ => Err("Attachment is not available".into()),
    }
}

pub async fn prune(store: Store) -> Result<(), String> {
    let Some(cache) = cache::Cache::open(store.data_dir(), false)? else {
        return Ok(());
    };
    for id in cache.tokens()? {
        let Some(manifest) = cache.manifest(&id) else {
            cache.remove(&id)?;
            continue;
        };
        let current = store
            .get_message(manifest.key.clone())
            .await
            .map_err(|e| e.to_string())?;
        let valid = current.is_some_and(|m| m.key == manifest.key && !m.expires_at_ms.is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
            && matches!(&m.body, MessageBody::Media(attachment) if cache::token(&m.key, attachment) == id
                && std::fs::symlink_metadata(cache.path(&id, attachment)).is_ok_and(|meta| meta.is_file() && meta.len() == attachment.size)));
        if !valid {
            cache.remove(&id)?;
        }
    }
    cache.remove_orphans()
}
