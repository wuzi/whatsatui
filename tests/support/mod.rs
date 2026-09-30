#![allow(dead_code)]
use whatsapp_tui::app::model::*;
pub fn account(s: &str) -> AccountId {
    s.into()
}
pub fn key(chat: &str, sender: &str, id: &str) -> MessageKey {
    MessageKey {
        account: account("test"),
        chat: chat.into(),
        sender: sender.into(),
        id: id.into(),
        from_me: sender == "test",
    }
}
pub fn draft(text: &str, revision: u64) -> Draft {
    Draft {
        attachment: None,
        text: text.into(),
        revision,
        reply: None,
        ..Default::default()
    }
}
pub fn outbound(key: MessageKey, draft: Draft) -> OutboundText {
    OutboundText {
        key,
        draft,
        created_at_ms: 1_790_640_000_000,
    }
}
pub fn message(key: MessageKey, text: &str) -> MessageRecord {
    MessageRecord {
        key,
        body: MessageBody::Text(text.into()),
        quote: None,
        created_at_ms: 1_790_640_000_000,
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: None,
    }
}
pub fn batch(messages: Vec<MessageRecord>) -> MessageBatch {
    MessageBatch {
        account: account("test"),
        source: MessageSource::Live,
        changes: messages.into_iter().map(MessageChange::Upsert).collect(),
    }
}
pub fn ready_app() -> whatsapp_tui::app::App {
    use whatsapp_tui::{app::*, config::Config, whatsapp::BackendEvent};
    let mut app = App::new(Config::default());
    let now = tokio::time::Instant::now();
    let effects = app.update(
        Input::Backend(BackendEvent::AccountKnown(account("test"))),
        now,
    );
    let request = effects
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
        phone: Some("12345".into()),
        ..Default::default()
    };
    let effects = app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(vec![summary.clone()]),
        }),
        now,
    );
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                interactions: Default::default(),
                summary,
                messages: vec![message(key("chat", "alice", "one"), "Hello 👋")],
                draft: Draft::default(),
                receipts: vec![],
                has_older: false,
                has_newer: false,
            })),
        }),
        now,
    );
    app.update(
        Input::Backend(BackendEvent::ConnectionChanged {
            state: ConnectionState::Connected,
            reason: None,
        }),
        now,
    );
    app
}
pub fn press(app: &mut whatsapp_tui::app::App, key: &str) -> Vec<whatsapp_tui::app::Effect> {
    app.update(
        whatsapp_tui::app::Input::Terminal(crossterm::event::Event::Key(
            whatsapp_tui::config::bindings::parse_key(key).unwrap(),
        )),
        tokio::time::Instant::now(),
    )
}
