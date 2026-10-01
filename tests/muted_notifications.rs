mod support;
use support::*;
use whatsapp_tui::{
    app::model::*,
    notifications::{self, Overflow, Request},
    storage::Store,
};

const NOW: i64 = 1_790_880_000_000;

fn chat_mute(chat: &str, until_ms: i64, updated_at_ms: i64) -> ChatSummary {
    // JSON also exercises persisted chat data, including migration from rows
    // written before the optional mute field existed.
    let mut value = serde_json::to_value(ChatSummary {
        account: account("test"),
        chat: chat.into(),
        name: chat.into(),
        is_group: chat.ends_with("@g.us"),
        ..Default::default()
    })
    .unwrap();
    value["mute"] = serde_json::json!({"until_ms": until_ms, "updated_at_ms": updated_at_ms});
    serde_json::from_value(value).unwrap()
}

async fn save_mute(store: &Store, chat: &str, until_ms: i64, updated_at_ms: i64) {
    store
        .upsert_chats(
            account("test"),
            vec![chat_mute(chat, until_ms, updated_at_ms)],
        )
        .await
        .unwrap();
}

fn request(messages: &[MessageRecord], overflow: bool, previews: bool) -> Request {
    Request {
        keys: messages.iter().take(128).map(|m| m.key.clone()).collect(),
        overflow: overflow.then(|| Overflow {
            account: account("test"),
            since_ms: NOW,
            until_ms: NOW,
        }),
        previews,
    }
}

fn incoming(chat: &str, id: &str) -> MessageRecord {
    let mut message = message(key(chat, "alice", id), "Synthetic message");
    message.created_at_ms = NOW;
    message
}

#[tokio::test]
async fn muted_chats_suppress_popups_until_unmuted_or_expired() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    for chat in ["group@g.us", "friend@s.whatsapp.net"] {
        let message = incoming(chat, "one");
        store
            .apply_batch(batch(vec![message.clone()]))
            .await
            .unwrap();
        for (revision, until_ms, notify) in [
            (1, -1, false),
            (2, NOW + 1, false),
            (3, NOW, true),
            (4, NOW - 1, true),
            (5, 0, true),
        ] {
            save_mute(&store, chat, until_ms, revision).await;
            for previews in [true, false] {
                let popup = notifications::prepare(
                    request(std::slice::from_ref(&message), false, previews),
                    &store,
                    NOW,
                )
                .await
                .unwrap();
                assert_eq!(
                    popup.is_some(),
                    notify,
                    "{chat}, mute={until_ms}, previews={previews}"
                );
            }
        }
    }
}

#[tokio::test]
async fn muted_overflow_cannot_produce_a_generic_popup_or_hide_an_unmuted_chat() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let mut messages: Vec<_> = (0..128)
        .map(|i| incoming("first@g.us", &format!("{i}")))
        .collect();
    messages.push(incoming("overflow@g.us", "omitted"));
    store.apply_batch(batch(messages.clone())).await.unwrap();
    save_mute(&store, "first@g.us", -1, 1).await;
    save_mute(&store, "overflow@g.us", -1, 1).await;
    let pending = request(&messages, true, true);
    assert!(
        notifications::prepare(pending.clone(), &store, NOW)
            .await
            .unwrap()
            .is_none()
    );
    save_mute(&store, "overflow@g.us", 0, 2).await;
    assert!(
        notifications::prepare(pending.clone(), &store, NOW)
            .await
            .unwrap()
            .is_some()
    );
    save_mute(&store, "overflow@g.us", NOW + 1, 3).await;
    assert!(
        notifications::prepare(pending, &store, NOW)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn mixed_popup_excludes_muted_names_text_and_counts() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let mut quiet = incoming("quiet@g.us", "quiet");
    quiet.body = MessageBody::Text("Muted secret".into());
    let messages = vec![quiet, incoming("loud@g.us", "loud")];
    store.apply_batch(batch(messages.clone())).await.unwrap();
    save_mute(&store, "quiet@g.us", -1, 1).await;
    let popup = notifications::prepare(request(&messages, false, true), &store, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(popup.title, "loud@g.us");
    assert!(!popup.body.contains("Muted secret"));
    let popup = notifications::prepare(request(&messages, false, false), &store, NOW)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(popup.body, "New message");
}

#[tokio::test]
async fn mute_survives_restart_metadata_refresh_and_stale_sync() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("db");
    let store = Store::open(path.clone()).await.unwrap();
    let messages = vec![incoming("group@g.us", "one")];
    store.apply_batch(batch(messages.clone())).await.unwrap();
    save_mute(&store, "group@g.us", -1, 20).await;
    drop(store);
    let store = Store::open(path).await.unwrap();
    store
        .upsert_chats(
            account("test"),
            vec![ChatSummary {
                account: account("test"),
                chat: "group@g.us".into(),
                name: "Renamed group".into(),
                is_group: true,
                ..Default::default()
            }],
        )
        .await
        .unwrap();
    // An older app-state update and a history snapshot must not unmute it.
    save_mute(&store, "group@g.us", 0, 19).await;
    save_mute(&store, "group@g.us", 0, 0).await;
    assert!(
        notifications::prepare(request(&messages, false, true), &store, NOW)
            .await
            .unwrap()
            .is_none()
    );
    save_mute(&store, "group@g.us", 0, 21).await;
    save_mute(&store, "group@g.us", -1, 20).await;
    assert!(
        notifications::prepare(request(&messages, false, true), &store, NOW)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn muted_aliases_merge_by_latest_setting_and_stay_account_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let messages = vec![incoming("123@lid", "one")];
    store.apply_batch(batch(messages.clone())).await.unwrap();
    save_mute(&store, "123@lid", -1, 20).await;
    save_mute(&store, "456@s.whatsapp.net", 0, 10).await;
    store
        .merge_alias(
            account("test"),
            "123@lid".into(),
            "456@s.whatsapp.net".into(),
        )
        .await
        .unwrap();
    assert!(
        notifications::prepare(request(&messages, false, true), &store, NOW)
            .await
            .unwrap()
            .is_none()
    );
    let mut foreign = messages[0].clone();
    foreign.key.account = account("other");
    store
        .apply_batch(MessageBatch {
            account: account("other"),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(foreign.clone())],
        })
        .await
        .unwrap();
    assert!(
        notifications::prepare(request(&[foreign], false, true), &store, NOW)
            .await
            .unwrap()
            .is_some()
    );
    save_mute(&store, "123@lid", 0, 21).await;
    assert!(
        notifications::prepare(request(&messages, false, true), &store, NOW)
            .await
            .unwrap()
            .is_some()
    );
}
