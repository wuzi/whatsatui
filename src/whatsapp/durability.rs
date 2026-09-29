use crate::{
    app::model::*,
    storage::{Store, StoreError},
};
pub(super) async fn persist_normalized(
    store: &Store,
    batch: MessageBatch,
) -> Result<StoreChange, StoreError> {
    store.apply_batch(batch).await
}

pub(super) async fn persist_inbound(
    store: &Store,
    account: AccountId,
    messages: &[whatsapp_rust::types::events::InboundMessage],
) -> Result<StoreChange, StoreError> {
    let mut aliases = std::collections::BTreeSet::new();
    for (alias, canonical) in super::normalize::identity_aliases(messages) {
        aliases.insert(ChatId(alias.0.clone()));
        aliases.insert(ChatId(canonical.0.clone()));
        aliases.extend(
            store
                .merge_alias(account.clone(), alias, canonical)
                .await?
                .chats,
        );
    }
    let mut change = persist_normalized(
        store,
        super::normalize::message_batch(account, MessageSource::Live, messages),
    )
    .await?;
    aliases.extend(change.chats);
    change.chats = aliases.into_iter().collect();
    Ok(change)
}

pub(super) struct DurableInbox(pub Store, pub tokio::sync::watch::Sender<Option<String>>);
#[async_trait::async_trait]
impl whatsapp_rust::InboundDurabilityHook for DurableInbox {
    async fn on_messages(
        &self,
        client: std::sync::Arc<whatsapp_rust::Client>,
        messages: &[whatsapp_rust::types::events::InboundMessage],
    ) -> anyhow::Result<()> {
        let account = super::native::account(&client)
            .ok_or_else(|| anyhow::anyhow!("Account unavailable for durable ingestion"))?;
        for chunk in messages.chunks(100) {
            if let Err(error) = persist_inbound(&self.0, account.clone(), chunk).await {
                self.1.send_replace(Some(
                    "Incoming messages cannot be saved; check free disk space and file permissions"
                        .into(),
                ));
                return Err(error.into());
            }
        }
        self.1.send_replace(None);
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::whatsapp::normalize::{message_batch, tests::fixture};
    use diesel::{Connection, connection::SimpleConnection};
    #[tokio::test]
    async fn durability_failure_returns_error() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("db");
        let s = Store::open(p.clone()).await.unwrap();
        let mut c = diesel::SqliteConnection::establish(p.to_str().unwrap()).unwrap();
        c.batch_execute("CREATE TRIGGER fail_inbound BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'disk failure'); END;").unwrap();
        let b = message_batch(
            "self@s.whatsapp.net".into(),
            MessageSource::Live,
            &[fixture()],
        );
        assert!(persist_normalized(&s, b).await.is_err());
    }
    #[tokio::test]
    async fn replayed_batch_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(d.path().join("db")).await.unwrap();
        for _ in 0..2 {
            persist_normalized(
                &s,
                message_batch(
                    "self@s.whatsapp.net".into(),
                    MessageSource::Live,
                    &[fixture()],
                ),
            )
            .await
            .unwrap();
        }
        let snap = s
            .snapshot(
                "self@s.whatsapp.net".into(),
                "120363000000001@g.us".into(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(snap.messages.len(), 1);
        assert_eq!(snap.summary.unread, 1);
    }
}
