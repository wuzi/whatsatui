mod support;
use support::*;
use whatsapp_tui::{app::model::*, config::Config, message_actions, storage::Store, ui};

fn media_body() -> MessageBody {
    serde_json::from_value(serde_json::json!({"Media": {
        "kind": "Image", "filename": "café.jpg", "mime": "image/jpeg",
        "caption": "*café* at the corner", "size": 8,
        "direct_path": "/v/t62.7118-24/example?hash=test",
        "media_key": vec![1; 32], "sha256": vec![2; 32], "encrypted_sha256": vec![3; 32]
    }}))
    .expect("received media records must deserialize")
}

#[test]
fn attachment_body_preserves_legacy_records_and_original_caption() {
    let old = r#"{"Unsupported":{"kind":"image","caption":"old caption"}}"#;
    let body: MessageBody = serde_json::from_str(old).unwrap();
    assert_eq!(serde_json::to_string(&body).unwrap(), old);
    let mut m = message(key("chat", "alice", "image"), "");
    m.body = media_body();
    assert_eq!(message_actions::text(&m, 0), Some("*café* at the corner"));
    assert!(!format!("{:?}", m.body).contains("hash=test"));
    let mut view = ready_app().view();
    view.messages[0].body = m.body;
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|f| ui::render(f, &view, &Config::default()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(text.contains("[image]"));
    assert!(text.contains("café.jpg"));
    assert!(text.contains("8 B"));
    assert!(text.contains("café at the corner"));
    assert!(!text.contains("*café*"));
}

#[tokio::test]
async fn persisted_media_caption_is_searchable_after_replay_and_restart() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("chat.sqlite3");
    let store = Store::open(path.clone()).await.unwrap();
    let mut m = message(key("chat", "alice", "image"), "");
    m.body = media_body();
    for _ in 0..2 {
        store.apply_batch(batch(vec![m.clone()])).await.unwrap();
    }
    store.flush().await.unwrap();
    drop(store);
    let store = Store::open(path).await.unwrap();
    assert_eq!(
        store
            .get_message(m.key.clone())
            .await
            .unwrap()
            .unwrap()
            .body,
        m.body
    );
    let hits = store
        .search_messages(account("test"), "chat".into(), "CAFÉ".into(), 0)
        .await
        .unwrap();
    assert_eq!(hits.hits.len(), 1);
    assert_eq!(hits.hits[0].key, m.key);
    assert!(
        store.list_chats(account("test")).await.unwrap()[0]
            .preview
            .contains("café")
    );
}
