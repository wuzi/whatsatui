mod support;
use support::*;
use whatsapp_tui::{app::model::*, storage::Store};

fn reaction(key: MessageKey, reactor: &str, emoji: &str, at_ms: i64) -> MessageChange {
    MessageChange::Reaction(Reaction {
        key,
        reactor: reactor.into(),
        emoji: emoji.into(),
        at_ms,
        event_id: format!("reaction-{at_ms}").into(),
    })
}
async fn apply(s: &Store, changes: Vec<MessageChange>) {
    s.apply_batch(MessageBatch {
        account: account("test"),
        source: MessageSource::Live,
        changes,
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn reactions_before_original_replay_replace_remove_and_keep_unread_and_preview() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let k = key("group@g.us", "alice", "original");
    apply(&s, vec![reaction(k.clone(), "bob", "👍", 20)]).await;
    apply(
        &s,
        vec![MessageChange::Upsert(message(k.clone(), "original text"))],
    )
    .await;
    for _ in 0..2 {
        apply(
            &s,
            vec![
                reaction(k.clone(), "bob", "👍", 20),
                reaction(k.clone(), "test", "👍", 25),
            ],
        )
        .await;
    }
    let snap = s
        .snapshot(account("test"), "group@g.us".into(), None)
        .await
        .unwrap();
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.summary.unread, 1);
    assert_eq!(snap.summary.preview, "original text");
    assert_eq!(snap.interactions.reactions.len(), 2);
    apply(&s, vec![reaction(k.clone(), "bob", "❤️", 30)]).await;
    apply(
        &s,
        vec![
            reaction(k.clone(), "bob", "", 40),
            reaction(k.clone(), "bob", "👍", 20),
        ],
    )
    .await;
    drop(s);
    let s = Store::open(d.path().join("db")).await.unwrap();
    apply(&s, vec![reaction(k.clone(), "bob", "❤️", 30)]).await;
    let snap = s
        .snapshot(account("test"), k.chat.clone(), None)
        .await
        .unwrap();
    assert_eq!(snap.interactions.reactions.len(), 1);
    assert_eq!(snap.interactions.reactions[0].reactor.0, "test");
    apply(
        &s,
        vec![
            MessageChange::Delete { key: k.clone() },
            reaction(k.clone(), "bob", "🔥", 50),
        ],
    )
    .await;
    let snap = s.snapshot(account("test"), k.chat, None).await.unwrap();
    assert!(snap.interactions.reactions.is_empty());
}
#[tokio::test]
async fn aliases_merge_target_and_reactor_without_reviving_removed_reactions_or_crossing_accounts()
{
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let old = key("123@lid", "123@lid", "original");
    let canonical = key("555@s.whatsapp.net", "555@s.whatsapp.net", "original");
    apply(
        &s,
        vec![
            MessageChange::Upsert(message(old.clone(), "hi")),
            reaction(old.clone(), "123@lid", "👍", 10),
            reaction(canonical.clone(), "555@s.whatsapp.net", "", 20),
        ],
    )
    .await;
    let mut other = canonical.clone();
    other.account = "other".into();
    s.apply_batch(MessageBatch {
        account: "other".into(),
        source: MessageSource::History,
        changes: vec![
            MessageChange::Upsert(message(other.clone(), "other")),
            reaction(other.clone(), "123@lid", "❤️", 30),
        ],
    })
    .await
    .unwrap();
    s.merge_alias(
        account("test"),
        "123@lid".into(),
        "555@s.whatsapp.net".into(),
    )
    .await
    .unwrap();
    apply(&s, vec![reaction(old.clone(), "123@lid", "🔥", 15)]).await;
    let snap = s
        .snapshot(account("test"), canonical.chat.clone(), None)
        .await
        .unwrap();
    assert!(snap.interactions.reactions.is_empty());
    apply(&s, vec![reaction(old, "123@lid", "😀", 40)]).await;
    let snap = s
        .snapshot(account("test"), canonical.chat, None)
        .await
        .unwrap();
    assert_eq!(
        snap.interactions.reactions[0].reactor.0,
        "555@s.whatsapp.net"
    );
    assert_eq!(
        snap.interactions.reactions[0].key.sender.0,
        "555@s.whatsapp.net"
    );
    let snap = s.snapshot("other".into(), other.chat, None).await.unwrap();
    assert_eq!(snap.interactions.reactions[0].reactor.0, "123@lid");
}
#[tokio::test]
async fn equal_timestamp_removal_wins_and_expiry_scrubs_reactions() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let k = key("chat", "alice", "one");
    let mut m = message(k.clone(), "hi");
    m.expires_at_ms = Some(100);
    apply(
        &s,
        vec![
            MessageChange::Upsert(m),
            reaction(k.clone(), "bob", "", 20),
            reaction(k.clone(), "bob", "👍", 20),
        ],
    )
    .await;
    assert!(
        s.snapshot(account("test"), k.chat.clone(), None)
            .await
            .unwrap()
            .interactions
            .reactions
            .is_empty()
    );
    apply(&s, vec![reaction(k.clone(), "bob", "👍", 30)]).await;
    s.expire(account("test"), 101).await.unwrap();
    assert!(
        s.snapshot(account("test"), k.chat, None)
            .await
            .unwrap()
            .interactions
            .reactions
            .is_empty()
    );
}

#[tokio::test]
async fn version_two_database_migrates_without_losing_saved_drafts() {
    use diesel::{Connection, RunQueryDsl, connection::SimpleConnection};
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("db");
    let mut db = diesel::SqliteConnection::establish(path.to_str().unwrap()).unwrap();
    db.batch_execute(include_str!("../migrations/00000000000001_initial/up.sql"))
        .unwrap();
    db.batch_execute(include_str!(
        "../migrations/00000000000002_draft_barrier/up.sql"
    ))
    .unwrap();
    diesel::sql_query("INSERT INTO drafts(account,chat,revision,data) VALUES('test','chat',3,?)")
        .bind::<diesel::sql_types::Text, _>(
            serde_json::to_string(&draft("saved before upgrade", 3)).unwrap(),
        )
        .execute(&mut db)
        .unwrap();
    drop(db);
    let s = Store::open(path).await.unwrap();
    let m = message(key("chat", "alice", "one"), "hello");
    apply(
        &s,
        vec![
            MessageChange::Upsert(m.clone()),
            reaction(m.key, "test", "👍", 10),
        ],
    )
    .await;
    let snap = s
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(snap.draft.text, "saved before upgrade");
    assert_eq!(snap.interactions.reactions[0].emoji, "👍");
}
