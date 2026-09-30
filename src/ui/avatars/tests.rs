use super::*;
use image::{DynamicImage, Rgb, RgbImage};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Source {
    photo: Mutex<Option<Vec<u8>>>,
    calls: AtomicUsize,
}
#[async_trait::async_trait]
impl Provider for Source {
    async fn fetch(&self, _: &Identity) -> Result<Option<Vec<u8>>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.photo.lock().unwrap().clone())
    }
}
fn picture(color: [u8; 3]) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::from_pixel(96, 96, Rgb(color)))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}
fn identity() -> Identity {
    Identity {
        account: "test".into(),
        jid: "alice@s.whatsapp.net".into(),
    }
}

struct Harness {
    avatars: Avatars,
    terminal: Terminal<TestBackend>,
    source: Arc<Source>,
    root: tempfile::TempDir,
}
impl Harness {
    async fn new(kitty: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = Arc::new(Source {
            photo: Mutex::new(Some(picture([80, 160, 200]))),
            calls: AtomicUsize::new(0),
        });
        let mut avatars = Avatars::new(
            root.path().to_owned(),
            source.clone(),
            &crate::ui::Images::default(),
        );
        avatars.kitty = kitty;
        let mut harness = Self {
            avatars,
            terminal: Terminal::new(TestBackend::new(20, 8)).unwrap(),
            source,
            root,
        };
        harness.draw();
        harness.settle().await;
        harness
    }
    fn draw(&mut self) -> Buffer {
        self.terminal
            .draw(|f| {
                self.avatars.begin_frame();
                self.avatars
                    .draw(f, identity(), "Alice", Rect::new(1, 1, 4, 2), 0);
                self.avatars.end_frame();
            })
            .unwrap();
        self.terminal.backend().buffer().clone()
    }
    async fn settle(&mut self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            while self.avatars.active.is_some() {
                tokio::time::sleep(Duration::from_millis(5)).await;
                self.avatars.poll();
            }
        })
        .await
        .expect("avatar refresh must finish");
        self.draw();
        // Kitty's first frame contains the one-time transmission; compare its
        // stable placeholder frame when testing whether a refresh changes it.
        self.draw();
    }
    fn expire(&mut self, disk: bool) {
        self.avatars
            .cache
            .get_mut(&identity().token())
            .unwrap()
            .refresh = Instant::now();
        if disk {
            let path = self
                .root
                .path()
                .join("avatars")
                .join(format!("{}.avatar", identity().token()));
            let mut bytes = std::fs::read(&path).unwrap();
            bytes[..8].copy_from_slice(&0_i64.to_le_bytes());
            std::fs::write(path, bytes).unwrap();
        }
    }
}
fn has_photo(buffer: &Buffer) -> bool {
    buffer.content.iter().any(|c| {
        c.symbol().contains('\u{10eeee}') || matches!(c.bg, ratatui::style::Color::Rgb(..))
    })
}

#[tokio::test]
async fn periodic_refresh_keeps_pixels_and_does_not_retransmit_unchanged_photos() {
    for kitty in [false, true] {
        let mut h = Harness::new(kitty).await;
        let before = h.draw();
        assert!(has_photo(&before));
        for disk in [false, true, false] {
            h.expire(disk);
            h.avatars.poll();
            assert_eq!(
                h.draw(),
                before,
                "refresh must not show initials or new image data"
            );
            assert!(
                h.avatars.take_cleanup().is_empty(),
                "visible photo was deleted"
            );
            h.settle().await;
            assert_eq!(
                h.draw(),
                before,
                "unchanged photo must keep its terminal image ID"
            );
            assert!(h.avatars.take_cleanup().is_empty());
        }
        assert_eq!(h.source.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn changed_photo_replaces_pixels_only_after_refresh_finishes() {
    for kitty in [false, true] {
        let mut h = Harness::new(kitty).await;
        let before = h.draw();
        *h.source.photo.lock().unwrap() = Some(picture([200, 80, 160]));
        h.expire(true);
        h.avatars.poll();
        assert_eq!(h.draw(), before, "old photo must remain during refresh");
        assert!(h.avatars.take_cleanup().is_empty());
        h.settle().await;
        let after = h.draw();
        assert!(has_photo(&after));
        assert_ne!(after, before, "a changed profile photo must still update");
        assert_eq!(h.source.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            h.avatars.take_cleanup().matches("a=d,d=I").count(),
            usize::from(kitty)
        );
        assert!(h.avatars.take_cleanup().is_empty());
    }
}

#[tokio::test]
async fn failed_refresh_keeps_photo_and_allows_a_later_retry() {
    for kitty in [false, true] {
        let mut h = Harness::new(kitty).await;
        let before = h.draw();
        *h.source.photo.lock().unwrap() = Some(b"invalid image".to_vec());
        h.expire(true);
        h.avatars.poll();
        h.settle().await;
        assert_eq!(
            h.draw(),
            before,
            "failed refresh must not replace a usable photo"
        );
        assert!(h.avatars.take_cleanup().is_empty());
        assert_eq!(
            h.source.calls.load(Ordering::SeqCst),
            2,
            "failure must back off"
        );
        *h.source.photo.lock().unwrap() = Some(picture([200, 80, 160]));
        h.expire(true);
        h.avatars.poll();
        h.settle().await;
        assert_ne!(
            h.draw(),
            before,
            "a later successful refresh must be visible"
        );
        assert_eq!(h.source.calls.load(Ordering::SeqCst), 3);
    }
}

#[tokio::test]
async fn confirmed_missing_photo_removes_old_pixels_and_uses_initials() {
    for kitty in [false, true] {
        let mut h = Harness::new(kitty).await;
        *h.source.photo.lock().unwrap() = None;
        h.expire(true);
        h.avatars.poll();
        h.settle().await;
        let after = h.draw();
        assert!(!has_photo(&after));
        let text: String = after.content.iter().map(|c| c.symbol()).collect();
        assert!(text.contains("AL"));
        assert_eq!(
            h.avatars.take_cleanup().matches("a=d,d=I").count(),
            usize::from(kitty)
        );
        assert_eq!(h.source.calls.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn hiding_an_avatar_during_refresh_cancels_it_and_releases_its_image() {
    let mut h = Harness::new(true).await;
    h.expire(false);
    h.avatars.poll();
    h.avatars.begin_frame();
    h.avatars.end_frame();
    h.terminal.draw(|_| {}).unwrap();
    assert!(!has_photo(h.terminal.backend().buffer()));
    assert_eq!(h.avatars.take_cleanup().matches("a=d,d=I").count(), 1);
    assert!(h.avatars.active.is_none());
    assert!(!h.avatars.poll());
    assert!(h.avatars.take_cleanup().is_empty());
}

#[tokio::test]
async fn confirmed_missing_photo_is_cleared_even_when_cache_write_fails() {
    for kitty in [false, true] {
        let mut h = Harness::new(kitty).await;
        *h.source.photo.lock().unwrap() = None;
        h.expire(true);
        let path = h
            .root
            .path()
            .join("avatars")
            .join(format!("{}.avatar", identity().token()));
        // A directory at the destination forces persist() to fail regardless
        // of the test user's filesystem privileges.
        std::fs::remove_file(&path).unwrap();
        std::fs::create_dir(&path).unwrap();
        h.avatars.poll();
        h.settle().await;
        assert!(
            !has_photo(&h.draw()),
            "confirmed removal must override a cache-write error"
        );
        assert_eq!(
            h.avatars.take_cleanup().matches("a=d,d=I").count(),
            usize::from(kitty)
        );
    }
}
