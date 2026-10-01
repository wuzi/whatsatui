use super::*;
use crate::{
    app::model::*,
    notifications::{Notifier, Popup, Request},
};
use std::sync::Mutex;

async fn notification(
    request: Request,
    store: &Store,
    cancel: tokio::sync::watch::Receiver<bool>,
    notifier: &impl Notifier,
) -> Result<(), String> {
    let (_context, context) = tokio::sync::watch::channel(crate::notifications::Context {
        account: request.account().cloned(),
        reading: None,
        enabled: true,
        previews: true,
    });
    notification_scoped(request, store, cancel, notifier, context).await
}

struct Capture(Mutex<Vec<Popup>>);
async fn gated_context_change(change: &str) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let key = MessageKey {
        account: "self".into(),
        chat: if change == "alias" {
            "123@lid"
        } else {
            "friend"
        }
        .into(),
        sender: "friend".into(),
        id: "id".into(),
        from_me: false,
    };
    store
        .apply_batch(MessageBatch {
            account: key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(MessageRecord {
                key: key.clone(),
                body: MessageBody::Text("private synthetic content".into()),
                quote: None,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
                edited_at_ms: None,
                expires_at_ms: None,
                send_state: None,
            })],
        })
        .await
        .unwrap();
    if change == "alias" {
        store
            .merge_alias(key.account.clone(), "123@lid".into(), "friend".into())
            .await
            .unwrap();
    }
    let mut context = crate::notifications::Context {
        account: Some(key.account.clone()),
        reading: None,
        enabled: true,
        previews: true,
    };
    let (context_tx, context_rx) = tokio::sync::watch::channel(context.clone());
    let (_stop, cancel) = tokio::sync::watch::channel(false);
    let capture = Capture(Mutex::new(vec![]));
    // Hold the actual storage worker while the notification starts preparing.
    let (entered, waiting) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let held = store.clone();
    let blocker = tokio::spawn(async move {
        held.call(move |_| {
            entered.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(3)).unwrap();
            Ok(())
        })
        .await
        .unwrap()
    });
    waiting.await.unwrap();
    let run = notification_scoped(
        Request {
            keys: vec![key.clone()],
            overflow: None,
            previews: true,
        },
        &store,
        cancel,
        &capture,
        context_rx,
    );
    tokio::pin!(run);
    assert!(futures_util::poll!(&mut run).is_pending());
    match change {
        "account" => context.account = Some("different".into()),
        "focus" => context.reading = Some(key.chat),
        "alias" => context.reading = Some("friend".into()),
        "pairing" => context.enabled = false,
        "privacy" => context.previews = false,
        _ => unreachable!(),
    }
    context_tx.send_replace(context);
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(2), run)
        .await
        .unwrap()
        .unwrap();
    blocker.await.unwrap();
    let messages = capture.0.lock().unwrap();
    if change == "privacy" {
        assert_eq!(messages.len(), 1);
        assert!(!messages[0].body.contains("private synthetic content"));
    } else {
        assert!(
            messages.is_empty(),
            "stale {change} context must cancel private popup"
        );
    }
}
#[tokio::test]
async fn pending_notification_observes_account_change() {
    gated_context_change("account").await;
}
#[tokio::test]
async fn pending_notification_observes_focus_change() {
    gated_context_change("focus").await;
}
#[tokio::test]
async fn pending_notification_observes_pairing_change() {
    gated_context_change("pairing").await;
}
#[tokio::test]
async fn pending_notification_observes_privacy_change() {
    gated_context_change("privacy").await;
}
#[tokio::test]
async fn pending_notification_observes_canonical_chat_focus() {
    gated_context_change("alias").await;
}
#[async_trait::async_trait]
impl Notifier for Capture {
    async fn show(&self, popup: &Popup) -> Result<(), String> {
        self.0.lock().unwrap().push(popup.clone());
        Ok(())
    }
}
#[tokio::test]
async fn notification_effect_revalidates_and_obeys_shutdown_before_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let key = MessageKey {
        account: "self".into(),
        chat: "friend".into(),
        sender: "friend".into(),
        id: "id".into(),
        from_me: false,
    };
    let record = MessageRecord {
        key: key.clone(),
        body: MessageBody::Text("synthetic message".into()),
        quote: None,
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: None,
    };
    store
        .apply_batch(MessageBatch {
            account: key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(record)],
        })
        .await
        .unwrap();
    let request = Request {
        keys: vec![key.clone()],
        previews: true,
        overflow: None,
    };
    let capture = Capture(Mutex::new(vec![]));
    let (stop, cancel) = tokio::sync::watch::channel(false);
    notification(request.clone(), &store, cancel.clone(), &capture)
        .await
        .unwrap();
    assert_eq!(capture.0.lock().unwrap()[0].body, "synthetic message");
    stop.send(true).unwrap();
    notification(request.clone(), &store, cancel.clone(), &capture)
        .await
        .unwrap();
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "shutdown must suppress queued popups"
    );
    stop.send(false).unwrap();
    store
        .apply_batch(MessageBatch {
            account: key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Delete { key }],
        })
        .await
        .unwrap();
    notification(request, &store, cancel, &capture)
        .await
        .unwrap();
    assert_eq!(
        capture.0.lock().unwrap().len(),
        1,
        "deletion must be revalidated before calling the desktop"
    );
}

