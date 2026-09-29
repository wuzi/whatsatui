mod support;
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{app::model::*, app::*, config::Config, storage::Store, whatsapp::BackendEvent};
fn snapshot(chat: &str, text: &str, revision: u64) -> Box<ChatSnapshot> {
    Box::new(ChatSnapshot {
        summary: ChatSummary {
            account: account("test"),
            chat: chat.into(),
            name: chat.into(),
            ..Default::default()
        },
        messages: vec![],
        draft: draft(text, revision),
        receipts: vec![],
        has_older: false,
        has_newer: false,
    })
}
fn completion(
    app: &mut App,
    effect: &Effect,
    snap: Result<Box<ChatSnapshot>, String>,
) -> Vec<Effect> {
    let Effect::LoadChat {
        request,
        account,
        chat,
        cursor,
    } = effect
    else {
        panic!("load expected")
    };
    app.update(
        Input::Store(StoreCompletion::Chat {
            request: *request,
            account: account.clone(),
            chat: chat.clone(),
            cursor: cursor.clone(),
            result: snap,
        }),
        Instant::now(),
    )
}

#[tokio::test]
async fn missing_ack_times_out_and_late_ack_or_rejection_reconciles() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    for id in ["missing", "positive", "negative", "reordered"] {
        s.stage_resend(outbound(key("chat", "test", id), draft("outgoing", 1)))
            .await
            .unwrap();
    }
    s.set_send_state(key("chat", "test", "positive"), SendState::Sent)
        .await
        .unwrap();
    s.set_send_state(key("chat", "test", "negative"), SendState::Failed)
        .await
        .unwrap();
    s.set_send_state(key("chat", "test", "negative"), SendState::Sending)
        .await
        .unwrap();
    s.set_send_state(key("chat", "test", "negative"), SendState::Unconfirmed)
        .await
        .unwrap();
    s.set_send_state(key("chat", "test", "reordered"), SendState::Sent)
        .await
        .unwrap();
    s.set_send_state(key("chat", "test", "reordered"), SendState::Sending)
        .await
        .unwrap();
    s.expire(account("test"), 1_790_640_030_001).await.unwrap();
    let snap = s
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    let state = |id: &str| {
        snap.messages
            .iter()
            .find(|m| m.key.id.0 == id)
            .unwrap()
            .send_state
    };
    assert_eq!(state("missing"), Some(SendState::Unconfirmed));
    assert_eq!(state("positive"), Some(SendState::Sent));
    assert_eq!(state("negative"), Some(SendState::Failed));
    assert_eq!(state("reordered"), Some(SendState::Sent));
    s.record_receipt(Receipt {
        key: key("chat", "test", "missing"),
        recipient: "alice".into(),
        state: ReceiptState::Read,
        at_ms: 1_790_640_040_000,
    })
    .await
    .unwrap();
    s.set_send_state(key("chat", "test", "missing"), SendState::Unconfirmed)
        .await
        .unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages
            .iter()
            .find(|m| m.key.id.0 == "missing")
            .unwrap()
            .send_state,
        Some(SendState::Read)
    );
}

