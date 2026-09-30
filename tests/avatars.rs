mod support;
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};
use whatsapp_tui::avatars::{Cache, Identity, Provider};

struct Source {
    calls: AtomicUsize,
    seen: Mutex<Vec<Identity>>,
    bytes: Mutex<Result<Option<Vec<u8>>, String>>,
}
#[async_trait::async_trait]
impl Provider for Source {
    async fn fetch(&self, identity: &Identity) -> Result<Option<Vec<u8>>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(identity.clone());
        self.bytes.lock().unwrap().clone()
    }
}
fn picture() -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        256,
        256,
        image::Rgb([80, 160, 200]),
    ))
    .write_to(&mut bytes, image::ImageFormat::Png)
    .unwrap();
    bytes.into_inner()
}
fn source() -> Source {
    Source {
        calls: AtomicUsize::new(0),
        seen: Mutex::new(vec![]),
        bytes: Mutex::new(Ok(Some(picture()))),
    }
}
fn identity(account: &str) -> Identity {
    Identity {
        account: account.into(),
        jid: "alice@s.whatsapp.net".into(),
    }
}
#[tokio::test]
async fn photos_are_small_cached_and_account_scoped() {
    let root = tempfile::tempdir().unwrap();
    let cache = Cache::new(root.path().to_owned());
    let source = source();
    let photo = cache
        .load(&source, &identity("a"), 1000)
        .await
        .unwrap()
        .unwrap();
    assert_eq!((photo.width(), photo.height()), (96, 96));
    let cached = Cache::new(root.path().to_owned())
        .load(&source, &identity("a"), 1001)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(cached.to_rgb8().get_pixel(10, 10).0, [80, 160, 200]);
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    *source.bytes.lock().unwrap() = Ok(None);
    assert!(
        cache
            .load(&source, &identity("b"), 1002)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn private_photo_removes_stale_cached_pixels() {
    let root = tempfile::tempdir().unwrap();
    let cache = Cache::new(root.path().to_owned());
    let source = source();
    cache.load(&source, &identity("a"), 1000).await.unwrap();
    *source.bytes.lock().unwrap() = Ok(None);
    assert!(
        cache
            .load(&source, &identity("a"), 86_401_000)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        Cache::new(root.path().to_owned())
            .load(&source, &identity("a"), 86_402_000)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 2);
}
#[tokio::test]
async fn damaged_and_oversized_photos_are_rejected_without_caching() {
    let root = tempfile::tempdir().unwrap();
    let cache = Cache::new(root.path().to_owned());
    let source = source();
    for bytes in [b"not a photo".to_vec(), vec![0; 1024 * 1024 + 1]] {
        *source.bytes.lock().unwrap() = Ok(Some(bytes));
        assert!(cache.load(&source, &identity("a"), 1000).await.is_err());
    }
    *source.bytes.lock().unwrap() = Ok(Some(picture()));
    assert!(
        cache
            .load(&source, &identity("a"), 1001)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 3);
}
#[tokio::test]
async fn avatar_disk_cache_stays_bounded() {
    let root = tempfile::tempdir().unwrap();
    let cache = Cache::new(root.path().to_owned());
    let source = Source {
        calls: AtomicUsize::new(0),
        seen: Mutex::new(vec![]),
        bytes: Mutex::new(Ok(None)),
    };
    for i in 0..140 {
        cache
            .load(&source, &identity(&i.to_string()), 1000 + i)
            .await
            .unwrap();
    }
    let files: Vec<_> = std::fs::read_dir(root.path().join("avatars"))
        .unwrap()
        .collect();
    assert!(files.len() <= 128);
}

#[tokio::test]
async fn visible_photos_render_pixels_then_clear_when_hidden() {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let source = Arc::new(source());
    let images = whatsapp_tui::ui::Images::default();
    let mut avatars =
        whatsapp_tui::ui::Avatars::new(root.path().to_owned(), source.clone(), &images);
    let mut terminal = Terminal::new(TestBackend::new(20, 8)).unwrap();
    for _ in 0..100 {
        avatars.poll();
        terminal
            .draw(|f| {
                avatars.begin_frame();
                avatars.draw(f, identity("a"), "Alice", Rect::new(1, 1, 4, 2), 0);
                avatars.end_frame();
            })
            .unwrap();
        if terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.bg == ratatui::style::Color::Rgb(80, 160, 200))
        {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.bg == ratatui::style::Color::Rgb(80, 160, 200))
    );
    assert_eq!(source.calls.load(Ordering::SeqCst), 1);
    avatars.begin_frame();
    avatars.end_frame();
    terminal.draw(|_| {}).unwrap();
    assert!(
        !terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.bg == ratatui::style::Color::Rgb(80, 160, 200))
    );
    avatars.stop();
}

