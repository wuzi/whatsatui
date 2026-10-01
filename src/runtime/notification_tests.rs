use super::*;
use crate::{
    app::model::*,
    notifications::{Notifier, Popup, Request},
};
use std::sync::Mutex;

struct Capture(Mutex<Vec<Popup>>);
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
        overflow: false,
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
            overflow: false,
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