#[test]
fn alias_snapshot_preserves_both_drafts_and_survives_an_old_save() {
    let mut app = ready_app();
    press(&mut app, "enter");
    press(&mut app, "a");
    let old_save = app.flush_drafts().pop().unwrap();
    let fx = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let load = fx
        .iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    completion(
        &mut app,
        load,
        Ok(snapshot("canonical", "canonical saved text\n\na", 2)),
    );
    assert_eq!(app.view().draft.text, "canonical saved text\n\na");
    let Effect::SaveDraft {
        request,
        account,
        chat,
        draft,
    } = old_save
    else {
        panic!()
    };
    app.update(
        Input::Store(StoreCompletion::DraftSaved {
            request,
            account,
            chat,
            revision: draft.revision,
            result: Ok(()),
        }),
        Instant::now(),
    );
    let saves = app.flush_drafts();
    let (chat, saved) = saves
        .iter()
        .find_map(|e| {
            if let Effect::SaveDraft { chat, draft, .. } = e {
                Some((chat, draft))
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(chat.0, "canonical");
    assert_eq!(saved.text, "canonical saved text\n\na");
    assert!(saved.revision > 2);
}

#[test]
fn alias_merge_preserves_both_locally_edited_composers() {
    let mut app = ready_app();
    let fx = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec![],
        })),
        Instant::now(),
    );
    assert!(fx.is_empty());
    let fx = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["canonical".into()],
        })),
        Instant::now(),
    );
    let request = fx
        .iter()
        .find_map(|e| {
            if let Effect::LoadChats { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(vec![
                snapshot("chat", "", 0).summary,
                snapshot("canonical", "", 0).summary,
            ]),
        }),
        Instant::now(),
    );
    let load = press(&mut app, "j")
        .into_iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    completion(&mut app, &load, Ok(snapshot("canonical", "canonical", 4)));
    press(&mut app, "enter");
    press(&mut app, "b");
    press(&mut app, "esc");
    press(&mut app, "esc");
    let load = press(&mut app, "k")
        .into_iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    press(&mut app, "enter");
    press(&mut app, "a");
    completion(
        &mut app,
        &load,
        Ok(snapshot("canonical", "canonical\n\na", 6)),
    );
    assert!(app.view().draft.text.contains("canonicalb"));
    assert!(app.view().draft.text.contains("\n\na"));
    assert!(app.view().notice.unwrap().contains("review"));
    let saves = app.flush_drafts();
    assert_eq!(saves.len(), 1);
    let Effect::SaveDraft { chat, draft, .. } = &saves[0] else {
        panic!()
    };
    assert_eq!(chat.0, "canonical");
    assert!(draft.text.contains("canonicalb"));
    assert!(draft.revision > 6);
}

