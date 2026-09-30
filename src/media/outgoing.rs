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
    #[serde(default)]
    pub sticker: Option<StickerInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StickerInfo {
    pub animated: bool,
}

impl LocalImage {
    pub fn label(&self) -> &'static str {
        if self.sticker.is_some() {
            "sticker"
        } else {
            "image"
        }
    }
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
    Ok(data_dir.join("outgoing").join(format!(
        "{}.{}",
        image.id,
        if image.sticker.is_some() {
            "webp"
        } else {
            "jpg"
        }
    )))
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
    if let Some(info) = &image.sticker {
        let actual = sticker_info(&bytes)?;
        if info != &actual || image.width != 512 || image.height != 512 {
            return Err("Attached sticker metadata changed; attach it again".into());
        }
    }
    Ok(bytes)
}

pub fn import(source: &Path, data_dir: &Path) -> Result<LocalImage, String> {
    let filename = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "image.jpg".into());
    import_bytes(&read_bounded(source)?, filename, data_dir)
}

pub fn import_bytes(
    source: &[u8],
    filename: String,
    data_dir: &Path,
) -> Result<LocalImage, String> {
    let image = decode(source)?;
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
        filename,
        size: bytes.len() as u64,
        width: rgb.width(),
        height: rgb.height(),
        sticker: None,
    };
    persist(local, &bytes, data_dir)
}

fn persist(local: LocalImage, bytes: &[u8], data_dir: &Path) -> Result<LocalImage, String> {
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
        if entry
            .path()
            .extension()
            .is_some_and(|e| e == "jpg" || e == "webp")
        {
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
    temp.write_all(bytes)
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

/// Preserve valid WebP stickers, including animation. Other images become static stickers.
pub fn import_sticker(
    source: &[u8],
    data_dir: &Path,
    preserve: bool,
) -> Result<LocalImage, String> {
    if preserve {
        let info = sticker_info(source)?;
        return save_sticker(source, info, data_dir);
    }
    let image = decode(source)?
        .resize(512, 512, image::imageops::FilterType::Lanczos3)
        .to_rgba8();
    let mut canvas = image::RgbaImage::new(512, 512);
    image::imageops::overlay(
        &mut canvas,
        &image,
        i64::from((512 - image.width()) / 2),
        i64::from((512 - image.height()) / 2),
    );
    let encoder = webp::Encoder::from_rgba(canvas.as_raw(), 512, 512);
    for quality in [85.0, 65.0, 45.0, 25.0, 10.0] {
        let bytes = encoder
            .encode_simple(false, quality)
            .map_err(|_| "Cannot encode sticker")?;
        if bytes.len() <= 100 * 1024 {
            return save_sticker(&bytes, StickerInfo { animated: false }, data_dir);
        }
    }
    Err("This image cannot fit WhatsApp's 100 KiB sticker limit".into())
}

fn save_sticker(bytes: &[u8], sticker: StickerInfo, data_dir: &Path) -> Result<LocalImage, String> {
    persist(
        LocalImage {
            id: format!("{:x}", Sha256::digest(bytes)),
            filename: "sticker.webp".into(),
            size: bytes.len() as u64,
            width: 512,
            height: 512,
            sticker: Some(sticker),
        },
        bytes,
        data_dir,
    )
}

fn sticker_info(bytes: &[u8]) -> Result<StickerInfo, String> {
    use image::ImageDecoder;
    if bytes.len() > 500 * 1024 {
        return Err("Sticker exceeds 500 KiB".into());
    }
    let decoder = image::codecs::webp::WebPDecoder::new(std::io::Cursor::new(bytes))
        .map_err(|_| "Sticker is not a valid WebP image")?;
    if decoder.dimensions() != (512, 512) {
        return Err("Sticker must be 512 × 512 pixels".into());
    }
    let animated = decoder.has_animation();
    if !animated && bytes.len() > 100 * 1024 {
        return Err("Static sticker exceeds 100 KiB".into());
    }
    decode(bytes)?;
    if animated {
        let mut offset = 12;
        let mut duration = 0u32;
        let mut frames = 0;
        while offset + 8 <= bytes.len() {
            let size =
                u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
            let chunk = bytes
                .get(offset + 8..offset + 8 + size)
                .ok_or("Damaged sticker animation")?;
            if &bytes[offset..offset + 4] == b"ANMF" {
                if chunk.len() < 16 {
                    return Err("Damaged sticker animation".into());
                }
                let delay = u32::from_le_bytes([chunk[12], chunk[13], chunk[14], 0]);
                if delay < 8 {
                    return Err("Sticker animation frames must last at least 8 ms".into());
                }
                duration = duration.saturating_add(delay);
                frames += 1;
            }
            offset += 8 + size + (size % 2);
        }
        if frames == 0 || duration > 10_000 {
            return Err("Sticker animation must be at most 10 seconds".into());
        }
    }
    Ok(StickerInfo { animated })
}
