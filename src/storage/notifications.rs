use super::{Store, StoreError, merge, worker};
use crate::{
    app::model::*,
    notifications::{CAPACITY, Message, Overflow},
};
use diesel::Connection;

impl Store {
    pub(crate) async fn notification_overflow(
        &self,
        overflow: Overflow,
        reading: Option<ChatId>,
        now_ms: i64,
    ) -> Result<bool, StoreError> {
        self.call(move |c| {
            // Keep only an account/time window for overflow. Query unread
            // storage independently of the first 128 candidates; no bodies
            // or unbounded list of omitted identities are retained in memory.
            let reading = reading.map(|chat| merge::canonical(c, &overflow.account, &chat.0)).transpose()?;
            let found: Vec<bool> = super::records::rows(c,
                "SELECT 'true' AS data FROM messages m LEFT JOIN chats c ON c.account=m.account AND c.chat=m.chat WHERE m.account=? AND m.unread=1 AND m.from_me=0 AND m.created_at_ms BETWEEN ? AND ? AND m.chat != ? AND (json_extract(m.data,'$.expires_at_ms') IS NULL OR json_extract(m.data,'$.expires_at_ms') > CAST(? AS INTEGER)) AND json_extract(m.data,'$.body') NOT IN ('Deleted','Expired') AND COALESCE(json_extract(c.data,'$.mute.until_ms'),0) != -1 AND COALESCE(json_extract(c.data,'$.mute.until_ms'),0) <= CAST(? AS INTEGER) LIMIT 1",
                &[&overflow.account.0, &overflow.since_ms.to_string(), &overflow.until_ms.to_string(), reading.as_deref().unwrap_or(""), &now_ms.to_string(), &now_ms.to_string()])?;
            Ok(!found.is_empty())
        }).await
    }
    pub(crate) async fn notification_messages(
        &self,
        keys: Vec<MessageKey>,
        now_ms: i64,
        reading: Option<ChatId>,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |c| {
            c.transaction::<_, StoreError, _>(|c| {
                let mut messages = Vec::<Message>::new();
                for key in keys.into_iter().take(CAPACITY) {
                    let Some(record) = worker::get(c, &key)? else {
                        continue;
                    };
                    if let Some(reading) = &reading
                        && merge::canonical(c, &key.account, &reading.0)? == record.key.chat.0
                    {
                        continue;
                    }
                    if !merge::unread(c, &record)?
                        || record.expires_at_ms.is_some_and(|at| at <= now_ms)
                        || messages.iter().any(|m| m.record.key == record.key)
                    {
                        continue;
                    }
                    let chat = worker::summary(c, &record.key.account, &record.key.chat)?;
                    if chat.mute.is_some_and(|mute| mute.is_muted(now_ms)) {
                        continue;
                    }
                    let chat_name = chat.name;
                    let sender_name = worker::summary(
                        c,
                        &record.key.account,
                        &ChatId(record.key.sender.0.clone()),
                    )?
                    .name;
                    messages.push(Message {
                        record,
                        chat_name,
                        sender_name,
                    });
                }
                Ok(messages)
            })
        })
        .await
    }
}
