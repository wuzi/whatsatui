mod support;
use crossterm::event::Event;
use support::*;
use tokio::{sync::mpsc, time::Instant};
use whatsapp_tui::{app::model::*, app::*, runtime, storage::Store, whatsapp::BackendEvent};
async fn fresh() -> (tempfile::TempDir, Store) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    (d, s)
}
async fn changes(s: &Store, source: MessageSource, changes: Vec<MessageChange>) {
    s.apply_batch(MessageBatch {
        account: account("test"),
        source,
        changes,
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn history_cannot_resurrect_deleted_text() {
    for deletion_first in [false, true] {
        let (_d, s) = fresh().await;
        let k = key("chat", "alice", "one");
        let m = message(k.clone(), "old secret");
        if !deletion_first {
            s.apply_batch(batch(vec![m.clone()])).await.unwrap();
        }
        changes(
            &s,
            MessageSource::Live,
            vec![MessageChange::Delete { key: k.clone() }],
        )
        .await;
        for _ in 0..2 {
            changes(
                &s,
                MessageSource::History,
                vec![MessageChange::Upsert(m.clone())],
            )
            .await;
        }
        let snap = s
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap();
        assert_eq!(snap.messages.len(), 1);
        assert_eq!(snap.messages[0].body, MessageBody::Deleted);
        assert!(!snap.summary.preview.contains("old secret"));
    }
}
#[tokio::test]
async fn expiry_removes_all_previews() {
    let (_d, s) = fresh().await;
    let k = key("chat", "alice", "one");
    let mut m = message(k.clone(), "expired secret");
    m.expires_at_ms = Some(2000);
    let quote = Quote {
        key: k,
        preview: "expired secret".into(),
        availability: QuoteAvailability::Available,
    };
    let mut reply = message(key("other", "bob", "two"), "a reply");
    reply.quote = Some(quote.clone());
    s.apply_batch(batch(vec![m.clone(), reply])).await.unwrap();
    let mut d = draft("draft text", 1);
    d.reply = Some(quote);
    s.save_draft(account("test"), "chat".into(), d.clone())
        .await
        .unwrap();
    s.expire(account("test"), 2000).await.unwrap();
    s.expire(account("test"), 2000).await.unwrap();
    d.revision = 2;
    s.save_draft(account("test"), "chat".into(), d)
        .await
        .unwrap();
    changes(&s, MessageSource::History, vec![MessageChange::Upsert(m)]).await;
    for chat in ["chat", "other"] {
        let snap = s
            .snapshot(account("test"), chat.into(), None)
            .await
            .unwrap();
        let cached = serde_json::to_string(&snap.messages).unwrap()
            + &serde_json::to_string(&snap.draft).unwrap()
            + &snap.summary.preview;
        assert!(!cached.contains("expired secret"));
    }
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .reply
            .unwrap()
            .availability,
        QuoteAvailability::Expired
    );
}
#[tokio::test]
async fn receipt_before_message_is_retained() {
    let (_d, s) = fresh().await;
    let k = key("chat", "test", "one");
    s.record_receipt(Receipt {
        key: k.clone(),
        recipient: "alice".into(),
        state: ReceiptState::Read,
        at_ms: 10,
    })
    .await
    .unwrap();
    let mut m = message(k.clone(), "hello");
    m.send_state = Some(SendState::Sent);
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    s.record_receipt(Receipt {
        key: k.clone(),
        recipient: "alice".into(),
        state: ReceiptState::Delivered,
        at_ms: 9,
    })
    .await
    .unwrap();
    s.apply_batch(batch(vec![m])).await.unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages[0]
            .send_state,
        Some(SendState::Read)
    );
}
#[tokio::test]
async fn sender_collision_stays_distinct() {
    let (_d, s) = fresh().await;
    s.apply_batch(batch(vec![
        message(key("g@g.us", "a", "same"), "first"),
        message(key("g@g.us", "b", "same"), "second"),
    ]))
    .await
    .unwrap();
    changes(
        &s,
        MessageSource::Live,
        vec![MessageChange::Delete {
            key: key("g@g.us", "a", "same"),
        }],
    )
    .await;
    let snap = s
        .snapshot(account("test"), "g@g.us".into(), None)
        .await
        .unwrap();
    assert_eq!(snap.messages.len(), 2);
    assert_eq!(
        snap.messages
            .iter()
            .find(|m| m.key.sender.0 == "b")
            .unwrap()
            .body,
        MessageBody::Text("second".into())
    );
}
#[tokio::test]
async fn alias_merge_is_idempotent() {
    let (_d, s) = fresh().await;
    let alias = "123@lid";
    let canonical = "55110001@s.whatsapp.net";
    s.apply_batch(batch(vec![
        message(key(alias, alias, "same"), "hello"),
        message(key(canonical, canonical, "same"), "hello"),
        message(key("g@g.us", alias, "group-id"), "group one"),
        message(key("g@g.us", "other@lid", "group-id"), "group two"),
    ]))
    .await
    .unwrap();
    s.save_draft(account("test"), alias.into(), draft("unfinished", 3))
        .await
        .unwrap();
    for _ in 0..2 {
        s.merge_alias(account("test"), alias.into(), canonical.into())
            .await
            .unwrap();
    }
    s.apply_batch(batch(vec![message(key(alias, alias, "same"), "hello")]))
        .await
        .unwrap();
    let chats = s.list_chats(account("test")).await.unwrap();
    assert!(!chats.iter().any(|c| c.chat.0 == alias));
    assert_eq!(chats.len(), 2);
    let snap = s
        .snapshot(account("test"), canonical.into(), None)
        .await
        .unwrap();
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.draft.text, "unfinished");
    let group = s
        .snapshot(account("test"), "g@g.us".into(), None)
        .await
        .unwrap();
    assert_eq!(group.messages.len(), 2);
    assert!(group.messages.iter().any(|m| m.key.sender.0 == canonical));
}
#[tokio::test]
async fn history_does_not_increment_unread() {
    let (_d, s) = fresh().await;
    let m = message(key("chat", "alice", "one"), "one");
    for _ in 0..2 {
        changes(
            &s,
            MessageSource::History,
            vec![MessageChange::Upsert(m.clone())],
        )
        .await;
    }
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .summary
            .unread,
        0
    );
    let next = message(key("chat", "alice", "two"), "two");
    s.apply_batch(batch(vec![next.clone()])).await.unwrap();
    s.apply_batch(batch(vec![next.clone()])).await.unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .summary
            .unread,
        1
    );
    s.mark_read(account("test"), "chat".into(), vec![next.key])
        .await
        .unwrap();
    s.apply_batch(batch(vec![m])).await.unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .summary
            .unread,
        0
    );
}
#[tokio::test]
async fn one_group_receipt_is_not_everyone_read() {
    let (_d, s) = fresh().await;
    let k = key("g@g.us", "test", "one");
    s.stage_outgoing(outbound(k.clone(), draft("group", 1)))
        .await
        .unwrap();
    for _ in 0..2 {
        s.record_receipt(Receipt {
            key: k.clone(),
            recipient: "alice".into(),
            state: ReceiptState::Read,
            at_ms: 1,
        })
        .await
        .unwrap();
    }
    let snap = s
        .snapshot(account("test"), "g@g.us".into(), None)
        .await
        .unwrap();
    assert_eq!(snap.receipts.len(), 1);
    assert_eq!(snap.messages[0].send_state, Some(SendState::Sent));
}
#[tokio::test]
async fn reading_position_survives_new_messages() {
    let (_d, s) = fresh().await;
    let initial = (0..100)
        .map(|n| message(key("chat", "alice", &format!("{n:04}")), "old"))
        .collect();
    s.apply_batch(batch(initial)).await.unwrap();
    let mut app = ready_app();
    let (tx, _rx) = mpsc::channel(32);
    async fn reload(
        app: &mut App,
        s: &Store,
        tx: &mpsc::Sender<whatsapp_tui::whatsapp::BackendCommand>,
    ) {
        let next = app.update(
            Input::Backend(BackendEvent::StoreChanged(StoreChange {
                account: account("test"),
                chats: vec!["chat".into()],
            })),
            Instant::now(),
        );
        for e in next {
            if let Some(input) = runtime::execute(e, s.clone(), tx.clone()).await {
                app.update(input, Instant::now());
            }
        }
    }
    reload(&mut app, &s, &tx).await;
    press(&mut app, "tab");
    press(&mut app, "up");
    let anchor = app.view().selected_message.unwrap();
    s.apply_batch(batch(
        (100..250)
            .map(|n| message(key("chat", "alice", &format!("{n:04}")), "new"))
            .collect(),
    ))
    .await
    .unwrap();
    reload(&mut app, &s, &tx).await;
    assert_eq!(app.view().selected_message, Some(anchor));
    assert!(app.view().messages.len() <= 100);
}
#[tokio::test]
async fn edits_arriving_before_content_are_ordered() {
    let (_d, s) = fresh().await;
    let k = key("chat", "alice", "one");
    for (text, at) in [("new", 30), ("old", 20)] {
        changes(
            &s,
            MessageSource::Live,
            vec![MessageChange::Edit {
                key: k.clone(),
                text: text.into(),
                edited_at_ms: at,
            }],
        )
        .await;
    }
    s.apply_batch(batch(vec![message(k, "original")]))
        .await
        .unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages[0]
            .body,
        MessageBody::Text("new".into())
    );
}
#[test]
fn read_requires_visible_bottom_and_foreground() {
    let mut app = ready_app();
    let now = Instant::now();
    assert!(
        !app.update(Input::Tick(0), now)
            .iter()
            .any(|e| matches!(e, Effect::MarkRead { .. }))
    );
    app.update(Input::Terminal(Event::FocusLost), now);
    assert!(
        !press(&mut app, "tab")
            .iter()
            .any(|e| matches!(e, Effect::MarkRead { .. }))
    );
    let effects = app.update(Input::Terminal(Event::FocusGained), now);
    assert!(effects.iter().any(|e| matches!(e, Effect::MarkRead { .. })));
}
#[tokio::test]
async fn read_watermark_blocks_old_unique_replays() {
    let (_d, s) = fresh().await;
    let newest = message(key("chat", "alice", "new"), "new");
    s.apply_batch(batch(vec![newest.clone()])).await.unwrap();
    s.mark_read(account("test"), "chat".into(), vec![newest.key])
        .await
        .unwrap();
    let mut old = message(key("chat", "alice", "old"), "old offline replay");
    old.created_at_ms -= 10_000;
    s.apply_batch(batch(vec![old])).await.unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .summary
            .unread,
        0
    );
}

