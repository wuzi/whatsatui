mod support;
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tokio::sync::watch;
use whatsapp_tui::{
    app::model::*,
    config::{Config, ImageProtocol},
    media::{Attachment, AttachmentKind, Downloader, preview},
    storage::Store,
    ui,
};

fn message(kind: AttachmentKind, mime: &str, bytes: &[u8]) -> MessageRecord {
    let mut m = support::message(support::key("chat", "alice", "gif"), "");
    m.body = MessageBody::Media(Box::new(Attachment {
        kind,
        audio: None,
        filename: None,
        caption: Some("An inline loop".into()),
        mime: Some(mime.into()),
        size: bytes.len() as u64,
        direct_path: "/v/gif".into(),
        media_key: [1; 32],
        sha256: Sha256::digest(bytes).into(),
        encrypted_sha256: [2; 32],
    }));
    m
}
struct Source {
    bytes: &'static [u8],
    calls: AtomicUsize,
}

#[tokio::test]
async fn ordinary_videos_stay_explicit_and_gif_downloads_are_verified() {
    let root = tempfile::tempdir().unwrap();
    let store = Store::open(root.path().join("chat.sqlite3")).await.unwrap();
    let video = message(AttachmentKind::Video, "video/mp4", b"expected");
    store
        .apply_batch(support::batch(vec![video.clone()]))
        .await
        .unwrap();
    let source = Source {
        bytes: b"tampered",
        calls: AtomicUsize::new(0),
    };
    let (_stop, cancel) = watch::channel(false);
    let error = preview::load(video, store.clone(), &source, cancel.clone())
        .await
        .err()
        .unwrap();
    assert!(error.contains("No inline preview"));
    assert_eq!(source.calls.load(Ordering::SeqCst), 0);
    let gif = message(AttachmentKind::Gif, "video/mp4", b"expected");
    store
        .apply_batch(support::batch(vec![gif.clone()]))
        .await
        .unwrap();
    let error = preview::load(gif.clone(), store.clone(), &source, cancel.clone())
        .await
        .err()
        .unwrap();
    assert!(error.contains("integrity check failed"), "{error}");
    store
        .apply_batch(MessageBatch {
            account: gif.key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Delete {
                key: gif.key.clone(),
            }],
        })
        .await
        .unwrap();
    assert!(preview::load(gif, store, &source, cancel).await.is_err());
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
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
        std::fs::write(path, self.bytes).map_err(|e| e.to_string())
    }
}

#[test]
fn gifs_reserve_preview_rows_and_do_not_launch_playback_on_a_click() {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    for (kind, mime) in [
        (AttachmentKind::Gif, "video/mp4"),
        (AttachmentKind::Image, "image/gif"),
        (AttachmentKind::Document, "IMAGE/GIF; version=89a"),
    ] {
        let mut view = support::ready_app().view();
        view.messages = vec![message(kind, mime, b"synthetic")];
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        let config = Config::default();
        let off = Config::parse("[media]\ninline = false").unwrap();
        assert!(
            ui::timeline_viewport(Rect::new(0, 0, 80, 12), &view, &config)
                .unwrap()
                .max_scroll
                > ui::timeline_viewport(Rect::new(0, 0, 80, 12), &view, &off)
                    .unwrap()
                    .max_scroll
        );
        let mut hits = None;
        terminal
            .draw(|f| {
                hits = Some(ui::render_interactive(
                    f,
                    &view,
                    &config,
                    &mut ui::Images::default(),
                    &mut ui::Avatars::default(),
                ))
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("[gif]"));
        assert!(!text.contains("[Play]"));
        let hits = hits.unwrap();
        for y in 0..24 {
            for x in 0..80 {
                assert!(!matches!(
                    hits.hit(x, y),
                    Some(ui::interaction::Target::PlayAudio(_))
                ));
            }
        }
    }
}

#[tokio::test]
#[ignore = "requires ffmpeg and ffprobe; synthetic fixtures only"]
async fn native_gif_previews_verify_reuse_animate_and_stop_when_hidden() {
    use ratatui::{Terminal, backend::TestBackend};
    for (kind, mime, bytes) in [
        (
            AttachmentKind::Gif,
            "video/mp4",
            include_bytes!("fixtures/loop.mp4").as_slice(),
        ),
        (
            AttachmentKind::Document,
            "image/gif",
            include_bytes!("fixtures/loop.gif").as_slice(),
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let store = Store::open(root.path().join("chat.sqlite3")).await.unwrap();
        let message = message(kind, mime, bytes);
        store
            .apply_batch(support::batch(vec![message.clone()]))
            .await
            .unwrap();
        let source = Arc::new(Source {
            bytes,
            calls: AtomicUsize::new(0),
        });
        let (stop, cancel) = watch::channel(false);
        let animation = preview::load(
            message.clone(),
            store.clone(),
            source.as_ref(),
            cancel.clone(),
        )
        .await
        .unwrap();
        assert!((2..=96).contains(&animation.frames.len()));
        assert_eq!(animation.loops, None);
        assert_eq!(
            animation.frames.iter().map(|f| f.duration_ms).sum::<u64>(),
            1200
        );
        assert_ne!(
            animation.frames.first().unwrap().image,
            animation.frames.last().unwrap().image
        );
        assert!(
            animation
                .frames
                .iter()
                .all(|f| f.image.width() <= 160 && f.image.height() <= 160)
        );
        let mut images = ui::Images::new(store.clone(), source.clone(), ImageProtocol::Halfblocks);
        let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
        let mut colors = std::collections::HashSet::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(4);
        while std::time::Instant::now() < deadline && colors.len() < 2 {
            images.poll();
            terminal
                .draw(|f| {
                    images.begin_frame();
                    images.draw(f, &message, f.area(), 0, f.area().as_size());
                    images.end_frame();
                })
                .unwrap();
            for cell in &terminal.backend().buffer().content {
                if let ratatui::style::Color::Rgb(r, _, b) = cell.fg
                    && r.abs_diff(b) > 100
                {
                    colors.insert(if r > b { "red" } else { "blue" });
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        }
        assert_eq!(colors.len(), 2, "GIF remained static");
        assert_eq!(source.calls.load(Ordering::SeqCst), 1);
        images.begin_frame();
        images.end_frame();
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        assert!(!images.poll());
        stop.send(true).unwrap();
        assert!(
            preview::load(message, store, source.as_ref(), cancel)
                .await
                .is_err()
        );
    }
}
