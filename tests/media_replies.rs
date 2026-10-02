mod support;
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    media::{Attachment, AttachmentKind},
    message_actions,
    whatsapp::{BackendEvent, encode},
};
fn media(kind: AttachmentKind) -> MessageRecord {
    let mut m = message(
        MessageKey {
            account: "111@s.whatsapp.net".into(),
            chat: "123@g.us".into(),
            sender: "222@s.whatsapp.net".into(),
            id: "original".into(),
            from_me: false,
        },
        "",
    );
    m.body = MessageBody::Media(Box::new(Attachment {
        audio: None,
        kind,
        filename: (kind == AttachmentKind::Document).then(|| "notes.pdf".into()),
        caption: None,
        mime: Some(
            if kind == AttachmentKind::Sticker {
                "image/webp"
            } else {
                "image/jpeg"
            }
            .into(),
        ),
        size: 64,
        direct_path: "/v/media".into(),
        media_key: [1; 32],
        sha256: [2; 32],
        encrypted_sha256: [3; 32],
    }));
    m
}
#[test]
fn captionless_media_quotes_have_a_label_and_typed_wire_body() {
    for (kind, label) in [
        (AttachmentKind::Image, "[image]"),
        (AttachmentKind::Sticker, "[sticker]"),
        (AttachmentKind::Document, "[document] notes.pdf"),
        (AttachmentKind::Video, "[video]"),
        (AttachmentKind::Gif, "[gif]"),
    ] {
        let m = media(kind);
        let quote = message_actions::quote(&m, 1).unwrap();
        assert_eq!(quote.preview, label);
        assert_eq!(quote.media_kind, Some(kind));
        let mut key = m.key.clone();
        key.from_me = true;
        key.sender = key.account.0.clone().into();
        key.id = "reply".into();
        let outbound = OutboundText {
            key,
            draft: Draft {
                text: "a reply".into(),
                reply: Some(quote),
                ..Default::default()
            },
            created_at_ms: 1,
        };
        let encoded = encode::encode_text(&outbound).unwrap();
        let ctx = encoded
            .message
            .extended_text_message
            .as_option()
            .unwrap()
            .context_info
            .as_option()
            .unwrap();
        assert_eq!(ctx.stanza_id.as_deref(), Some("original"));
        assert_eq!(ctx.participant.as_deref(), Some("222@s.whatsapp.net"));
        let quoted = ctx.quoted_message.as_option().unwrap();
        assert_eq!(quoted.image_message.is_set(), kind == AttachmentKind::Image);
        assert_eq!(
            quoted.video_message.is_set(),
            matches!(kind, AttachmentKind::Video | AttachmentKind::Gif)
        );
        if kind == AttachmentKind::Gif {
            assert_eq!(
                quoted.video_message.as_option().unwrap().gif_playback,
                Some(true)
            );
        }
        assert_eq!(
            quoted.sticker_message.is_set(),
            kind == AttachmentKind::Sticker
        );
        assert_eq!(
            quoted.document_message.is_set(),
            kind == AttachmentKind::Document
        );
    }
}
#[test]
fn quotes_are_bounded_and_old_drafts_without_media_kind_still_load() {
    use unicode_segmentation::UnicodeSegmentation;
    let mut m = media(AttachmentKind::Image);
    if let MessageBody::Media(a) = &mut m.body {
        a.caption = Some("👩‍💻".repeat(200));
    }
    let q = message_actions::quote(&m, 1).unwrap();
    assert_eq!(q.preview.graphemes(true).count(), 160);
    let mut value = serde_json::to_value(q).unwrap();
    value.as_object_mut().unwrap().remove("media_kind");
    let old: Quote = serde_json::from_value(value).unwrap();
    assert!(old.media_kind.is_none());
    m.expires_at_ms = Some(10);
    assert!(message_actions::quote(&m, 10).is_none());
    m.expires_at_ms = None;
    m.body = MessageBody::Deleted;
    assert!(message_actions::quote(&m, 1).is_none());
}
fn with_reply() -> App {
    let mut app = ready_app();
    let mut current = app.view().messages[0].clone();
    let original = message(key("chat", "alice", "original"), "older original");
    current.quote = message_actions::quote(&original, chrono::Utc::now().timestamp_millis());
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary: app.view().chats[0].clone(),
                messages: vec![current],
                draft: Default::default(),
                receipts: vec![],
                interactions: Default::default(),
                has_older: true,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
    press(&mut app, "tab");
    app
}
#[test]
fn jumping_to_a_quote_uses_the_originals_key_and_cached_page_cursor() {
    let mut app = with_reply();
    let effects = press(&mut app, "q");
    let (request, key) = effects
        .into_iter()
        .find_map(|e| {
            if let Effect::LoadOriginal { request, key } = e {
                Some((request, key))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(key.id.0, "original");
    let mut original = message(key.clone(), "older original");
    original.created_at_ms -= 10_000;
    let effects = app.update(
        Input::Store(StoreCompletion::Original {
            request,
            key: key.clone(),
            result: Ok(Some(Box::new(original.clone()))),
        }),
        Instant::now(),
    );
    assert!(effects.iter().any(|e|matches!(e,Effect::LoadChat{cursor:Some(c),..}if c.key==key && c.created_at_ms==original.created_at_ms && c.direction==PageDirection::AtOrBefore)));
    assert_eq!(app.view().selected_message, Some(key));
    assert!(!app.view().at_bottom);
}
#[test]
fn missing_expired_and_wrong_scope_originals_do_not_navigate() {
    for mode in 0..4 {
        let mut app = with_reply();
        let (request, key) = press(&mut app, "q")
            .into_iter()
            .find_map(|e| {
                if let Effect::LoadOriginal { request, key } = e {
                    Some((request, key))
                } else {
                    None
                }
            })
            .unwrap();
        let mut original = message(key.clone(), "original");
        match mode {
            1 => original.expires_at_ms = Some(1),
            2 => original.key.chat = "elsewhere".into(),
            3 => original.key.account = "elsewhere".into(),
            _ => {}
        }
        let effects = app.update(
            Input::Store(StoreCompletion::Original {
                request,
                key,
                result: Ok((mode != 0).then(|| Box::new(original))),
            }),
            Instant::now(),
        );
        assert!(!effects.iter().any(|e| matches!(e, Effect::LoadChat { .. })));
        assert!(app.view().notice.is_some());
    }
}

#[test]
fn new_arrival_during_quote_lookup_does_not_steal_the_requested_jump() {
    let mut app = with_reply();
    let (request, key) = press(&mut app, "q")
        .into_iter()
        .find_map(|e| {
            if let Effect::LoadOriginal { request, key } = e {
                Some((request, key))
            } else {
                None
            }
        })
        .unwrap();
    let source = app.view().messages[0].clone();
    let mut arrival = message(support::key("chat", "alice", "arrival"), "new");
    arrival.created_at_ms += 1000;
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let reload = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request: reload,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary: app.view().chats[0].clone(),
                messages: vec![source, arrival],
                draft: Default::default(),
                receipts: vec![],
                interactions: Default::default(),
                has_older: true,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
    let original = message(key.clone(), "original");
    let effects = app.update(
        Input::Store(StoreCompletion::Original {
            request,
            key: key.clone(),
            result: Ok(Some(Box::new(original))),
        }),
        Instant::now(),
    );
    assert!(
        effects
            .iter()
            .any(|e| matches!(e,Effect::LoadChat{cursor:Some(c),..} if c.key==key))
    );
}

#[test]
fn quoted_lines_have_click_targets_after_wrapping_and_clipping() {
    use ratatui::{Terminal, backend::TestBackend};
    use whatsapp_tui::ui::{self, interaction::Target};
    for (w, h) in [(40, 13), (120, 32)] {
        let app = with_reply();
        let mut v = app.view();
        v.messages[0].quote.as_mut().unwrap().preview = "quoted line ".repeat(30);
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut map = None;
        t.draw(|f| {
            map = Some(ui::render_interactive(
                f,
                &v,
                &app.config,
                &mut ui::Images::default(),
                &mut ui::Avatars::default(),
            ))
        })
        .unwrap();
        let map = map.unwrap();
        let timeline = ui::layout::calculate(map.area, v.focus).messages;
        let hits = (timeline.y..timeline.bottom())
            .flat_map(|y| (timeline.x..timeline.right()).map(move |x| (x, y)))
            .filter(
                |&(x, y)| matches!(map.hit(x,y),Some(Target::Quote(k)) if k==&v.messages[0].key),
            )
            .collect::<Vec<_>>();
        assert!(!hits.is_empty(), "{w}x{h}");
        for (x, y) in hits {
            assert!(
                y > timeline.y
                    && y < timeline.bottom() - 1
                    && x > timeline.x
                    && x < timeline.right() - 1
            );
        }
    }
}

#[test]
fn navigation_after_request_cancels_a_delayed_quote_jump() {
    let mut app = with_reply();
    let (request, key) = press(&mut app, "q")
        .into_iter()
        .find_map(|e| {
            if let Effect::LoadOriginal { request, key } = e {
                Some((request, key))
            } else {
                None
            }
        })
        .unwrap();
    press(&mut app, "tab");
    let original = message(key.clone(), "original");
    let effects = app.update(
        Input::Store(StoreCompletion::Original {
            request,
            key,
            result: Ok(Some(Box::new(original))),
        }),
        Instant::now(),
    );
    assert!(!effects.iter().any(|e| matches!(e, Effect::LoadChat { .. })));
    assert_eq!(app.view().focus, Focus::Composer);
}