#[tokio::test]
async fn runtime_persists_read_and_expiry_before_notifying_ui() {
    let (_d, s) = fresh().await;
    let mut m = message(key("chat", "alice", "one"), "secret");
    m.expires_at_ms = Some(2000);
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let (tx, mut rx) = mpsc::channel(4);
    runtime::execute(
        Effect::MarkRead {
            account: account("test"),
            chat: "chat".into(),
            keys: vec![m.key],
        },
        s.clone(),
        tx.clone(),
    )
    .await;
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .summary
            .unread,
        0
    );
    assert!(matches!(
        rx.try_recv(),
        Ok(whatsapp_tui::whatsapp::BackendCommand::MarkRead(_))
    ));
    runtime::execute(
        Effect::Expire {
            account: account("test"),
            now_ms: 2000,
        },
        s.clone(),
        tx,
    )
    .await;
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages[0]
            .body,
        MessageBody::Expired
    );
}

#[tokio::test]
async fn transmit_refreshes_a_quote_expired_after_staging() {
    let (_d, s) = fresh().await;
    let mut original = message(key("chat", "alice", "old"), "secret");
    original.expires_at_ms = Some(2000);
    s.apply_batch(batch(vec![original.clone()])).await.unwrap();
    let mut d = draft("reply", 1);
    d.reply = Some(Quote {
        key: original.key,
        preview: "secret".into(),
        availability: QuoteAvailability::Available,
    });
    let out = outbound(key("chat", "test", "new"), d);
    s.stage_outgoing(out.clone()).await.unwrap();
    s.expire(account("test"), 2000).await.unwrap();
    let (tx, mut rx) = mpsc::channel(4);
    runtime::execute(Effect::Transmit(out), s, tx).await;
    let whatsapp_tui::whatsapp::BackendCommand::Transmit(out) = rx.try_recv().unwrap() else {
        panic!("transmit")
    };
    assert_eq!(
        out.draft.reply.unwrap().availability,
        QuoteAvailability::Expired
    );
}

