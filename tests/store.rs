mod support;
use diesel::{Connection, connection::SimpleConnection};
use support::*;
use whatsapp_tui::{
    app::model::*,
    storage::{Store, StoreError, paths::DataDirGuard},
};

#[tokio::test]
async fn reopen_keeps_history_and_draft() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    store
        .stage_outgoing(outbound(key("chat", "test", "one"), draft("sent text", 1)))
        .await
        .unwrap();
    store
        .save_draft(account("test"), "chat".into(), draft("new words", 3))
        .await
        .unwrap();
    store.flush().await.unwrap();
    drop(store);
    let store = Store::open(path).await.unwrap();
    store.recover_sends(account("test")).await.unwrap();
    let s = store
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(s.draft.text, "new words");
    assert_eq!(s.messages[0].send_state, Some(SendState::Unconfirmed));
}
#[tokio::test]
async fn outgoing_commit_is_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    store
        .save_draft(account("test"), "chat".into(), draft("keep me", 1))
        .await
        .unwrap();
    let mut conn = diesel::sqlite::SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    conn.batch_execute("CREATE TRIGGER fail_clear BEFORE UPDATE ON drafts BEGIN SELECT RAISE(ABORT, 'test transaction failure'); END;").unwrap();
    assert!(
        store
            .stage_outgoing(outbound(key("chat", "test", "one"), draft("keep me", 1)))
            .await
            .is_err()
    );
    let s = store
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(s.draft.text, "keep me");
    assert!(s.messages.is_empty());
}
#[tokio::test]
async fn newer_draft_survives_older_send() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    store
        .save_draft(account("test"), "chat".into(), draft("new words", 2))
        .await
        .unwrap();
    store
        .stage_outgoing(outbound(key("chat", "test", "one"), draft("old words", 1)))
        .await
        .unwrap();
    assert_eq!(
        store
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .text,
        "new words"
    );
}
#[tokio::test]
async fn stale_draft_write_is_ignored() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    s.save_draft(account("test"), "chat".into(), draft("new", 2))
        .await
        .unwrap();
    s.save_draft(account("test"), "chat".into(), draft("old", 1))
        .await
        .unwrap();
    assert_eq!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .text,
        "new"
    );
    s.stage_outgoing(outbound(key("chat", "test", "one"), draft("new", 2)))
        .await
        .unwrap();
    s.save_draft(account("test"), "chat".into(), draft("new", 2))
        .await
        .unwrap();
    assert!(
        s.snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap()
            .draft
            .text
            .is_empty()
    );
}
#[tokio::test]
async fn message_keys_include_sender() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    s.apply_batch(batch(vec![
        message(key("g@g.us", "a", "id"), "first"),
        message(key("g@g.us", "b", "id"), "second"),
    ]))
    .await
    .unwrap();
    assert_eq!(
        s.snapshot(account("test"), "g@g.us".into(), None)
            .await
            .unwrap()
            .messages
            .len(),
        2
    );
}
#[tokio::test]
async fn accounts_are_isolated() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    s.apply_batch(batch(vec![message(key("chat", "a", "id"), "private")]))
        .await
        .unwrap();
    assert!(
        s.snapshot(account("other"), "chat".into(), None)
            .await
            .unwrap()
            .messages
            .is_empty()
    );
    assert!(s.list_chats(account("other")).await.unwrap().is_empty());
}
#[tokio::test]
async fn second_instance_is_rejected() {
    use std::os::unix::fs::PermissionsExt;
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("account");
    let first = DataDirGuard::acquire(&path).unwrap();
    assert!(matches!(
        DataDirGuard::acquire(&path),
        Err(StoreError::Locked)
    ));
    let s = Store::open(path.join("db")).await.unwrap();
    s.flush().await.unwrap();
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(path.join("db"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(first);
    assert!(DataDirGuard::acquire(&path).is_ok());
}
