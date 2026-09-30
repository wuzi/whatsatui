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

#[test]
fn clipboard_completion_keeps_new_typing_and_respects_removal_and_account_switch() {
    use whatsapp_tui::desktop::clipboard::Paste;
    for discard in ["none", "remove", "account"] {
        let dir = tempfile::tempdir().unwrap();
        let mut app = ready_app();
        press(&mut app, "enter");
        press(&mut app, "a");
        let Effect::PasteClipboard {
            request,
            account,
            chat,
            ..
        } = press(&mut app, "ctrl-v").remove(0)
        else {
            panic!()
        };
        assert!(press(&mut app, "ctrl-v").is_empty());
        assert!(
            !press(&mut app, "enter")
                .iter()
                .any(|e| matches!(e, Effect::Prepare { .. }))
        );
        press(&mut app, "b");
        match discard {
            "remove" => {
                press(&mut app, "alt-a");
            }
            "account" => {
                app.update(
                    Input::Backend(whatsapp_tui::whatsapp::BackendEvent::AccountKnown(
                        "other".into(),
                    )),
                    tokio::time::Instant::now(),
                );
            }
            _ => {}
        }
        app.update(
            Input::ClipboardRead {
                request,
                account,
                chat,
                result: Ok(Paste::Image(Box::new(image(dir.path())))),
            },
            tokio::time::Instant::now(),
        );
        assert_eq!(app.view().draft.attachment.is_some(), discard == "none");
        if discard != "account" {
            assert_eq!(app.view().draft.text, "ab");
        }
    }
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "x");
    press(&mut app, "home");
    let Effect::PasteClipboard {
        request,
        account,
        chat,
        ..
    } = press(&mut app, "ctrl-v").remove(0)
    else {
        panic!()
    };
    app.update(
        Input::ClipboardRead {
            request,
            account,
            chat,
            result: Ok(Paste::Text("hello 😀 ".into())),
        },
        tokio::time::Instant::now(),
    );
    assert_eq!(app.view().draft.text, "hello 😀 x");
    assert!(app.view().draft.attachment.is_none());
}

#[tokio::test]
async fn attachment_only_draft_stages_and_survives_restart_without_replay() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    let mut d = draft("", 3);
    d.attachment = Some(Box::new(image(dir.path())));
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
    assert_eq!(
        app.view().draft.attachment.as_ref(),
        Some(&Box::new(local.clone()))
    );
    assert!(press(&mut app, "enter").iter().any(|e| matches!(e, Effect::Prepare {draft,..} if draft.attachment.is_some() && draft.text.is_empty())));
    press(&mut app, "alt-a");
    assert!(app.view().draft.attachment.is_none());
}

#[tokio::test]
async fn staging_an_image_preserves_newer_caption_and_attachment_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut captured = draft("first caption", 4);
    captured.attachment = Some(Box::new(image(dir.path())));
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
    d.attachment = Some(Box::new(image(dir.path())));
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

fn attach(app: &mut App, local: outgoing::LocalImage) {
    press(app, "ctrl-o");
    app.update(
        Input::Terminal(crossterm::event::Event::Paste("image.png".into())),
        tokio::time::Instant::now(),
    );
    let effects = press(app, "enter");
    let (request, account, chat) = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::ImportImage {
                request,
                account,
                chat,
                ..
            } => Some((request, account, chat)),
            _ => None,
        })
        .unwrap();
    app.update(
        Input::ImageImported {
            request,
            account,
            chat,
            result: Ok(local),
        },
        tokio::time::Instant::now(),
    );
}
fn refresh(app: &mut App) -> Effect {
    app.update(
        Input::Backend(whatsapp_tui::whatsapp::BackendEvent::StoreChanged(
            StoreChange {
                account: "test".into(),
                chats: vec!["chat".into()],
            },
        )),
        tokio::time::Instant::now(),
    )
    .into_iter()
    .find(|e| matches!(e, Effect::LoadChat { .. }))
    .unwrap()
}
fn loaded(app: &mut App, effect: Effect, snapshot: ChatSnapshot) {
    let Effect::LoadChat {
        request,
        account,
        chat,
        cursor,
    } = effect
    else {
        panic!()
    };
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account,
            chat,
            cursor,
            result: Ok(Box::new(snapshot)),
        }),
        tokio::time::Instant::now(),
    );
}
fn second_image(dir: &std::path::Path) -> outgoing::LocalImage {
    let source = dir.join("second.png");
    image::RgbImage::from_pixel(20, 20, image::Rgb([220, 50, 30]))
        .save(&source)
        .unwrap();
    outgoing::import(&source, dir).unwrap()
}

