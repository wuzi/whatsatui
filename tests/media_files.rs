mod support;
use std::{
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use support::*;
use tokio::sync::watch;
use whatsapp_tui::{
    app::model::*,
    desktop::Desktop,
    media::{self, Attachment, AttachmentKind, Downloader, MediaAction},
    storage::Store,
};

fn attachment() -> Attachment {
    Attachment {
        kind: AttachmentKind::Document,
        filename: Some("../../outside;$(touch nope).txt".into()),
        mime: Some("text/plain".into()),
        caption: None,
        size: 6,
        direct_path: "/v/document?token=private".into(),
        media_key: [1; 32],
        encrypted_sha256: [2; 32],
        sha256: [
            0x58, 0x91, 0xb5, 0xb5, 0x22, 0xd5, 0xdf, 0x08, 0x6d, 0x0f, 0xf0, 0xb1, 0x10, 0xfb,
            0xd9, 0xd2, 0x1b, 0xb4, 0xfc, 0x71, 0x63, 0xaf, 0x34, 0xd0, 0x82, 0x86, 0xa2, 0xe8,
            0x46, 0xf6, 0xbe, 0x03,
        ],
    }
}
fn record(id: &str) -> MessageRecord {
    let mut m = message(key("chat", "alice", id), "");
    m.body = MessageBody::Media(Box::new(attachment()));
    m
}
#[derive(Default)]
struct Viewer(Mutex<Vec<PathBuf>>);
#[async_trait::async_trait]
impl Desktop for Viewer {
    async fn copy(&self, _: &str) -> Result<(), String> {
        panic!("unexpected clipboard action")
    }
    async fn open(&self, _: &str) -> Result<(), String> {
        panic!("unexpected browser action")
    }
    async fn open_file(&self, path: &Path) -> Result<(), String> {
        assert_eq!(std::fs::read(path).unwrap(), b"hello\n");
        self.0.lock().unwrap().push(path.to_owned());
        Ok(())
    }
}
struct Source {
    bytes: &'static [u8],
    calls: AtomicUsize,
    fail: bool,
    mutate: Option<(Store, MessageRecord)>,
}
impl Source {
    fn good() -> Self {
        Self {
            bytes: b"hello\n",
            calls: AtomicUsize::new(0),
            fail: false,
            mutate: None,
        }
    }
}
#[async_trait::async_trait]
impl Downloader for Source {
    async fn download(
        &self,
        _: &Attachment,
        destination: &Path,
        _: watch::Receiver<bool>,
    ) -> Result<(), String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        std::fs::write(destination, self.bytes).unwrap();
        if let Some((store, m)) = &self.mutate {
            store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        }
        if self.fail {
            Err("download failed".into())
        } else {
            Ok(())
        }
    }
}
async fn act(
    m: &MessageRecord,
    action: MediaAction,
    store: &Store,
    source: &Source,
    viewer: &Viewer,
) -> Result<String, String> {
    let (_stop, cancel) = watch::channel(false);
    media::execute(m.clone(), action, store.clone(), source, viewer, cancel).await
}
fn payloads(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir.join("media"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().unwrap() != ".lock" && p.extension().is_some_and(|e| e != "json"))
        .collect()
}

#[tokio::test]
async fn downloads_privately_without_opening_and_reuses_verified_bytes() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let m = record("one");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    assert!(
        act(&m, MediaAction::Open, &store, &source, &viewer)
            .await
            .is_err()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    act(&m, MediaAction::Download, &store, &source, &viewer)
        .await
        .unwrap();
    assert!(viewer.0.lock().unwrap().is_empty());
    act(&m, MediaAction::Download, &store, &source, &viewer)
        .await
        .unwrap();
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    act(&m, MediaAction::Open, &store, &source, &viewer)
        .await
        .unwrap();
    let path = viewer.0.lock().unwrap()[0].clone();
    assert_eq!(path.parent().unwrap(), dir.path().join("media"));
    assert!(
        !path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("outside")
    );
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        std::fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(payloads(dir.path()).len(), 1);
}

