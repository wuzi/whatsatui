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
        text: text.into(),
        revision,
        reply: None,
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