#[tokio::test]
async fn sidebar_photos_follow_visible_chat_rows_and_their_click_targets() {
    use ratatui::{Terminal, backend::TestBackend, style::Color};
    use std::sync::Arc;
    use whatsapp_tui::{
        app::{Overlay, model::ChatSummary},
        config::Config,
        ui,
    };
    let root = tempfile::tempdir().unwrap();
    let source = Arc::new(source());
    let mut images = ui::Images::default();
    let mut avatars = ui::Avatars::new(root.path().to_owned(), source.clone(), &images);
    let mut view = support::ready_app().view();
    view.chats = (0..50)
        .map(|n| ChatSummary {
            account: "test".into(),
            chat: if n == 1 {
                "team@g.us".into()
            } else {
                format!("person{n}").into()
            },
            name: format!("Person {n}"),
            is_group: n == 1,
            ..Default::default()
        })
        .collect();
    let mut config = Config::default();
    // Four whole two-line rows and one blank row; the blank row must not fetch a photo.
    let mut terminal = Terminal::new(TestBackend::new(40, 13)).unwrap();
    let paint = |terminal: &mut Terminal<TestBackend>,
                 view: &whatsapp_tui::app::ViewModel,
                 config: &Config,
                 images: &mut ui::Images,
                 avatars: &mut ui::Avatars| {
        let mut map = None;
        terminal
            .draw(|f| map = Some(ui::render_interactive(f, view, config, images, avatars)))
            .unwrap();
        map.unwrap()
    };
    let photo_cells = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .filter(|c| c.bg == Color::Rgb(80, 160, 200))
            .count()
    };
    for selected in [0, 7] {
        view.chat = Some(view.chats[selected].chat.clone());
        for _ in 0..100 {
            avatars.poll();
            paint(&mut terminal, &view, &config, &mut images, &mut avatars);
            if photo_cells(&terminal) == 32 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(
            photo_cells(&terminal),
            32,
            "every visible chat needs its photo"
        );
        let map = paint(&mut terminal, &view, &config, &mut images, &mut avatars);
        for row in 0..4 {
            let index = if selected == 0 { row } else { row + 4 };
            assert!(
                matches!(map.hit(3, 2 + row as u16 * 2), Some(ui::interaction::Target::Chat(chat)) if chat == &view.chats[index].chat)
            );
        }
    }
    let seen = source.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 8, "only the two visible pages may fetch photos");
    assert_eq!(seen[1].jid, "team@g.us", "groups use the group's photo");
    assert!(seen.iter().all(|id| id.account == "test".into()));

    view.chat = Some(view.chats[49].chat.clone());
    for overlay in [Some(Overlay::Help), None] {
        view.overlay = overlay;
        config.media.avatars = view.overlay.is_some();
        paint(&mut terminal, &view, &config, &mut images, &mut avatars);
        assert_eq!(photo_cells(&terminal), 0);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        avatars.poll();
        assert_eq!(
            source.calls.load(Ordering::SeqCst),
            8,
            "hidden/disabled photos must not fetch"
        );
    }
    avatars.stop();
}
