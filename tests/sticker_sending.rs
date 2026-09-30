mod support;
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, view_model::*, *},
    media::{Attachment, AttachmentKind, outgoing},
    storage::Store,
};

fn sticker(dir: &std::path::Path) -> outgoing::LocalImage {
    outgoing::import_sticker(include_bytes!("fixtures/send-sticker.webp"), dir, true).unwrap()
}

fn received(id: &str) -> MessageRecord {
    use sha2::{Digest, Sha256};
    let bytes = include_bytes!("fixtures/send-sticker.webp");
    let mut m = message(key("source", "alice", id), "");
    m.body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Sticker,
        mime: Some("image/webp".into()),
        filename: None,
        caption: None,
        size: bytes.len() as u64,
        direct_path: "/o1/v/test-sticker".into(),
        media_key: [1; 32],
        sha256: Sha256::digest(bytes).into(),
        encrypted_sha256: [2; 32],
    }));
    m
}

#[test]
fn pasted_stickers_have_transparent_padding_and_received_animation_is_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let mut source = Vec::new();
    image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
        400,
        200,
        image::Rgba([255, 0, 0, 255]),
    ))
    .write_to(
        &mut std::io::Cursor::new(&mut source),
        image::ImageFormat::Png,
    )
    .unwrap();
    let local = outgoing::import_sticker(&source, dir.path(), false).unwrap();
    assert!(local.sticker.as_ref().is_some_and(|s| !s.animated));
    assert!(local.size <= 100 * 1024);
    let bytes = outgoing::read(&local, dir.path()).unwrap();
    let decoded = image::load_from_memory(&bytes).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (512, 512));
    assert_eq!(decoded.get_pixel(0, 0)[3], 0);
    assert_eq!(decoded.get_pixel(256, 256)[3], 255);
    let animated = sticker(dir.path());
    assert!(animated.sticker.as_ref().unwrap().animated);
    assert_eq!(
        outgoing::read(&animated, dir.path()).unwrap(),
        include_bytes!("fixtures/send-sticker.webp")
    );
    let mut forged = animated.clone();
    forged.sticker.as_mut().unwrap().animated = false;
    assert!(outgoing::read(&forged, dir.path()).is_err());
    assert!(outgoing::import_sticker(b"bad", dir.path(), true).is_err());
    assert!(
        outgoing::import_sticker(include_bytes!("fixtures/sticker.webp"), dir.path(), true)
            .is_err()
    );
}

fn open(app: &mut App, items: Vec<MessageRecord>) {
    let effects = press(app, "ctrl-s");
    let Effect::LoadStickers {
        request,
        account,
        chat,
    } = effects
        .into_iter()
        .find(|e| matches!(e, Effect::LoadStickers { .. }))
        .unwrap()
    else {
        panic!()
    };
    app.update(
        Input::Store(StoreCompletion::Stickers {
            request,
            account,
            chat,
            result: Ok(items),
        }),
        Instant::now(),
    );
}

#[test]
fn sending_a_pasted_sticker_preserves_text_and_existing_image_draft() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "x");
    let Effect::PasteClipboard {
        request,
        account,
        chat,
        ..
    } = press(&mut app, "ctrl-v").remove(0)
    else {
        panic!()
    };
    let photo = outgoing::import_bytes(
        include_bytes!("fixtures/sticker.webp"),
        "photo.png".into(),
        dir.path(),
    )
    .unwrap();
    app.update(
        Input::ClipboardRead {
            request,
            account,
            chat,
            result: Ok(whatsapp_tui::desktop::clipboard::Paste::Image(Box::new(
                photo,
            ))),
        },
        Instant::now(),
    );
    let original = app.view().draft;
    open(&mut app, vec![]);
    let Effect::PasteClipboard {
        request,
        account,
        chat,
        sticker: true,
    } = press(&mut app, "ctrl-v").remove(0)
    else {
        panic!()
    };
    app.update(
        Input::ClipboardRead {
            request,
            account,
            chat,
            result: Ok(whatsapp_tui::desktop::clipboard::Paste::Image(Box::new(
                sticker(dir.path()),
            ))),
        },
        Instant::now(),
    );
    assert_eq!(app.view().draft, original);
    assert!(
        matches!(app.view().overlay, Some(Overlay::Stickers(p)) if matches!(&p.items[0], StickerChoice::Local(_)))
    );
    let Effect::Prepare { request, draft, .. } = press(&mut app, "enter").remove(0) else {
        panic!()
    };
    assert!(draft.text.is_empty());
    assert!(draft.attachment.as_ref().unwrap().sticker.is_some());
    let sent = outbound(key("chat", "test", "new-sticker"), draft);
    let staged = app.update(
        Input::Backend(whatsapp_tui::whatsapp::BackendEvent::Prepared {
            request,
            message: Box::new(sent.clone()),
        }),
        Instant::now(),
    );
    assert!(staged.iter().any(|e| matches!(
        e,
        Effect::Stage {
            preserve_draft: true,
            ..
        }
    )));
    app.update(
        Input::Store(StoreCompletion::Staged {
            request,
            message: Box::new(sent),
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert_eq!(app.view().draft, original);
}

#[test]
fn cancelled_sticker_send_ignores_late_import() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = ready_app();
    press(&mut app, "enter");
    open(&mut app, vec![received("one")]);
    let Effect::ImportSticker {
        request,
        account,
        chat,
        ..
    } = press(&mut app, "enter").remove(0)
    else {
        panic!()
    };
    assert!(press(&mut app, "enter").is_empty());
    press(&mut app, "esc");
    let effects = app.update(
        Input::StickerImported {
            request,
            account,
            chat,
            result: Ok(sticker(dir.path())),
        },
        Instant::now(),
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::Prepare { .. })));
    assert!(app.view().draft.attachment.is_none());
}

