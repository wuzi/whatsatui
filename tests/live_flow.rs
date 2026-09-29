mod support;
use crossterm::event::Event;
use diesel::{Connection, connection::SimpleConnection};
use support::*;
use tokio::{sync::mpsc, time::Instant};
use whatsapp_tui::{
    app::model::*,
    app::*,
    config::Config,
    runtime,
    storage::Store,
    whatsapp::{BackendCommand, BackendEvent},
};
fn submit(app: &mut App) -> (RequestId, Draft, Vec<Effect>) {
    press(app, "enter");
    app.update(Input::Terminal(Event::Paste("old".into())), Instant::now());
    let effects = press(app, "enter");
    let (r, d) = effects
        .iter()
        .find_map(|e| {
            if let Effect::Prepare { request, draft, .. } = e {
                Some((*request, draft.clone()))
            } else {
                None
            }
        })
        .unwrap();
    (r, d, effects)
}
async fn effects(
    app: &mut App,
    store: &Store,
    tx: &mpsc::Sender<BackendCommand>,
    list: Vec<Effect>,
) -> Vec<Effect> {
    let mut next = vec![];
    for effect in list {
        if let Some(input) = runtime::execute(effect, store.clone(), tx.clone()).await {
            next.extend(app.update(input, Instant::now()));
        }
    }
    next
}
#[tokio::test]
async fn send_waits_for_commit() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    let (tx, mut rx) = mpsc::channel(32);
    let mut app = ready_app();
    let (request, draft, list) = submit(&mut app);
    effects(&mut app, &store, &tx, list).await;
    assert!(matches!(
        rx.try_recv().unwrap(),
        BackendCommand::PrepareText { .. }
    ));
    assert!(rx.try_recv().is_err());
    let message = outbound(key("chat", "test", "one"), draft);
    let staged = app.update(
        Input::Backend(BackendEvent::Prepared { request, message }),
        Instant::now(),
    );
    assert!(rx.try_recv().is_err());
    let transmit = effects(&mut app, &store, &tx, staged).await;
    assert!(rx.try_recv().is_err());
    let stored = store
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(stored.messages.len(), 1);
    effects(&mut app, &store, &tx, transmit).await;
    assert!(matches!(
        rx.try_recv().unwrap(),
        BackendCommand::Transmit(_)
    ));
}
#[tokio::test]
async fn commit_failure_keeps_draft() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let store = Store::open(path.clone()).await.unwrap();
    let (tx, mut rx) = mpsc::channel(32);
    let mut app = ready_app();
    let (request, draft, _) = submit(&mut app);
    store
        .save_draft(account("test"), "chat".into(), draft.clone())
        .await
        .unwrap();
    let mut c = diesel::SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    c.batch_execute("CREATE TRIGGER fail_send BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'failure'); END;").unwrap();
    let staged = app.update(
        Input::Backend(BackendEvent::Prepared {
            request,
            message: outbound(key("chat", "test", "one"), draft),
        }),
        Instant::now(),
    );
    let next = effects(&mut app, &store, &tx, staged).await;
    effects(&mut app, &store, &tx, next).await;
    assert!(rx.try_recv().is_err());
    assert_eq!(app.view().draft.text, "old");
    assert!(
        store
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
}
#[tokio::test]
async fn new_typing_survives_send_completion() {
    for save_before_stage in [false, true] {
        let d = tempfile::tempdir().unwrap();
        let store = Store::open(d.path().join("db")).await.unwrap();
        let (tx, _rx) = mpsc::channel(32);
        let mut app = ready_app();
        let (request, draft, _) = submit(&mut app);
        let message = outbound(key("chat", "test", "one"), draft);
        let staged = app.update(
            Input::Backend(BackendEvent::Prepared { request, message }),
            Instant::now(),
        );
        app.update(Input::Terminal(Event::Paste("new".into())), Instant::now());
        let latest = app.view().draft;
        if save_before_stage {
            store
                .save_draft(account("test"), "chat".into(), latest.clone())
                .await
                .unwrap();
        }
        effects(&mut app, &store, &tx, staged).await;
        assert_eq!(app.view().draft.text, "oldnew");
        assert_eq!(app.view().draft.revision, 2);
        if !save_before_stage {
            store
                .save_draft(account("test"), "chat".into(), latest)
                .await
                .unwrap();
        }
        let snapshot = store
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap();
        assert_eq!(snapshot.draft.text, "oldnew");
        assert_eq!(snapshot.draft.revision, 2);
    }
}
#[test]
fn double_enter_submits_once() {
    let mut app = ready_app();
    let (_, _, first) = submit(&mut app);
    let second = press(&mut app, "enter");
    assert_eq!(
        first
            .iter()
            .chain(&second)
            .filter(|e| matches!(e, Effect::Prepare { .. }))
            .count(),
        1
    );
}
#[tokio::test]
async fn restart_never_resends() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let store = Store::open(path.clone()).await.unwrap();
    store
        .stage_outgoing(outbound(key("chat", "test", "one"), draft("hello", 1)))
        .await
        .unwrap();
    let (tx, mut rx) = mpsc::channel(32);
    let mut app = ready_app();
    let list = app.update(
        Input::Backend(BackendEvent::ConnectionChanged {
            state: ConnectionState::Reconnecting,
            reason: None,
        }),
        Instant::now(),
    );
    effects(&mut app, &store, &tx, list).await;
    assert_eq!(
        store
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages[0]
            .send_state,
        Some(SendState::Unconfirmed)
    );
    let list = app.update(
        Input::Backend(BackendEvent::ConnectionChanged {
            state: ConnectionState::Connected,
            reason: None,
        }),
        Instant::now(),
    );
    effects(&mut app, &store, &tx, list).await;
    let reopened = Store::open(path).await.unwrap();
    reopened.recover_sends(account("test")).await.unwrap();
    assert!(rx.try_recv().is_err());
}
#[tokio::test]
async fn late_receipt_resolves_original_attempt() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    for id in ["old", "new"] {
        store
            .stage_outgoing(outbound(key("chat", "test", id), draft("text", 1)))
            .await
            .unwrap();
    }
    store.recover_sends(account("test")).await.unwrap();
    store
        .record_receipt(Receipt {
            key: key("chat", "test", "old"),
            recipient: "alice".into(),
            state: ReceiptState::Read,
            at_ms: 1,
        })
        .await
        .unwrap();
    let snapshot = store
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(
        snapshot
            .messages
            .iter()
            .find(|m| m.key.id.0 == "old")
            .unwrap()
            .send_state,
        Some(SendState::Read)
    );
    assert_eq!(
        snapshot
            .messages
            .iter()
            .find(|m| m.key.id.0 == "new")
            .unwrap()
            .send_state,
        Some(SendState::Unconfirmed)
    );
}
#[test]
fn account_change_ignores_stale_completion() {
    let mut app = ready_app();
    let (request, draft, _) = submit(&mut app);
    let message = outbound(key("chat", "test", "one"), draft);
    app.update(
        Input::Backend(BackendEvent::Prepared {
            request,
            message: message.clone(),
        }),
        Instant::now(),
    );
    app.update(
        Input::Backend(BackendEvent::AccountKnown("different".into())),
        Instant::now(),
    );
    let next = app.update(
        Input::Store(StoreCompletion::Staged {
            request,
            message,
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert!(!next.iter().any(|e| matches!(e, Effect::Transmit(_))));
    assert_eq!(app.view().account.unwrap().0, "different");
    assert!(app.view().draft.text.is_empty());
}
#[test]
fn unexpected_preparation_never_stages() {
    let mut app = ready_app();
    let next = app.update(
        Input::Backend(BackendEvent::Prepared {
            request: RequestId(1000),
            message: outbound(key("chat", "test", "foreign"), draft("bad", 1)),
        }),
        Instant::now(),
    );
    assert!(!next.iter().any(|e| matches!(e, Effect::Stage { .. })));
}
#[test]
fn background_refresh_does_not_drop_typing() {
    let mut app = ready_app();
    press(&mut app, "enter");
    app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    app.update(Input::Terminal(Event::Paste("kept".into())), Instant::now());
    assert_eq!(app.view().draft.text, "kept");
}
#[test]
fn typing_during_initial_load_is_replayed_over_saved_draft() {
    let mut app = App::new(Config::default());
    let now = Instant::now();
    let list = app.update(
        Input::Backend(BackendEvent::AccountKnown(account("test"))),
        now,
    );
    let request = list
        .iter()
        .find_map(|e| {
            if let Effect::LoadChats { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    let summary = ChatSummary {
        account: account("test"),
        chat: "chat".into(),
        name: "Alice".into(),
        ..Default::default()
    };
    let list = app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(vec![summary.clone()]),
        }),
        now,
    );
    let request = list
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    press(&mut app, "enter");
    app.update(Input::Terminal(Event::Paste(" typed".into())), now);
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                summary,
                messages: vec![],
                draft: draft("saved", 7),
                receipts: vec![],
                has_older: false,
            })),
        }),
        now,
    );
    assert_eq!(app.view().draft.text, "saved typed");
    assert_eq!(app.view().draft.revision, 8);
}
#[test]
fn quitting_during_stage_never_transmits() {
    let mut app = ready_app();
    let (request, draft, _) = submit(&mut app);
    let message = outbound(key("chat", "test", "one"), draft);
    app.update(
        Input::Backend(BackendEvent::Prepared {
            request,
            message: message.clone(),
        }),
        Instant::now(),
    );
    app.request_shutdown();
    let next = app.update(
        Input::Store(StoreCompletion::Staged {
            request,
            message,
            result: Ok(()),
        }),
        Instant::now(),
    );
    assert!(!next.iter().any(|e| matches!(e, Effect::Transmit(_))));
    assert!(next.iter().any(|e| matches!(
        e,
        Effect::PersistOutcome {
            state: SendState::Unconfirmed,
            ..
        }
    )));
}
#[tokio::test]
async fn resend_staging_preserves_current_draft() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    store
        .save_draft(account("test"), "chat".into(), draft("unfinished", 1))
        .await
        .unwrap();
    let (tx, _rx) = mpsc::channel(1);
    runtime::execute(
        Effect::Stage {
            request: RequestId(100),
            message: outbound(
                key("chat", "test", "new-attempt"),
                draft("resent message", 8),
            ),
            preserve_draft: true,
        },
        store.clone(),
        tx,
    )
    .await
    .unwrap();
    let s = store
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(s.draft.text, "unfinished");
    assert_eq!(s.draft.revision, 1);
    assert_eq!(s.messages.len(), 1);
}
#[tokio::test]
async fn unexpected_backend_stop_recovers_pending_attempt() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    store
        .stage_outgoing(outbound(key("chat", "test", "one"), draft("hello", 1)))
        .await
        .unwrap();
    let (tx, _rx) = mpsc::channel(32);
    let mut app = ready_app();
    let list = app.update(Input::Backend(BackendEvent::Stopped), Instant::now());
    effects(&mut app, &store, &tx, list).await;
    assert_eq!(
        store
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .messages[0]
            .send_state,
        Some(SendState::Unconfirmed)
    );
}
