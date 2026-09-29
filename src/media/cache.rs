use super::{Attachment, MAX_FILE_BYTES};
use crate::{
    app::model::MessageKey,
    storage::paths::{private_dir, private_file},
};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub(super) struct Cache {
    pub root: PathBuf,
    _lock: File,
}
#[derive(Serialize, Deserialize)]
pub(super) struct Manifest {
    pub key: MessageKey,
}

pub(super) fn token(key: &MessageKey, attachment: &Attachment) -> String {
    let data = serde_json::to_vec(&(key, attachment)).expect("serializable attachment identity");
    format!("{:x}", Sha256::digest(data))
}
fn managed_token(token: &str) -> bool {
    token.len() == 64
        && token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn io_error(_: impl std::fmt::Display) -> String {
    "Cannot access the media folder".into()
}

impl Cache {
    pub fn open(data_dir: &Path, create: bool) -> Result<Option<Self>, String> {
        let root = data_dir.join("media");
        if !create && !root.try_exists().map_err(io_error)? {
            return Ok(None);
        }
        private_dir(&root).map_err(io_error)?;
        let lock = private_file(&root.join(".lock")).map_err(io_error)?;
        if let Err(e) = lock.try_lock_exclusive() {
            return if e.kind() == std::io::ErrorKind::WouldBlock {
                Ok(None)
            } else {
                Err(io_error(e))
            };
        }
        let cache = Self { root, _lock: lock };
        for entry in fs::read_dir(&cache.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if entry.file_name().to_string_lossy().starts_with(".partial-") {
                fs::remove_file(entry.path()).map_err(io_error)?;
            }
        }
        Ok(Some(cache))
    }
    pub fn tokens(&self) -> Result<Vec<String>, String> {
        Ok(fs::read_dir(&self.root)
            .map_err(io_error)?
            .filter_map(|e| {
                let path = e.ok()?.path();
                let stem = path.file_stem()?.to_str()?;
                (path.extension().is_some_and(|e| e == "json") && managed_token(stem))
                    .then(|| stem.to_owned())
            })
            .collect())
    }
    pub fn manifest(&self, id: &str) -> Option<Manifest> {
        let path = self.root.join(format!("{id}.json"));
        let meta = fs::symlink_metadata(&path).ok()?;
        if !meta.is_file() || meta.len() > 64 * 1024 {
            return None;
        }
        serde_json::from_slice(&fs::read(path).ok()?).ok()
    }
    pub fn path(&self, id: &str, attachment: &Attachment) -> PathBuf {
        self.root
            .join(format!("{id}.{}", attachment.extension().unwrap_or("bin")))
    }
    pub fn remove(&self, id: &str) -> Result<(), String> {
        if !managed_token(id) {
            return Err("Invalid managed attachment identity".into());
        }
        let prefix = format!("{id}.");
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            if entry.file_name().to_string_lossy().starts_with(&prefix) {
                fs::remove_file(entry.path()).map_err(io_error)?;
            }
        }
        Ok(())
    }
    pub fn remove_orphans(&self) -> Result<(), String> {
        let tokens = self.tokens()?;
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let path = entry.map_err(io_error)?.path();
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str())
                && managed_token(stem)
                && !tokens.iter().any(|s| s == stem)
            {
                fs::remove_file(path).map_err(io_error)?;
            }
        }
        Ok(())
    }
    pub fn reserve(&self, size: u64) -> Result<(), String> {
        let mut bytes = 0u64;
        for entry in fs::read_dir(&self.root).map_err(io_error)? {
            let meta = fs::symlink_metadata(entry.map_err(io_error)?.path()).map_err(io_error)?;
            if meta.is_file() {
                bytes = bytes.saturating_add(meta.len());
            }
        }
        if self.tokens()?.len() >= 128
            || bytes.saturating_add(size).saturating_add(64 * 1024) > 512 * 1024 * 1024
        {
            return Err(format!(
                "Media folder is full; remove downloaded files from {}",
                self.root.display()
            ));
        }
        Ok(())
    }
    pub fn temporary(&self) -> Result<tempfile::NamedTempFile, String> {
        tempfile::Builder::new()
            .prefix(".partial-")
            .tempfile_in(&self.root)
            .map_err(io_error)
    }
    pub fn publish(
        &self,
        file: tempfile::NamedTempFile,
        key: &MessageKey,
        attachment: &Attachment,
    ) -> Result<PathBuf, String> {
        let id = token(key, attachment);
        let path = self.path(&id, attachment);
        file.as_file().sync_all().map_err(io_error)?;
        file.persist_noclobber(&path).map_err(io_error)?;
        let publish_manifest = || -> Result<(), String> {
            let mut manifest = self.temporary()?;
            serde_json::to_writer(&mut manifest, &Manifest { key: key.clone() })
                .map_err(io_error)?;
            manifest.flush().map_err(io_error)?;
            manifest.as_file().sync_all().map_err(io_error)?;
            manifest
                .persist_noclobber(self.root.join(format!("{id}.json")))
                .map_err(io_error)?;
            Ok(())
        };
        if let Err(error) = publish_manifest() {
            self.remove(&id)?;
            return Err(error);
        }
        Ok(path)
    }
}

pub(super) async fn verify(path: PathBuf, attachment: &Attachment) -> Result<(), String> {
    let length = attachment.size;
    let expected = attachment.sha256;
    tokio::task::spawn_blocking(move || {
        let meta = fs::symlink_metadata(&path).map_err(io_error)?;
        if !meta.is_file() || meta.len() != length || length > MAX_FILE_BYTES {
            return Err("Attachment size or file type does not match".into());
        }
        let mut file = File::open(&path).map_err(io_error)?;
        let mut hasher = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut read = 0u64;
        loop {
            let n = file.read(&mut buffer).map_err(io_error)?;
            if n == 0 {
                break;
            }
            read = read.saturating_add(n as u64);
            if read > length {
                return Err("Attachment grew during verification".into());
            }
            hasher.update(&buffer[..n]);
        }
        if read != length || <[u8; 32]>::from(hasher.finalize()) != expected {
            return Err("Attachment integrity check failed; download it again".into());
        }
        Ok(())
    })
    .await
    .map_err(io_error)?
}