#[tokio::test]
async fn conflicting_image_drafts_keep_their_captions_quotes_and_recover_after_send() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("drafts.sqlite3");
    let store = Store::open(db.clone()).await.unwrap();
    let mut alias = draft("caption A", 2);
    alias.attachment = Some(Box::new(image(dir.path())));
    alias.reply = Some(Quote {
        media_kind: None,
        key: key("alias", "alias", "quoted"),
        preview: "original".into(),
        availability: QuoteAvailability::Available,
    });
    let mut canonical = draft("caption B", 3);
    canonical.attachment = Some(Box::new(second_image(dir.path())));
    store
        .save_draft("test".into(), "alias".into(), alias.clone())
        .await
        .unwrap();
    store
        .save_draft("test".into(), "chat".into(), canonical.clone())
        .await
        .unwrap();
    store
        .merge_alias("test".into(), "alias".into(), "chat".into())
        .await
        .unwrap();
    let merged = store
        .snapshot("test".into(), "chat".into(), None)
        .await
        .unwrap()
        .draft;
    assert_eq!(merged.attachment, canonical.attachment);
    assert_eq!(merged.text, "caption B");
    assert_eq!(merged.recovered.len(), 1);
    assert_eq!(merged.recovered[0].attachment, alias.attachment);
    assert_eq!(merged.recovered[0].text, "caption A");
    assert_eq!(
        merged.recovered[0].reply.as_ref().unwrap().key.chat.0,
        "chat"
    );
    let mut composer = ready_app();
    let request = refresh(&mut composer);
    loaded(
        &mut composer,
        request,
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap(),
    );
    press(&mut composer, "enter");
    press(&mut composer, "ctrl-o");
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(90, 30)).unwrap();
    terminal
        .draw(|frame| whatsapp_tui::ui::render(frame, &composer.view(), &composer.config))
        .unwrap();
    let screen = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect::<String>();
    assert!(screen.contains("Saved drafts"));
    assert!(screen.contains("caption A"));
    press(&mut composer, "enter");
    assert_eq!(composer.view().draft.attachment, alias.attachment);
    assert_eq!(composer.view().draft.text, "caption A");
    assert_eq!(composer.view().draft.recovered[0].text, "caption B");
    press(&mut composer, "ctrl-o");
    press(&mut composer, "enter");
    assert_eq!(composer.view().draft.attachment, canonical.attachment);
    let effects = press(&mut composer, "enter");
    let (request, captured) = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::Prepare { request, draft, .. } => Some((request, draft)),
            _ => None,
        })
        .unwrap();
    let sent = outbound(key("chat", "test", "sent-B"), captured);
    composer.update(
        Input::Backend(whatsapp_tui::whatsapp::BackendEvent::Prepared {
            request,
            message: Box::new(sent.clone()),
        }),
        tokio::time::Instant::now(),
    );
    store.stage_outgoing(sent.clone()).await.unwrap();
    composer.update(
        Input::Store(StoreCompletion::Staged {
            request,
            message: Box::new(sent),
            result: Ok(()),
        }),
        tokio::time::Instant::now(),
    );
    assert!(composer.view().draft.attachment.is_none());
    assert_eq!(composer.view().draft.recovered.len(), 1);
    assert_eq!(composer.view().draft.recovered[0].text, "caption A");
    store
        .apply_batch(MessageBatch {
            account: "test".into(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Delete {
                key: key("chat", "chat", "quoted"),
            }],
        })
        .await
        .unwrap();
    store.flush().await.unwrap();
    drop(store);
    let store = Store::open(db).await.unwrap();
    let snap = store
        .snapshot("test".into(), "chat".into(), None)
        .await
        .unwrap();
    assert!(snap.summary.has_draft);
    assert!(snap.draft.attachment.is_none());
    assert!(snap.draft.text.is_empty());
    assert_eq!(snap.draft.recovered.len(), 1);
    assert_eq!(
        snap.draft.recovered[0].reply.as_ref().unwrap().availability,
        QuoteAvailability::Deleted
    );
    let mut app = ready_app();
    let request = refresh(&mut app);
    loaded(&mut app, request, snap);
    press(&mut app, "enter");
    press(&mut app, "ctrl-o");
    assert!(
        press(&mut app, "enter").is_empty(),
        "restoring does not send or import"
    );
    assert_eq!(app.view().draft.attachment, alias.attachment);
    assert_eq!(app.view().draft.text, "caption A");
    assert!(app.view().draft.recovered.is_empty());
    assert!(app.view().overlay.is_none());
    let pending = press(&mut app, "enter");
    assert!(pending.iter().any(|e| matches!(e, Effect::Prepare { draft, .. } if draft.text == "caption A" && draft.attachment == alias.attachment)));
}

