mod support;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use support::*;
use tokio::sync::watch;
use whatsapp_tui::{
    app::model::*,
    media::{self, Attachment, AttachmentKind, AudioMetadata, Downloader},
    storage::Store,
};

fn audio() -> MessageRecord {
    let mut m = message(key("chat", "alice", "voice"), "");
    m.body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Audio,
        audio: Some(AudioMetadata {
            seconds: Some(12),
            voice: true,
        }),
        filename: None,
        mime: Some("audio/ogg; codecs=opus".into()),
        caption: None,
        size: 5,
        direct_path: "/v/audio".into(),
        media_key: [1; 32],
        encrypted_sha256: [2; 32],
        sha256: Sha256::digest(b"audio").into(),
    }));
    m
}
struct Source {
    calls: AtomicUsize,
    mutate: Option<(Store, MessageRecord)>,
    bytes: &'static [u8],
}
#[async_trait::async_trait]
impl Downloader for Source {
    async fn download(
        &self,
        _: &Attachment,
        path: &std::path::Path,
        _: watch::Receiver<bool>,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some((store, m)) = &self.mutate {
            store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        }
        std::fs::write(path, self.bytes).map_err(|e| e.to_string())
    }
}
#[tokio::test]
async fn verified_audio_is_reused_and_playback_snapshot_survives_cache_cleanup() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let m = audio();
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let source = Source {
        calls: AtomicUsize::new(0),
        mutate: None,
        bytes: b"audio",
    };
    let (_stop, cancel) = watch::channel(false);
    let first = media::audio::prepare(&m, &store, &source, cancel.clone())
        .await
        .unwrap();
    let second = media::audio::prepare(&m, &store, &source, cancel)
        .await
        .unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(first.path()).unwrap(), b"audio");
    assert_ne!(first.path(), second.path());
    // Preparation must release the cache lock; pruning cannot unlink the
    // pinned playback snapshot or block a different media action.
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        media::prune(store.clone()),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(std::fs::read(second.path()).unwrap(), b"audio");
    let path = first.path().to_owned();
    drop(first);
    assert!(!path.exists());
}
#[tokio::test]
async fn corrupt_or_expired_downloads_never_produce_playable_files() {
    for expired in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
        let m = audio();
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        let mut stale = m.clone();
        stale.body = MessageBody::Expired;
        let source = Source {
            calls: AtomicUsize::new(0),
            mutate: expired.then_some((store.clone(), stale)),
            bytes: if expired { b"audio" } else { b"wrong" },
        };
        let (_stop, cancel) = watch::channel(false);
        assert!(
            media::audio::prepare(&m, &store, &source, cancel)
                .await
                .is_err()
        );
    }
}
#[tokio::test]
async fn cancelled_or_non_audio_requests_do_not_download() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let m = audio();
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let source = Source {
        calls: AtomicUsize::new(0),
        mutate: None,
        bytes: b"audio",
    };
    let (_stop, cancel) = watch::channel(true);
    assert!(
        media::audio::prepare(&m, &store, &source, cancel)
            .await
            .is_err()
    );
    let (_stop, cancel) = watch::channel(false);
    assert!(
        media::audio::prepare(
            &message(key("chat", "alice", "text"), "hello"),
            &store,
            &source,
            cancel
        )
        .await
        .is_err()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn legacy_attachments_and_audio_quotes_keep_their_meaning() {
    let MessageBody::Media(a) = audio().body else {
        panic!()
    };
    assert_eq!(a.extension(), Some("ogg"));
    let mut json = serde_json::to_value(&a).unwrap();
    json.as_object_mut().unwrap().remove("audio");
    json["kind"] = serde_json::json!("Image");
    assert!(
        serde_json::from_value::<Attachment>(json)
            .unwrap()
            .audio
            .is_none()
    );
    let quote = whatsapp_tui::message_actions::quote(&audio(), 0).unwrap();
    assert_eq!(quote.media_kind, Some(AttachmentKind::Audio));
    assert!(quote.preview.contains("audio"));
}
