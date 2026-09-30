//! Immutable image snapshots. Persist content IDs, never user-supplied paths.
use super::preview::{MAX_BYTES, decode, read_bounded};
use crate::storage::paths::private_dir;
use image::codecs::jpeg::JpegEncoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalImage {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub width: u32,
    pub height: u32,
}

pub fn path(image: &LocalImage, data_dir: &Path) -> Result<PathBuf, String> {
    if image.id.len() != 64
        || !image
            .id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("Invalid attached image reference".into());
    }
    Ok(data_dir.join("outgoing").join(format!("{}.jpg", image.id)))
}

pub fn read(image: &LocalImage, data_dir: &Path) -> Result<Vec<u8>, String> {
    let path = path(image, data_dir)?;
    if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file()) {
        return Err("Attached image is missing; attach it again".into());
    }
    let bytes = read_bounded(&path)?;
    if bytes.len() as u64 != image.size || format!("{:x}", Sha256::digest(&bytes)) != image.id {
        return Err("Attached image changed; attach it again".into());
    }
    Ok(bytes)
}

pub fn import(source: &Path, data_dir: &Path) -> Result<LocalImage, String> {
    let image = decode(&read_bounded(source)?)?;
    // JPEG has no alpha channel. Composite transparent stickers/images onto white.
    let rgba = image.to_rgba8();
    let mut rgb = image::RgbImage::new(rgba.width(), rgba.height());
    for (out, input) in rgb.pixels_mut().zip(rgba.pixels()) {
        let alpha = u32::from(input[3]);
        for c in 0..3 {
            out[c] = ((u32::from(input[c]) * alpha + 255 * (255 - alpha)) / 255) as u8;
        }
    }
    let mut bytes = Vec::new();
    JpegEncoder::new_with_quality(&mut bytes, 88)
        .encode_image(&rgb)
        .map_err(|_| "Cannot prepare image")?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err("Prepared image exceeds 16 MiB".into());
    }
    let local = LocalImage {
        id: format!("{:x}", Sha256::digest(&bytes)),
        filename: source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image.jpg".into()),
        size: bytes.len() as u64,
        width: rgb.width(),
        height: rgb.height(),
    };
    let root = data_dir.join("outgoing");
    private_dir(&root).map_err(|_| "Cannot create attached image folder")?;
    let dest = path(&local, data_dir)?;
    if dest.exists() {
        read(&local, data_dir)?;
        return Ok(local);
    }
    let mut used = 0u64;
    let mut count = 0usize;
    for entry in std::fs::read_dir(&root).map_err(|_| "Cannot read attached image folder")? {
        let entry = entry.map_err(|_| "Cannot read attached image folder")?;
        if entry.path().extension().is_some_and(|e| e == "jpg") {
            count += 1;
            used += entry
                .metadata()
                .map_err(|_| "Cannot read attached image")?
                .len();
        }
    }
    if count >= 128 || used + local.size > 512 * 1024 * 1024 {
        return Err("Attached image folder is full; remove unused snapshots from outgoing/".into());
    }
    let mut temp = tempfile::Builder::new()
        .prefix(".image-")
        .tempfile_in(root)
        .map_err(|_| "Cannot save attached image")?;
    temp.write_all(&bytes)
        .map_err(|_| "Cannot save attached image")?;
    temp.as_file()
        .sync_all()
        .map_err(|_| "Cannot save attached image")?;
    match temp.persist_noclobber(dest) {
        Ok(_) => {}
        Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
            read(&local, data_dir)?;
        }
        Err(_) => return Err("Cannot save attached image".into()),
    }
    Ok(local)
}