#[tokio::test]
async fn initial_unread_baseline_is_not_replayed_after_reading() {
    let (_d, s) = fresh().await;
    let summary = ChatSummary {
        account: account("test"),
        chat: "chat".into(),
        name: "Alice".into(),
        unread: 3,
        latest_at_ms: 100,
        ..Default::default()
    };
    s.upsert_chats(account("test"), vec![summary.clone()])
        .await
        .unwrap();
    let mut m = message(key("chat", "alice", "new"), "new");
    m.created_at_ms = 200;
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    assert_eq!(s.list_chats(account("test")).await.unwrap()[0].unread, 4);
    s.mark_read(account("test"), "chat".into(), vec![m.key.clone()])
        .await
        .unwrap();
    s.upsert_chats(account("test"), vec![summary])
        .await
        .unwrap();
    changes(&s, MessageSource::History, vec![MessageChange::Upsert(m)]).await;
    assert_eq!(s.list_chats(account("test")).await.unwrap()[0].unread, 0);
}

#[tokio::test]
async fn expired_reply_is_removed_from_a_dirty_visible_draft() {
    let (_d, s) = fresh().await;
    let mut m = message(key("chat", "alice", "one"), "Hello 👋");
    m.expires_at_ms = Some(2000);
    s.apply_batch(batch(vec![m])).await.unwrap();
    let mut app = ready_app();
    press(&mut app, "tab");
    press(&mut app, "r");
    press(&mut app, "x");
    assert!(app.view().draft.reply.is_some());
    let change = s.expire(account("test"), 2000).await.unwrap();
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(change)),
        Instant::now(),
    );
    let (tx, _rx) = mpsc::channel(8);
    for effect in effects {
        if let Some(input) = runtime::execute(effect, s.clone(), tx.clone()).await {
            app.update(input, Instant::now());
        }
    }
    assert_eq!(app.view().draft.text, "x");
    assert_eq!(
        app.view().draft.reply.unwrap().availability,
        QuoteAvailability::Expired
    );
}

