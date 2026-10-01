use super::{Store, StoreError, merge, worker};
use crate::{
    app::model::*,
    notifications::{CAPACITY, Message},
};
use diesel::Connection;

impl Store {
    pub(crate) async fn notification_messages(
        &self,
        keys: Vec<MessageKey>,
        now_ms: i64,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |c| {
            c.transaction::<_, StoreError, _>(|c| {
                let mut messages = Vec::<Message>::new();
                for key in keys.into_iter().take(CAPACITY) {
                    let Some(record) = worker::get(c, &key)? else {
                        continue;
                    };
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
