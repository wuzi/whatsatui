use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use std::{io::Cursor, os::unix::fs::PermissionsExt};
use whatsapp_tui::media::{outgoing, preview};
mod support;

fn png() -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(24, 16, Rgba([0, 200, 230, 255])))
        .write_to(&mut bytes, ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

#[test]
fn decode_bounds_and_corruption() {
    assert_eq!(preview::decode(&png()).unwrap().width(), 24);
    assert!(preview::decode(b"not an image").is_err());
    let mut bytes = png();
    // A PNG header declaring huge dimensions must be rejected even before decoding.
    bytes[16..20].copy_from_slice(&100_000u32.to_be_bytes());
    assert!(preview::decode(&bytes).is_err());
    assert!(preview::decode(&vec![0; 16 * 1024 * 1024 + 1]).is_err());
    let sticker = preview::decode(include_bytes!("fixtures/sticker.webp")).unwrap();
    assert_eq!((sticker.width(), sticker.height()), (128, 128));
}

#[tokio::test]
async fn outgoing_animated_sticker_preview_keeps_motion_and_accepts_cancellation() {
    let temp = tempfile::tempdir().unwrap();
    let bytes = include_bytes!("fixtures/send-sticker.webp");
    let image = outgoing::import_sticker(bytes, temp.path(), true).unwrap();
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let preview = preview::load_local(image.clone(), temp.path().to_owned(), cancel.clone())
        .await
        .unwrap();
    assert!(preview.frames.len() > 1);
    assert!(preview.frames.len() <= 96);
    assert!(
        preview
            .frames
            .iter()
            .all(|f| f.image.width() <= 160 && f.image.height() <= 160)
    );
    stop.send(true).unwrap();
    assert!(
        preview::load_local(image, temp.path().to_owned(), cancel)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn previews_verify_cache_and_reject_deleted_messages() {
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use whatsapp_tui::{
        app::model::*,
        media::{Attachment, AttachmentKind, Downloader},
        storage::Store,
    };
    struct Source(AtomicUsize);
    #[async_trait::async_trait]
    impl Downloader for Source {
        async fn download(
            &self,
            _: &Attachment,
            path: &std::path::Path,
            _: tokio::sync::watch::Receiver<bool>,
        ) -> Result<(), String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            std::fs::write(path, png()).unwrap();
            Ok(())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path().join("chat.sqlite3")).await.unwrap();
    let mut record = support::message(support::key("chat", "alice", "preview"), "");
    record.body = MessageBody::Media(Box::new(Attachment {
        audio: None,
        kind: AttachmentKind::Image,
        filename: None,
        caption: None,
        mime: Some("image/png".into()),
        size: png().len() as u64,
        direct_path: "/v/preview".into(),
        media_key: [1; 32],
        sha256: Sha256::digest(png()).into(),
        encrypted_sha256: [2; 32],
    }));
    store
        .apply_batch(support::batch(vec![record.clone()]))
        .await
        .unwrap();
    let source = Source(AtomicUsize::new(0));
    let (_stop, cancel) = tokio::sync::watch::channel(false);
    for _ in 0..2 {
        assert!(
            preview::load(record.clone(), store.clone(), &source, cancel.clone())
                .await
                .is_ok()
        );
    }
    assert_eq!(source.0.load(Ordering::SeqCst), 1);
    store
        .apply_batch(MessageBatch {
            account: "test".into(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Delete {
                key: record.key.clone(),
            }],
        })
        .await
        .unwrap();
    assert!(preview::load(record, store, &source, cancel).await.is_err());
    assert_eq!(source.0.load(Ordering::SeqCst), 1);
}

#[test]
fn imported_image_is_private_immutable_and_verifiable() {
    let temp = tempfile::tempdir().unwrap();
    let data = temp.path().join("data");
    let source = temp.path().join("my picture.png");
    std::fs::write(&source, png()).unwrap();
    let image = outgoing::import(&source, &data).unwrap();
    std::fs::write(&source, b"changed source").unwrap();
    let bytes = outgoing::read(&image, &data).unwrap();
    assert_eq!(image::guess_format(&bytes).unwrap(), ImageFormat::Jpeg);
    assert_eq!((image.width, image.height), (24, 16));
    let path = outgoing::path(&image, &data).unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    std::fs::write(&path, b"tampered").unwrap();
    assert!(outgoing::read(&image, &data).is_err());
    let mut forged = image;
    forged.id = "../../escape".into();
    assert!(outgoing::path(&forged, &data).is_err());
}