#[tokio::test]
async fn alias_merge_updates_the_open_composer_without_losing_edits() {
    let (_d, s) = fresh().await;
    s.apply_batch(batch(vec![message(key("chat", "alice", "one"), "hello")]))
        .await
        .unwrap();
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "x");
    let change = s
        .merge_alias(account("test"), "chat".into(), "canonical".into())
        .await
        .unwrap();
    let (tx, _rx) = mpsc::channel(8);
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(change)),
        Instant::now(),
    );
    for effect in effects {
        if let Some(input) = runtime::execute(effect, s.clone(), tx.clone()).await {
            app.update(input, Instant::now());
        }
    }
    assert_eq!(app.view().chat, Some("canonical".into()));
    assert_eq!(app.view().draft.text, "x");
    for effect in app.flush_drafts() {
        runtime::execute(effect, s.clone(), tx.clone()).await;
    }
    assert_eq!(
        s.snapshot(account("test"), "canonical".into(), None)
            .await
            .unwrap()
            .draft
            .text,
        "x"
    );
}

#[tokio::test]
async fn paging_forward_does_not_skip_to_the_last_message() {
    let (_d, s) = fresh().await;
    s.apply_batch(batch(
        (0..300)
            .map(|n| message(key("chat", "alice", &format!("{n:04}")), "text"))
            .collect(),
    ))
    .await
    .unwrap();
    let mut app = ready_app();
    let (tx, _rx) = mpsc::channel(16);
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    for e in effects {
        if let Some(i) = runtime::execute(e, s.clone(), tx.clone()).await {
            app.update(i, Instant::now());
        }
    }
    press(&mut app, "tab");
    for _ in 0..60 {
        let metrics = whatsapp_tui::ui::timeline_viewport(
            ratatui::layout::Rect::new(0, 0, 80, 24),
            &app.view(),
            &app.config,
        )
        .unwrap();
        app.update(Input::TimelineViewport(metrics), Instant::now());
        if app.view().messages.first().unwrap().key.id.0 == "0100" {
            break;
        }
        for e in press(&mut app, "pageup") {
            if let Some(i) = runtime::execute(e, s.clone(), tx.clone()).await {
                app.update(i, Instant::now());
            }
        }
    }
    assert_eq!(app.view().messages.first().unwrap().key.id.0, "0100");
    // The older page opens at its newest item. Forward paging must open at the next item.
    let effects = press(&mut app, "pagedown");
    for e in effects {
        if let Some(i) = runtime::execute(e, s.clone(), tx.clone()).await {
            app.update(i, Instant::now());
        }
    }
    assert_eq!(app.view().selected_message.unwrap().id.0, "0200");
}

#[tokio::test]
async fn contact_names_outrank_push_names() {
    let (_d, s) = fresh().await;
    for (name, priority) in [("Saved contact", 3), ("Push name", 1)] {
        let mut value = serde_json::to_value(ChatSummary {
            account: account("test"),
            chat: "chat".into(),
            name: name.into(),
            ..Default::default()
        })
        .unwrap();
        value["name_priority"] = priority.into();
        s.upsert_chats(
            account("test"),
            vec![serde_json::from_value(value).unwrap()],
        )
        .await
        .unwrap();
    }
    assert_eq!(
        s.list_chats(account("test")).await.unwrap()[0].name,
        "Saved contact"
    );
}

#[tokio::test]
async fn empty_visible_page_cannot_mark_a_later_arrival_read() {
    let (_d, s) = fresh().await;
    // A read effect captured an empty page; this arrival committed before that effect ran.
    s.apply_batch(batch(vec![message(
        key("chat", "alice", "new"),
        "not displayed yet",
    )]))
    .await
    .unwrap();
    s.mark_read(account("test"), "chat".into(), vec![])
        .await
        .unwrap();
    assert_eq!(s.list_chats(account("test")).await.unwrap()[0].unread, 1);
}
