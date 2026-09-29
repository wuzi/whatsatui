mod support;
use support::*;
use unicode_segmentation::UnicodeSegmentation;
use whatsapp_tui::{app::model::*, storage::Store};
const NOW: i64 = 1_800_000_000_000;
async fn search(store: &Store, chat: &str, query: &str) -> MessageSearchPage {
    store
        .search_messages(account("test"), chat.into(), query.into(), NOW)
        .await
        .unwrap()
}
fn ids(page: &MessageSearchPage) -> Vec<&str> {
    page.hits.iter().map(|h| h.key.id.0.as_str()).collect()
}

#[tokio::test]
async fn search_is_literal_unicode_and_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut caption = message(key("chat", "alice", "caption"), "");
    caption.body = MessageBody::Unsupported {
        kind: "image".into(),
        caption: Some("CAFÉ with friends".into()),
    };
    let mut quote = message(key("chat", "alice", "quote"), "thanks");
    quote.quote = Some(Quote {
        key: key("chat", "alice", "missing"),
        preview: "café".into(),
        availability: QuoteAvailability::Available,
    });
    store
        .apply_batch(batch(vec![
            message(key("chat", "alice", "text"), "café amanhã"),
            caption,
            quote,
            message(key("other", "alice", "wrongchat"), "café"),
            message(key("chat", "alice", "literal"), "100%_done quote's \\ yes"),
        ]))
        .await
        .unwrap();
    let mut other = message(key("chat", "alice", "wrongaccount"), "café");
    other.key.account = account("other");
    store
        .apply_batch(MessageBatch {
            account: account("other"),
            source: MessageSource::History,
            changes: vec![MessageChange::Upsert(other)],
        })
        .await
        .unwrap();
    store
        .save_draft(account("test"), "chat".into(), draft("café unsent", 1))
        .await
        .unwrap();
    assert_eq!(
        ids(&search(&store, "chat", "CAFÉ").await),
        ["text", "caption"]
    );
    for query in ["%_", "quote's", "\\ yes"] {
        assert_eq!(ids(&search(&store, "chat", query).await), ["literal"]);
    }
    assert!(search(&store, "chat", "cafe").await.hits.is_empty());
    assert!(search(&store, "chat", "  ").await.hits.is_empty());
    assert!(
        store
            .search_messages(account("test"), "chat".into(), "界".repeat(257), NOW)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn search_tracks_edits_deletions_and_expiry() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let mut expired = message(key("chat", "alice", "expired"), "needle");
    expired.expires_at_ms = Some(NOW - 1);
    store
        .apply_batch(batch(vec![
            expired,
            message(key("chat", "alice", "edit"), "needle"),
            message(key("chat", "alice", "delete"), "needle"),
        ]))
        .await
        .unwrap();
    assert_eq!(
        ids(&search(&store, "chat", "needle").await),
        ["edit", "delete"]
    );
    store
        .apply_batch(MessageBatch {
            account: account("test"),
            source: MessageSource::Live,
            changes: vec![
                MessageChange::Edit {
                    key: key("chat", "alice", "edit"),
                    text: "replacement".into(),
                    edited_at_ms: NOW,
                },
                MessageChange::Delete {
                    key: key("chat", "alice", "delete"),
                },
            ],
        })
        .await
        .unwrap();
    assert!(search(&store, "chat", "needle").await.hits.is_empty());
    assert_eq!(ids(&search(&store, "chat", "replacement").await), ["edit"]);
    store
        .apply_batch(MessageBatch {
            account: account("test"),
            source: MessageSource::Live,
            changes: vec![MessageChange::Expire {
                key: key("chat", "alice", "edit"),
            }],
        })
        .await
        .unwrap();
    assert!(search(&store, "chat", "replacement").await.hits.is_empty());
}

#[tokio::test]
async fn search_finds_older_pages_with_bounded_results() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let messages = (0..205)
        .map(|i| {
            let mut m = message(
                key("chat", "alice", &format!("m{i:03}")),
                if i == 0 { "archive needle" } else { "archive" },
            );
            m.created_at_ms += i;
            m
        })
        .collect();
    store.apply_batch(batch(messages)).await.unwrap();
    assert_eq!(ids(&search(&store, "chat", "needle").await), ["m000"]);
    let page = search(&store, "chat", "archive").await;
    assert_eq!(page.hits.len(), 50);
    assert!(page.has_more);
    assert_eq!(page.hits[0].key.id.0, "m204");
    assert_eq!(page.hits[49].key.id.0, "m155");
    assert!(!search(&store, "chat", "needle").await.has_more);
}

#[tokio::test]
async fn search_resolves_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    store
        .apply_batch(batch(vec![message(
            key("alias@lid", "alias@lid", "one"),
            "needle",
        )]))
        .await
        .unwrap();
    store
        .merge_alias(
            account("test"),
            "alias@lid".into(),
            "123@s.whatsapp.net".into(),
        )
        .await
        .unwrap();
    for chat in ["alias@lid", "123@s.whatsapp.net"] {
        let page = search(&store, chat, "needle").await;
        assert_eq!(ids(&page), ["one"]);
        assert_eq!(page.hits[0].key.chat.0, "123@s.whatsapp.net");
        assert_eq!(page.hits[0].key.sender.0, "123@s.whatsapp.net");
    }
}

#[tokio::test]
async fn search_excerpt_contains_distant_match() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let body = format!("{} CAFÉ 👩‍💻 {}", "界".repeat(500), "a\u{301}".repeat(200));
    store
        .apply_batch(batch(vec![message(key("chat", "alice", "one"), &body)]))
        .await
        .unwrap();
    let page = search(&store, "chat", "café").await;
    assert_eq!(page.hits.len(), 1);
    assert!(page.hits[0].preview.contains("CAFÉ 👩‍💻"));
    assert!(page.hits[0].preview.graphemes(true).count() <= 162);
    assert!(page.hits[0].preview.starts_with('…'));
}