#[tokio::test]
async fn failed_wrong_hash_and_wrong_length_downloads_never_publish() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let m = record("bad");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let viewer = Viewer::default();
    for (bytes, fail) in [
        (b"jello\n".as_slice(), false),
        (b"hello\n!".as_slice(), false),
        (b"hello\n".as_slice(), true),
    ] {
        let source = Source {
            bytes,
            fail,
            ..Source::good()
        };
        assert!(
            act(&m, MediaAction::Download, &store, &source, &viewer)
                .await
                .is_err()
        );
        assert!(payloads(dir.path()).is_empty());
    }
    assert!(viewer.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn stale_records_and_mid_transfer_changes_do_not_publish() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let viewer = Viewer::default();
    for (i, body) in [
        MessageBody::Deleted,
        MessageBody::Expired,
        MessageBody::Text("edited".into()),
    ]
    .into_iter()
    .enumerate()
    {
        let m = record(&format!("stale{i}"));
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        let mut updated = m.clone();
        updated.body = body;
        updated.edited_at_ms = Some(1_900_000_000_000);
        let source = Source {
            mutate: Some((store.clone(), updated)),
            ..Source::good()
        };
        assert!(
            act(&m, MediaAction::Download, &store, &source, &viewer)
                .await
                .is_err()
        );
        assert!(payloads(dir.path()).is_empty());
        let unused = Source::good();
        assert!(
            act(&m, MediaAction::Download, &store, &unused, &viewer)
                .await
                .is_err()
        );
        assert_eq!(unused.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn corrupt_and_symlinked_cached_files_are_never_opened() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let m = record("corrupt");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    act(&m, MediaAction::Download, &store, &source, &viewer)
        .await
        .unwrap();
    let file = payloads(dir.path()).pop().unwrap();
    std::fs::write(&file, b"jello\n").unwrap();
    assert!(
        act(&m, MediaAction::Open, &store, &source, &viewer)
            .await
            .is_err()
    );
    act(&m, MediaAction::Download, &store, &source, &viewer)
        .await
        .unwrap();
    let outside = dir.path().join("outside.txt");
    std::fs::write(&outside, b"hello\n").unwrap();
    std::fs::remove_file(&file).unwrap();
    symlink(&outside, &file).unwrap();
    assert!(
        act(&m, MediaAction::Open, &store, &source, &viewer)
            .await
            .is_err()
    );
    assert_eq!(std::fs::read(outside).unwrap(), b"hello\n");
    assert!(viewer.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn unknown_file_types_download_but_cannot_launch_a_program() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut m = record("unknown");
    let MessageBody::Media(a) = &mut m.body else {
        unreachable!()
    };
    a.filename = Some("launcher.desktop".into());
    a.mime = Some("application/x-desktop".into());
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    act(&m, MediaAction::Download, &store, &source, &viewer)
        .await
        .unwrap();
    assert_eq!(payloads(dir.path())[0].extension().unwrap(), "bin");
    assert!(
        act(&m, MediaAction::Open, &store, &source, &viewer)
            .await
            .is_err()
    );
    assert!(viewer.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn size_and_storage_limits_reject_before_network_work() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut large = record("large");
    let MessageBody::Media(a) = &mut large.body else {
        unreachable!()
    };
    a.size = 50 * 1024 * 1024 + 1;
    store.apply_batch(batch(vec![large.clone()])).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    assert!(
        act(&large, MediaAction::Download, &store, &source, &viewer)
            .await
            .is_err()
    );
    std::fs::create_dir_all(dir.path().join("media")).unwrap();
    std::fs::File::create(dir.path().join("media/occupied.bin"))
        .unwrap()
        .set_len(512 * 1024 * 1024)
        .unwrap();
    let m = record("full");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    assert!(
        act(&m, MediaAction::Download, &store, &source, &viewer)
            .await
            .is_err()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn deleted_expired_and_aliased_media_are_removed_from_managed_storage() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    for id in ["deleted", "expired", "aliased"] {
        let mut m = record(id);
        m.key.chat = id.into();
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        act(&m, MediaAction::Download, &store, &source, &viewer)
            .await
            .unwrap();
        match id {
            "deleted" => {
                m.body = MessageBody::Deleted;
                store.apply_batch(batch(vec![m.clone()])).await.unwrap();
            }
            "expired" => {
                m.expires_at_ms = Some(1);
                store.apply_batch(batch(vec![m.clone()])).await.unwrap();
            }
            _ => {
                store
                    .merge_alias(account("test"), "aliased".into(), "canonical".into())
                    .await
                    .unwrap();
            }
        }
        media::prune(store.clone()).await.unwrap();
        assert!(payloads(dir.path()).is_empty(), "{id}");
        assert!(
            act(&m, MediaAction::Open, &store, &source, &viewer)
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn cancellation_and_attachment_count_limit_leave_no_partial_files() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let source = Source::good();
    let viewer = Viewer::default();
    let m = record("canceled");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let (_stop, cancel) = watch::channel(true);
    assert!(
        media::execute(
            m,
            MediaAction::Download,
            store.clone(),
            &source,
            &viewer,
            cancel
        )
        .await
        .is_err()
    );
    assert!(payloads(dir.path()).is_empty());
    for i in 0..128 {
        let m = record(&format!("item{i}"));
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        act(&m, MediaAction::Download, &store, &source, &viewer)
            .await
            .unwrap();
    }
    let m = record("over-limit");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    assert!(
        act(&m, MediaAction::Download, &store, &source, &viewer)
            .await
            .is_err()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 128);
    assert_eq!(payloads(dir.path()).len(), 128);
}