struct Pending(tokio::sync::Notify);
#[async_trait::async_trait]
impl Notifier for Pending {
    async fn show(&self, _: &Popup) -> Result<(), String> {
        self.0.notify_one();
        std::future::pending().await
    }
}
#[tokio::test]
async fn shutdown_cancels_a_notification_already_in_progress() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let key = MessageKey {
        account: "self".into(),
        chat: "friend".into(),
        sender: "friend".into(),
        id: "id".into(),
        from_me: false,
    };
    store
        .apply_batch(MessageBatch {
            account: key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(MessageRecord {
                key: key.clone(),
                body: MessageBody::Text("synthetic".into()),
                quote: None,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
                edited_at_ms: None,
                expires_at_ms: None,
                send_state: None,
            })],
        })
        .await
        .unwrap();
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let notifier = Pending(tokio::sync::Notify::new());
    let run = notification(
        Request {
            keys: vec![key],
            previews: true,
            overflow: None,
        },
        &store,
        cancel,
        &notifier,
    );
    let interrupt = async {
        notifier.0.notified().await;
        stop.send(true).unwrap();
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run, interrupt)
    })
    .await
    .unwrap();
    result.unwrap();
}

#[tokio::test]
async fn context_change_after_submission_does_not_resubmit() {
    change_after_submission_does_not_resubmit(false).await;
}

#[tokio::test]
async fn mute_change_cancels_a_notification_already_in_progress() {
    change_after_submission_does_not_resubmit(true).await;
}

async fn change_after_submission_does_not_resubmit(mute: bool) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let key = MessageKey {
        account: "self".into(),
        chat: "friend".into(),
        sender: "friend".into(),
        id: "id".into(),
        from_me: false,
    };
    store
        .apply_batch(MessageBatch {
            account: key.account.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(MessageRecord {
                key: key.clone(),
                body: MessageBody::Text("synthetic".into()),
                quote: None,
                created_at_ms: chrono::Utc::now().timestamp_millis(),
                edited_at_ms: None,
                expires_at_ms: None,
                send_state: None,
            })],
        })
        .await
        .unwrap();
    let (_stop, cancel) = tokio::sync::watch::channel(false);
    let (scope, context) = tokio::sync::watch::channel(crate::notifications::Context {
        account: Some(key.account.clone()),
        reading: None,
        enabled: true,
        previews: true,
    });
    let notifier = Pending(tokio::sync::Notify::new());
    let run = notification_scoped(
        Request {
            keys: vec![key],
            previews: true,
            overflow: None,
        },
        &store,
        cancel,
        &notifier,
        context,
    );
    let interrupt = async {
        notifier.0.notified().await;
        if mute {
            store
                .upsert_chats(
                    "self".into(),
                    vec![ChatSummary {
                        account: "self".into(),
                        chat: "friend".into(),
                        mute: Some(ChatMute {
                            until_ms: -1,
                            updated_at_ms: 1,
                        }),
                        ..Default::default()
                    }],
                )
                .await
                .unwrap();
        } else {
            scope.send_modify(|state| state.reading = Some("another-chat".into()));
        }
    };
    let (result, ()) = tokio::time::timeout(Duration::from_secs(1), async {
        tokio::join!(run, interrupt)
    })
    .await
    .unwrap();
    result.unwrap();
}
