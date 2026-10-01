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
                "SELECT 'true' AS data FROM messages WHERE account=? AND unread=1 AND from_me=0 AND created_at_ms BETWEEN ? AND ? AND chat != ? AND (json_extract(data,'$.expires_at_ms') IS NULL OR json_extract(data,'$.expires_at_ms') > CAST(? AS INTEGER)) AND json_extract(data,'$.body') NOT IN ('Deleted','Expired') LIMIT 1",
                &[&overflow.account.0, &overflow.since_ms.to_string(), &overflow.until_ms.to_string(), reading.as_deref().unwrap_or(""), &now_ms.to_string()])?;
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
                    let chat_name = worker::summary(c, &record.key.account, &record.key.chat)?.name;
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