#[test]
fn overlapping_failed_load_preserves_buffered_edits() {
    let mut app = App::new(Config::default());
    let fx = app.update(
        Input::Backend(BackendEvent::AccountKnown(account("test"))),
        Instant::now(),
    );
    let request = fx
        .iter()
        .find_map(|e| {
            if let Effect::LoadChats { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    let fx = app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(vec![
                snapshot("chat", "", 0).summary,
                snapshot("other", "", 0).summary,
            ]),
        }),
        Instant::now(),
    );
    let first = fx
        .into_iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    press(&mut app, "enter");
    press(&mut app, "x");
    press(&mut app, "esc");
    press(&mut app, "esc");
    press(&mut app, "j");
    let second = press(&mut app, "k")
        .into_iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    completion(&mut app, &first, Err("transient read failure".into()));
    completion(&mut app, &second, Ok(snapshot("chat", "old", 1)));
    assert_eq!(app.view().draft.text, "oldx");
    let effects = app.request_shutdown();
    let (request, account, chat, revision) = effects
        .into_iter()
        .find_map(|e| {
            if let Effect::SaveDraft {
                request,
                account,
                chat,
                draft,
            } = e
            {
                Some((request, account, chat, draft.revision))
            } else {
                None
            }
        })
        .unwrap();
    let effects = app.update(
        Input::Store(StoreCompletion::DraftSaved {
            request,
            account,
            chat,
            revision,
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert!(effects.iter().any(|e| matches!(e, Effect::Shutdown)));
}

#[test]
fn failed_initial_load_can_retry_during_quit() {
    let mut app = App::new(Config::default());
    let fx = app.update(
        Input::Backend(BackendEvent::AccountKnown(account("test"))),
        Instant::now(),
    );
    let request = fx
        .iter()
        .find_map(|e| {
            if let Effect::LoadChats { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    let fx = app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(vec![snapshot("chat", "", 0).summary]),
        }),
        Instant::now(),
    );
    let load = fx
        .iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    press(&mut app, "enter");
    press(&mut app, "x");
    completion(&mut app, load, Err("read failed".into()));
    let fx = app.request_shutdown();
    let retry = fx
        .iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    let fx = completion(&mut app, retry, Ok(snapshot("chat", "old", 3)));
    assert_eq!(app.view().draft.text, "oldx");
    assert!(
        fx.iter()
            .any(|e| matches!(e,Effect::SaveDraft{draft,..}if draft.text=="oldx"))
    );
    assert!(!fx.iter().any(|e| matches!(e, Effect::Shutdown)));
}

#[tokio::test]
async fn shutdown_bypasses_full_command_jobs_and_flushes_the_draft() {
    use whatsapp_tui::{
        runtime::{self, Screen},
        whatsapp::*,
    };
    struct Sink;
    impl Screen for Sink {
        fn draw(&mut self, _: &ViewModel, _: &Config) -> std::io::Result<()> {
            Ok(())
        }
        fn finish(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    store
        .upsert_chats(
            account("test"),
            (0..72)
                .map(|n| snapshot(&format!("{n:02}"), "", 0).summary)
                .collect(),
        )
        .await
        .unwrap();
    store
        .apply_batch(batch(
            (0..72)
                .map(|n| message(key(&format!("{n:02}"), "alice", "one"), "hi"))
                .collect(),
        ))
        .await
        .unwrap();
    let (commands, mut requests) = tokio::sync::mpsc::channel(32);
    let (tx, events) = tokio::sync::mpsc::channel(1);
    let (stop, wait) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        tx.send(BackendEvent::AccountKnown(account("test")))
            .await
            .unwrap();
        let _ = wait.await;
        requests.close();
        while requests.recv().await.is_some() {}
        drop(tx);
        Ok(())
    });
    let backend = BackendHandle {
        commands,
        events,
        control: BackendControl::new(stop, task),
    };
    let mut keys = vec!["enter"];
    for _ in 0..64 {
        keys.extend(["esc", "esc", "j", "enter"]);
    }
    keys.extend(["x", "ctrl-q"]);
    let stream = futures_util::stream::unfold(keys.into_iter(), |mut keys| async move {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        keys.next().map(|key| {
            (
                Ok(crossterm::event::Event::Key(
                    whatsapp_tui::config::bindings::parse_key(key).unwrap(),
                )),
                keys,
            )
        })
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        runtime::run_with_screen(
            App::new(Config::default()),
            store.clone(),
            backend,
            &mut Sink,
            Box::pin(stream),
        ),
    )
    .await
    .expect("shutdown was blocked by command jobs")
    .unwrap();
    let chats = store.list_chats(account("test")).await.unwrap();
    let draft_chat = chats
        .into_iter()
        .find(|c| c.has_draft)
        .expect("quit must commit typed text");
    assert_eq!(
        store
            .snapshot(account("test"), draft_chat.chat, None)
            .await
            .unwrap()
            .draft
            .text,
        "x"
    );
}

fn render_app(app: &mut App, width: u16, height: u16) -> String {
    let view = app.view();
    let config = app.config.clone();
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| whatsapp_tui::ui::render(f, &view, &config))
        .unwrap();
    if let Some(metrics) = whatsapp_tui::ui::timeline_viewport(
        ratatui::layout::Rect::new(0, 0, width, height),
        &view,
        &config,
    ) {
        app.update(Input::TimelineViewport(metrics), Instant::now());
    }
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
#[test]
fn long_message_navigation_reveals_start_middle_and_end() {
    let mut app = ready_app();
    let fx = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let load = fx
        .iter()
        .find(|e| matches!(e, Effect::LoadChat { .. }))
        .unwrap();
    let text = (0..60)
        .map(|n| format!("ROW-{n:02} 界 👩‍💻\n"))
        .collect::<String>();
    let mut snap = snapshot("chat", "", 0);
    snap.messages = vec![message(key("chat", "alice", "long"), &text)];
    completion(&mut app, load, Ok(snap));
    press(&mut app, "tab");
    let mut seen = render_app(&mut app, 80, 24);
    for _ in 0..70 {
        press(&mut app, "up");
        seen.push_str(&render_app(&mut app, 80, 24));
    }
    assert!(seen.contains("ROW-00"), "opening text was inaccessible");
    assert!(seen.contains("ROW-30"), "middle was inaccessible");
    assert!(seen.contains("ROW-59"));
    assert!(!app.view().at_bottom);
    let selected = app.view().selected_message.clone();
    render_app(&mut app, 51, 14);
    for _ in 0..20 {
        press(&mut app, "pageup");
        render_app(&mut app, 51, 14);
    }
    assert_eq!(app.view().selected_message, selected);
    assert!(render_app(&mut app, 51, 14).contains("ROW-00"));
    press(&mut app, "end");
    let bottom = render_app(&mut app, 80, 24);
    assert!(bottom.contains("ROW-59"));
    assert!(app.view().at_bottom);
    assert_eq!(app.view().selected_message.unwrap().id.0, "long");
}
