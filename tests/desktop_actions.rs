mod support;
use std::sync::Mutex;
use support::*;
use whatsapp_tui::{
    app::model::*,
    desktop::{self, Desktop},
    message_actions::{self, DesktopAction},
    storage::Store,
};

#[derive(Default)]
struct Capture(Mutex<Vec<(String, String)>>);
#[async_trait::async_trait]
impl Desktop for Capture {
    async fn copy(&self, text: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(("copy".into(), text.into()));
        Ok(())
    }
    async fn open(&self, url: &str) -> Result<(), String> {
        self.0.lock().unwrap().push(("open".into(), url.into()));
        Ok(())
    }
}
#[test]
fn discovers_literal_web_links_in_order_with_balanced_punctuation() {
    assert_eq!(
        message_actions::web_links(
            "(https://example.org/a_(b)). https://example.org/a_(b) `https://example.org/日本語?q=1&x=2` *https://example.org/b*"
        ),
        [
            "https://example.org/a_(b)",
            "https://example.org/日本語?q=1&x=2",
            "https://example.org/b"
        ]
    );
    assert!(message_actions::web_links("file:///tmp/a javascript:alert(1) ftp://example.org/a https://user:pass@example.org/a https://example.org/\u{202e}evil").is_empty());
}
#[test]
fn link_discovery_distinguishes_formatting_from_literal_url_characters() {
    for (source, expected) in [
        ("_*https://example.org/a*_", "https://example.org/a"),
        ("~*https://example.org/a*~", "https://example.org/a"),
        ("foo_https://example.org/a_", "https://example.org/a_"),
        ("https://example.org/_path_", "https://example.org/_path_"),
        ("*https://example.org/_path_*", "https://example.org/_path_"),
        ("_read https://example.org/a_", "https://example.org/a"),
        ("`https://example.org/_path_`", "https://example.org/_path_"),
    ] {
        assert_eq!(message_actions::web_links(source), [expected], "{source}");
    }
}
#[test]
fn link_discovery_preserves_host_fragments_and_punctuation() {
    assert_eq!(
        message_actions::web_links(
            "(https://example.org#section). _https://example.org#other_ https://example.org/#section https://example.org?q=1#section https://example.org# https://example.org#part_(one)"
        ),
        [
            "https://example.org#section",
            "https://example.org#other",
            "https://example.org/#section",
            "https://example.org?q=1#section",
            "https://example.org#",
            "https://example.org#part_(one)"
        ]
    );
}
#[test]
fn discovery_and_copy_text_have_explicit_limits() {
    let source = (0..80)
        .map(|i| format!("https://example.org/{i} "))
        .collect::<String>();
    assert_eq!(message_actions::web_links(&source).len(), 32);
    assert!(
        message_actions::web_links(&format!("https://example.org/{}", "x".repeat(4096))).is_empty()
    );
    let mut m = message(key("chat", "alice", "one"), "text");
    m.expires_at_ms = Some(100);
    assert!(message_actions::text(&m, 100).is_none());
    m.body = MessageBody::Deleted;
    assert!(message_actions::text(&m, 0).is_none());
}
#[tokio::test]
async fn copies_original_body_and_caption_and_only_selected_member_links() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let integration = Capture::default();
    let mut m = message(
        key("chat", "alice", "one"),
        "*café*\nhttps://example.org/?q=$(touch_nope)&x=1",
    );
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    desktop::execute(
        m.clone(),
        DesktopAction::CopyText,
        store.clone(),
        &integration,
    )
    .await
    .unwrap();
    desktop::execute(
        m.clone(),
        DesktopAction::OpenLink("https://example.org/?q=$(touch_nope)&x=1".into()),
        store.clone(),
        &integration,
    )
    .await
    .unwrap();
    assert!(
        desktop::execute(
            m.clone(),
            DesktopAction::OpenLink("https://unselected.example/".into()),
            store.clone(),
            &integration
        )
        .await
        .is_err()
    );
    assert_eq!(
        *integration.0.lock().unwrap(),
        vec![
            (
                "copy".into(),
                "*café*\nhttps://example.org/?q=$(touch_nope)&x=1".into()
            ),
            (
                "open".into(),
                "https://example.org/?q=$(touch_nope)&x=1".into()
            )
        ]
    );
    m.key.id = "caption".into();
    m.body = MessageBody::Unsupported {
        kind: "image".into(),
        caption: Some("_caption_".into()),
    };
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    desktop::execute(m, DesktopAction::CopyText, store.clone(), &integration)
        .await
        .unwrap();
    assert_eq!(integration.0.lock().unwrap().last().unwrap().1, "_caption_");
    let big = message(key("chat", "alice", "big"), &"x".repeat(1024 * 1024 + 1));
    store.apply_batch(batch(vec![big.clone()])).await.unwrap();
    assert!(
        desktop::execute(big, DesktopAction::CopyText, store, &integration)
            .await
            .is_err()
    );
    assert_eq!(integration.0.lock().unwrap().len(), 3);
}
#[tokio::test]
async fn rereads_before_actions_and_rejects_obsolete_content() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("chat.sqlite3")).await.unwrap();
    let integration = Capture::default();
    let m = message(key("chat", "alice", "one"), "old https://example.org/");
    store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    let mut changed = m.clone();
    changed.body = MessageBody::Text("new".into());
    changed.edited_at_ms = Some(1_900_000_000_000);
    store.apply_batch(batch(vec![changed])).await.unwrap();
    assert!(
        desktop::execute(m, DesktopAction::CopyText, store.clone(), &integration)
            .await
            .is_err()
    );
    for body in [MessageBody::Deleted, MessageBody::Expired] {
        let mut m = message(key("chat", "alice", &format!("{body:?}")), "hidden");
        m.body = body;
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
        assert!(
            desktop::execute(m, DesktopAction::CopyText, store.clone(), &integration)
                .await
                .is_err()
        );
    }
    let mut expired = message(key("chat", "alice", "deadline"), "expired before sweep");
    expired.expires_at_ms = Some(1);
    store
        .apply_batch(batch(vec![expired.clone()]))
        .await
        .unwrap();
    assert!(
        desktop::execute(
            expired,
            DesktopAction::CopyText,
            store.clone(),
            &integration
        )
        .await
        .is_err()
    );
    let alias = message(key("alias", "alice", "alias"), "moved");
    store.apply_batch(batch(vec![alias.clone()])).await.unwrap();
    store
        .merge_alias(account("test"), "alias".into(), "canonical".into())
        .await
        .unwrap();
    assert!(
        desktop::execute(alias, DesktopAction::CopyText, store, &integration)
            .await
            .is_err()
    );
    assert!(integration.0.lock().unwrap().is_empty());
}