#[tokio::test]
async fn recent_stickers_are_account_scoped_deduplicated_and_survive_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    let first = received("first");
    let mut latest = received("latest");
    latest.created_at_ms += 1;
    let mut expired = received("expired");
    expired.expires_at_ms = Some(1);
    let mut deleted = received("deleted");
    deleted.body = MessageBody::Deleted;
    store
        .apply_batch(batch(vec![first, latest.clone(), expired, deleted]))
        .await
        .unwrap();
    assert!(
        store
            .recent_stickers("other".into(), 10)
            .await
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store.recent_stickers("test".into(), 10).await.unwrap(),
        vec![latest]
    );
    let d = draft("keep this draft", 2);
    store
        .save_draft("test".into(), "chat".into(), d.clone())
        .await
        .unwrap();
    let sent = outbound(
        key("chat", "test", "sent"),
        Draft {
            attachment: Some(Box::new(sticker(dir.path()))),
            ..Default::default()
        },
    );
    store.stage_resend(sent.clone()).await.unwrap();
    assert_eq!(
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap()
            .draft,
        d
    );
    store.flush().await.unwrap();
    drop(store);
    let reopened = Store::open(path).await.unwrap();
    reopened.recover_sends("test".into()).await.unwrap();
    let stored = reopened
        .get_message(sent.key.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.send_state, Some(SendState::Unconfirmed));
    assert!(matches!(stored.body, MessageBody::LocalImage {image,..} if image.sticker.is_some()));
    assert_eq!(
        reopened
            .stored_outbound(sent.clone())
            .await
            .unwrap()
            .draft
            .attachment,
        sent.draft.attachment
    );
}

#[test]
fn composer_opens_a_sticker_picker_without_changing_the_draft() {
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "x");
    press(&mut app, "ctrl-s");
    assert!(
        app.view().overlay.is_some(),
        "Ctrl-S must open the sticker picker"
    );
    assert_eq!(app.view().draft.text, "x");
}

#[test]
fn changes_in_a_stickers_source_conversation_refresh_the_open_picker() {
    let mut app = ready_app();
    press(&mut app, "enter");
    open(&mut app, vec![received("deleted-remotely")]);
    let effects = app.update(
        Input::Backend(whatsapp_tui::whatsapp::BackendEvent::StoreChanged(
            StoreChange {
                account: "test".into(),
                chats: vec!["source".into()],
            },
        )),
        Instant::now(),
    );
    let Effect::LoadStickers {
        request,
        account,
        chat,
    } = effects
        .into_iter()
        .find(|e| matches!(e, Effect::LoadStickers { .. }))
        .expect("A source conversation change must refresh sticker availability")
    else {
        panic!()
    };
    assert!(press(&mut app, "enter").is_empty());
    app.update(
        Input::Store(StoreCompletion::Stickers {
            request,
            account,
            chat,
            result: Ok(vec![]),
        }),
        Instant::now(),
    );
    assert!(matches!(app.view().overlay, Some(Overlay::Stickers(p)) if p.items.is_empty()));
}
