//! Account-scoped profile thumbnails. Network access and decoding never run in a frame.
use crate::{app::model::AccountId, storage::paths::private_dir};
use image::{DynamicImage, ImageDecoder, ImageReader};
use sha2::{Digest, Sha256};
use std::{
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub const MAX_BYTES: usize = 1024 * 1024;
const DISK_BYTES: u64 = 128 * 1024;
const PHOTO_TTL: i64 = 3_600_000;
const MISSING_TTL: i64 = 300_000;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Identity {
    pub account: AccountId,
    pub jid: String,
}
impl Identity {
    pub fn token(&self) -> String {
        format!(
            "{:x}",
            Sha256::digest(
                serde_json::to_vec(&(&self.account, &self.jid)).expect("avatar identity")
            )
        )
    }
}
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    async fn fetch(&self, identity: &Identity) -> Result<Option<Vec<u8>>, String>;
}
pub struct Unavailable;
#[async_trait::async_trait]
impl Provider for Unavailable {
    async fn fetch(&self, _: &Identity) -> Result<Option<Vec<u8>>, String> {
        Ok(None)
    }
}

#[derive(Clone)]
pub struct Cache {
    root: PathBuf,
    allow_stale: Arc<AtomicBool>,
}
impl Cache {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            root: data_dir.join("avatars"),
            allow_stale: Arc::new(AtomicBool::new(true)),
        }
    }

    pub async fn load(
        &self,
        source: &dyn Provider,
        identity: &Identity,
        now_ms: i64,
    ) -> Result<Option<DynamicImage>, String> {
        let path = self.root.join(format!("{}.avatar", identity.token()));
        let read_path = path.clone();
        let previous = blocking(move || Ok(read(&read_path, now_ms))).await?;
        if let Some((expires, image)) = &previous
            && *expires > now_ms
        {
            return Ok(image.clone());
        }
        let result = tokio::time::timeout(Duration::from_secs(10), source.fetch(identity)).await;
        let bytes = match result {
            Ok(Ok(bytes)) => bytes,
            _ => {
                return previous
                    .filter(|(_, image)| image.is_none() || self.allow_stale.load(Ordering::SeqCst))
                    .map(|(_, image)| image)
                    .ok_or_else(|| "Profile photo unavailable".into());
            }
        };
        let root = self.root.clone();
        let allow_stale = self.allow_stale.clone();
        blocking(move || {
            let image = bytes.as_deref().map(decode).transpose()?;
            let ttl = if image.is_some() {
                PHOTO_TTL
            } else {
                MISSING_TTL
            };
            if let Err(error) = save(&root, &path, now_ms.saturating_add(ttl), image.as_ref()) {
                if image.is_some() {
                    return Err(error);
                }
                // A confirmed removal wins over a failed cache write. If the
                // obsolete file cannot be removed either, disable stale-photo
                // fallback for this cache's lifetime so an offline retry cannot
                // resurrect it. Fresh cached photos are still usable.
                if std::fs::remove_file(&path).is_err() {
                    allow_stale.store(false, Ordering::SeqCst);
                }
            }
            Ok(image)
        })
        .await
    }
}

async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    // Share the decoder budget with media. Keep the permit inside the blocking
    // job so cancellation cannot permit a second decoder to start early.
    let permit = crate::media::preview::DECODERS
        .acquire()
        .await
        .map_err(|_| "Photo decoder stopped")?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        job()
    })
    .await
    .map_err(|_| "Cannot prepare profile photo")?
}

fn decode(bytes: &[u8]) -> Result<DynamicImage, String> {
    if bytes.len() > MAX_BYTES {
        return Err("Profile photo exceeds 1 MiB".into());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "Invalid profile photo")?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(8 * 1024 * 1024);
    limits.max_image_width = Some(1024);
    limits.max_image_height = Some(1024);
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|_| "Invalid profile photo")?;
    let (w, h) = decoder.dimensions();
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 1_048_576 {
        return Err("Invalid profile photo dimensions".into());
    }
    let image = DynamicImage::from_decoder(decoder).map_err(|_| "Cannot decode profile photo")?;
    Ok(image.resize_to_fill(96, 96, image::imageops::FilterType::Triangle))
}

fn read(path: &Path, now: i64) -> Option<(i64, Option<DynamicImage>)> {
    let meta = std::fs::symlink_metadata(path).ok()?;
    if !meta.is_file() || meta.len() > DISK_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(DISK_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() < 8 || bytes.len() as u64 > DISK_BYTES {
        return None;
    }
    let expires = i64::from_le_bytes(bytes[..8].try_into().ok()?);
    if expires > now.saturating_add(PHOTO_TTL) {
        return None;
    }
    let image = if bytes.len() == 8 {
        None
    } else {
        Some(decode(&bytes[8..]).ok()?)
    };
    Some((expires, image))
}

fn save(
    root: &Path,
    path: &Path,
    expires: i64,
    image: Option<&DynamicImage>,
) -> Result<(), String> {
    private_dir(root).map_err(|_| "Cannot create photo cache")?;
    let mut files = std::fs::read_dir(root)
        .map_err(|_| "Cannot read photo cache")?
        .filter_map(Result::ok)
        .filter(|e| {
            e.path() != path
                && e.path().extension().is_some_and(|x| x == "avatar")
                && e.file_type().is_ok_and(|t| t.is_file())
        })
        .collect::<Vec<_>>();
    files.sort_by_key(|e| e.metadata().and_then(|m| m.modified()).ok());
    for old in files.iter().take(files.len().saturating_sub(127)) {
        std::fs::remove_file(old.path()).map_err(|_| "Cannot prune photo cache")?;
    }
    let mut bytes = Cursor::new(Vec::new());
    bytes
        .write_all(&expires.to_le_bytes())
        .map_err(|_| "Cannot save profile photo")?;
    if let Some(image) = image {
        image
            .write_to(&mut bytes, image::ImageFormat::Png)
            .map_err(|_| "Cannot save profile photo")?;
    }
    if bytes.get_ref().len() as u64 > DISK_BYTES {
        return Err("Profile thumbnail is too large".into());
    }
    let mut file =
        tempfile::NamedTempFile::new_in(root).map_err(|_| "Cannot save profile photo")?;
    file.write_all(bytes.get_ref())
        .map_err(|_| "Cannot save profile photo")?;
    file.persist(path)
        .map_err(|_| "Cannot save profile photo")?;
    Ok(())
}
