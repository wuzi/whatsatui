mod support;
use support::*;
use whatsapp_tui::{
    app::{model::*, *},
    media::outgoing,
    storage::Store,
};

fn image(dir: &std::path::Path) -> outgoing::LocalImage {
    let source = dir.join("picture.webp");
    std::fs::write(&source, include_bytes!("fixtures/sticker.webp")).unwrap();
    outgoing::import(&source, dir).unwrap()
}

#[tokio::test]
async fn attachment_only_draft_stages_and_survives_restart_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    let mut d = draft("", 3);
    d.attachment = Some(image(dir.path()));
    store
        .save_draft("test".into(), "chat".into(), d.clone())
        .await
        .unwrap();
    let sent = outbound(key("chat", "test", "image-send"), d.clone());
    store.stage_outgoing(sent.clone()).await.unwrap();
    let record = store.get_message(sent.key.clone()).await.unwrap().unwrap();
    assert!(
        matches!(record.body, MessageBody::LocalImage {ref image, ref caption} if Some(image) == d.attachment.as_ref() && caption.is_empty())
    );
    assert!(
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .attachment
            .is_none()
    );
    assert_eq!(
        store
            .stored_outbound(sent.clone())
            .await
            .unwrap()
            .draft
            .attachment,
        d.attachment
    );
    store.flush().await.unwrap();
    drop(store);
    let reopened = Store::open(path).await.unwrap();
    reopened.recover_sends("test".into()).await.unwrap();
    assert_eq!(
        reopened
            .get_message(sent.key)
            .await
            .unwrap()
            .unwrap()
            .send_state,
        Some(SendState::Unconfirmed)
    );
    let old: Draft = serde_json::from_str(r#"{"text":"old","reply":null,"revision":1}"#).unwrap();
    assert!(old.attachment.is_none());
}

#[test]
fn attach_cancel_stale_import_and_attachment_only_send() {
    let dir = tempfile::tempdir().unwrap();
    let local = image(dir.path());
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "ctrl-o");
    app.update(
        Input::Terminal(crossterm::event::Event::Paste("/tmp/my picture.png".into())),
        tokio::time::Instant::now(),
    );
    let effects = press(&mut app, "enter");
    let (request, account, chat) = effects
        .iter()
        .find_map(|e| match e {
            Effect::ImportImage {
                request,
                account,
                chat,
                ..
            } => Some((*request, account.clone(), chat.clone())),
            _ => None,
        })
        .expect("image import effect");
    press(&mut app, "esc");
    app.update(
        Input::ImageImported {
            request,
            account: account.clone(),
            chat: chat.clone(),
            result: Ok(local.clone()),
        },
        tokio::time::Instant::now(),
    );
    assert!(app.view().draft.attachment.is_none());
    press(&mut app, "ctrl-o");
    app.update(
        Input::Terminal(crossterm::event::Event::Paste("/tmp/my picture.png".into())),
        tokio::time::Instant::now(),
    );
    let effects = press(&mut app, "enter");
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::ImportImage { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::ImageImported {
            request,
            account,
            chat,
            result: Ok(local.clone()),
        },
        tokio::time::Instant::now(),
    );
    assert_eq!(app.view().draft.attachment.as_ref(), Some(&local));
    assert!(press(&mut app, "enter").iter().any(|e| matches!(e, Effect::Prepare {draft,..} if draft.attachment.is_some() && draft.text.is_empty())));
    press(&mut app, "alt-a");
    assert!(app.view().draft.attachment.is_none());
}

#[tokio::test]
async fn staging_an_image_preserves_newer_caption_and_attachment_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut captured = draft("first caption", 4);
    captured.attachment = Some(image(dir.path()));
    let mut newer = captured.clone();
    newer.revision = 5;
    newer.text = "next caption".into();
    store
        .save_draft("test".into(), "chat".into(), newer.clone())
        .await
        .unwrap();
    let sent = outbound(key("chat", "test", "old-revision"), captured);
    store.stage_outgoing(sent.clone()).await.unwrap();
    assert_eq!(
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap()
            .draft,
        newer
    );
    let record = store.get_message(sent.key).await.unwrap().unwrap();
    assert!(
        matches!(record.body, MessageBody::LocalImage {caption,..} if caption=="first caption")
    );
}

#[tokio::test]
async fn alias_merge_keeps_an_image_only_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut d = draft("", 2);
    d.attachment = Some(image(dir.path()));
    store
        .save_draft("test".into(), "alias".into(), d.clone())
        .await
        .unwrap();
    store
        .merge_alias("test".into(), "alias".into(), "chat".into())
        .await
        .unwrap();
    assert_eq!(
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .attachment,
        d.attachment
    );
}
