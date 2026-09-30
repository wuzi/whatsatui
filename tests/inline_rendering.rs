mod support;
use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use whatsapp_tui::{
    app::model::*,
    config::Config,
    media::{Attachment, AttachmentKind},
    ui,
};

#[test]
fn media_rows_are_reserved_and_can_be_disabled() {
    let mut view = support::ready_app().view();
    view.messages[0].body = MessageBody::Media(Box::new(Attachment {
        audio: None,
        kind: AttachmentKind::Sticker,
        filename: None,
        mime: Some("image/webp".into()),
        caption: None,
        size: 128,
        direct_path: "/v/sticker".into(),
        media_key: [1; 32],
        sha256: [2; 32],
        encrypted_sha256: [3; 32],
    }));
    view.focus = whatsapp_tui::app::Focus::Messages;
    let off = Config::parse("[media]\ninline = false").unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    let plain = ui::timeline_viewport(Rect::new(0, 0, 80, 12), &view, &off).unwrap();
    let inline = ui::timeline_viewport(Rect::new(0, 0, 80, 12), &view, &Config::default()).unwrap();
    assert!(inline.max_scroll > plain.max_scroll);
    for scroll in [0, 1, 8, 100] {
        view.message_scroll = scroll;
        terminal
            .draw(|f| ui::render(f, &view, &Config::default()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Messages"));
        assert!(text.contains("Message"));
    }
}

#[test]
fn graphics_configuration_is_explicit_and_validated() {
    assert!(Config::parse("[media]\nprotocol = 'kitty'").is_ok());
    assert!(Config::parse("[media]\nprotocol = 'halfblocks'").is_ok());
    assert!(Config::parse("[media]\nprotocol = 'magic'").is_err());
}

#[tokio::test]
async fn loaded_previews_clip_clear_on_delete_and_survive_resize() {
    use sha2::{Digest, Sha256};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use whatsapp_tui::{media::Downloader, storage::Store};
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
            std::fs::write(path, include_bytes!("fixtures/sticker.webp")).unwrap();
            Ok(())
        }
    }
    let temp = tempfile::tempdir().unwrap();
    let store = Store::open(temp.path().join("chat.sqlite3")).await.unwrap();
    let mut view = support::ready_app().view();
    let bytes = include_bytes!("fixtures/sticker.webp");
    view.messages[0].body = MessageBody::Media(Box::new(Attachment {
        audio: None,
        kind: AttachmentKind::Sticker,
        filename: None,
        mime: Some("image/webp".into()),
        caption: None,
        size: bytes.len() as u64,
        direct_path: "/v/sticker".into(),
        media_key: [1; 32],
        sha256: Sha256::digest(bytes).into(),
        encrypted_sha256: [3; 32],
    }));
    store
        .apply_batch(support::batch(view.messages.clone()))
        .await
        .unwrap();
    let source = Arc::new(Source(AtomicUsize::new(0)));
    let mut images = ui::Images::new(
        store.clone(),
        source.clone(),
        whatsapp_tui::config::ImageProtocol::Halfblocks,
    );
    let config = Config::default();
    view.focus = whatsapp_tui::app::Focus::Messages;
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|f| ui::render_with_images(f, &view, &config, &mut images))
        .unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            images.poll();
            terminal
                .draw(|f| ui::render_with_images(f, &view, &config, &mut images))
                .unwrap();
            if terminal
                .backend()
                .buffer()
                .content
                .iter()
                .any(|c| c.symbol() == "▀")
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(source.0.load(Ordering::SeqCst), 1);
    for (width, height) in [(40, 12), (120, 40), (80, 24)] {
        terminal.backend_mut().resize(width, height);
        terminal.resize(Rect::new(0, 0, width, height)).unwrap();
        terminal
            .draw(|f| ui::render_with_images(f, &view, &config, &mut images))
            .unwrap();
    }
    view.messages[0].body = MessageBody::Deleted;
    terminal
        .draw(|f| ui::render_with_images(f, &view, &config, &mut images))
        .unwrap();
    assert!(
        !terminal
            .backend()
            .buffer()
            .content
            .iter()
            .any(|c| c.symbol() == "▀")
    );
    images.stop();
}