#[tokio::test]
async fn alias_snapshot_respects_removal_and_replacement_after_capture() {
    for (replacement, conflict) in [(false, false), (true, false), (false, true), (true, true)] {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("drafts.sqlite3"))
            .await
            .unwrap();
        let mut app = ready_app();
        press(&mut app, "enter");
        attach(&mut app, image(dir.path()));
        store
            .save_draft("test".into(), "chat".into(), app.view().draft)
            .await
            .unwrap();
        let other = second_image(dir.path());
        if conflict {
            let mut independent = draft("independent caption", 3);
            independent.attachment = Some(Box::new(other.clone()));
            store
                .save_draft("test".into(), "canonical".into(), independent)
                .await
                .unwrap();
        }
        store
            .merge_alias("test".into(), "chat".into(), "canonical".into())
            .await
            .unwrap();
        let snap = store
            .snapshot("test".into(), "canonical".into(), None)
            .await
            .unwrap();
        let request = refresh(&mut app);
        if replacement {
            attach(&mut app, second_image(dir.path()));
        } else {
            press(&mut app, "alt-a");
        }
        let edited = app.view().draft.attachment;
        loaded(&mut app, request, snap);
        if conflict {
            assert_eq!(app.view().draft.attachment, Some(Box::new(other)));
            assert_eq!(app.view().draft.text, "independent caption");
            if replacement {
                assert_eq!(app.view().draft.recovered.len(), 1);
                assert_eq!(app.view().draft.recovered[0].attachment, edited);
            } else {
                assert!(app.view().draft.recovered.is_empty());
            }
        } else {
            assert_eq!(app.view().draft.attachment, edited);
            assert!(
                app.view().draft.recovered.is_empty(),
                "explicitly removed images must not be recovered"
            );
        }
        let current = app.view().draft;
        let saved = app.flush_drafts();
        assert!(saved.iter().any(|e| matches!(e, Effect::SaveDraft { chat, draft, .. } if chat.0 == "canonical" && draft == &current)));
    }
}

#[tokio::test]
async fn alias_resolution_closes_an_import_for_the_old_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("drafts.sqlite3"))
        .await
        .unwrap();
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "ctrl-o");
    app.update(
        Input::Terminal(crossterm::event::Event::Paste("old.png".into())),
        tokio::time::Instant::now(),
    );
    let pending = press(&mut app, "enter");
    store
        .merge_alias("test".into(), "chat".into(), "canonical".into())
        .await
        .unwrap();
    let request = refresh(&mut app);
    loaded(
        &mut app,
        request,
        store
            .snapshot("test".into(), "canonical".into(), None)
            .await
            .unwrap(),
    );
    assert!(app.view().overlay.is_none());
    let Effect::ImportImage {
        request,
        account,
        chat,
        ..
    } = pending
        .into_iter()
        .find(|e| matches!(e, Effect::ImportImage { .. }))
        .unwrap()
    else {
        panic!()
    };
    app.update(
        Input::ImageImported {
            request,
            account,
            chat,
            result: Ok(image(dir.path())),
        },
        tokio::time::Instant::now(),
    );
    assert!(app.view().draft.attachment.is_none());
    press(&mut app, "ctrl-o");
    assert!(matches!(
        app.view().overlay,
        Some(Overlay::Attachment {
            importing: None,
            ..
        })
    ));
}

#[tokio::test]
async fn alias_merge_preserves_recovery_while_canonical_composer_has_unsaved_edits() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("drafts.sqlite3"))
        .await
        .unwrap();
    let mut canonical = draft("caption B", 3);
    canonical.attachment = Some(Box::new(second_image(dir.path())));
    store
        .save_draft("test".into(), "chat".into(), canonical)
        .await
        .unwrap();
    let mut app = ready_app();
    let request = refresh(&mut app);
    loaded(
        &mut app,
        request,
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap(),
    );
    press(&mut app, "enter");
    for _ in 0..8 {
        press(&mut app, "x");
    }
    let mut alias = draft("caption A", 2);
    alias.attachment = Some(Box::new(image(dir.path())));
    store
        .save_draft("test".into(), "alias".into(), alias.clone())
        .await
        .unwrap();
    store
        .merge_alias("test".into(), "alias".into(), "chat".into())
        .await
        .unwrap();
    let request = refresh(&mut app);
    loaded(
        &mut app,
        request,
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap(),
    );
    assert_eq!(app.view().draft.text, "caption Bxxxxxxxx");
    assert_eq!(app.view().draft.recovered.len(), 1);
    assert_eq!(app.view().draft.recovered[0].attachment, alias.attachment);
    let effects = app.flush_drafts();
    let saved = effects
        .into_iter()
        .find_map(|e| match e {
            Effect::SaveDraft { draft, .. } => Some(draft),
            _ => None,
        })
        .unwrap();
    store
        .save_draft("test".into(), "chat".into(), saved)
        .await
        .unwrap();
    assert_eq!(
        store
            .snapshot("test".into(), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .recovered
            .len(),
        1
    );
}
