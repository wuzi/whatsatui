use crate::{
    app::model::*,
    storage::{Store, StoreError},
};
#[cfg(test)]
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
    events: &tokio::sync::mpsc::Sender<super::BackendEvent>,
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
    let (mut change, incoming) = store
        .apply_batch_with_incoming(super::normalize::message_batch(
            account,
            MessageSource::Live,
            messages,
        ))
        .await?;
    aliases.extend(change.chats);
    change.chats = aliases.into_iter().collect();
    if !incoming.is_empty() {
        // Persistence is complete. A closed UI must not turn a successful
        // durable write into an ingestion failure or trigger an ACK retry.
        let _ = events
            .send(super::BackendEvent::IncomingMessages(incoming))
            .await;
    }
    Ok(change)
}

pub(super) struct DurableInbox(
    pub Store,
    pub tokio::sync::watch::Sender<Option<String>>,
    pub tokio::sync::mpsc::Sender<super::BackendEvent>,
);
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
            if let Err(error) = persist_inbound(&self.0, account.clone(), chunk, &self.2).await {
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
    fn normalized() -> MessageBatch {
        let mut b = message_batch(
            "self@s.whatsapp.net".into(),
            MessageSource::Live,
            &[fixture()],
        );
        if let MessageChange::Upsert(m) = &mut b.changes[0] {
            m.expires_at_ms = None;
        }
        b
    }
    #[tokio::test]
    async fn hook_and_protocol_replay_emit_one_canonical_committed_message() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(d.path().join("db")).await.unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let mut m = fixture();
        let info = std::sync::Arc::make_mut(&mut m.info);
        info.ephemeral_expiration = None;
        info.source.sender = "123@lid".parse().unwrap();
        info.source.sender_alt = Some("551100000001@s.whatsapp.net".parse().unwrap());
        for _ in 0..2 {
            persist_inbound(&s, "self@s.whatsapp.net".into(), &[m.clone()], &tx)
                .await
                .unwrap();
        }
        let event = rx
            .try_recv()
            .expect("first durable insertion must reach the app");
        let super::super::BackendEvent::IncomingMessages(incoming) = event else {
            panic!()
        };
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].key.sender.0, "551100000001@s.whatsapp.net");
        assert!(
            s.get_message(incoming[0].key.clone())
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            rx.try_recv().is_err(),
            "protocol replay must not alert again"
        );
    }
    #[tokio::test]
    async fn incoming_signal_reports_only_first_live_unread_insertions() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(d.path().join("db")).await.unwrap();
        let live = normalized();
        let (_, incoming) = s.apply_batch_with_incoming(live.clone()).await.unwrap();
        assert_eq!(incoming.len(), 1);
        assert!(
            s.apply_batch_with_incoming(live.clone())
                .await
                .unwrap()
                .1
                .is_empty()
        );
        let mut history = live.clone();
        history.source = MessageSource::History;
        if let MessageChange::Upsert(m) = &mut history.changes[0] {
            m.key.id = "history".into();
        }
        assert!(
            s.apply_batch_with_incoming(history.clone())
                .await
                .unwrap()
                .1
                .is_empty()
        );
        history.source = MessageSource::Live;
        assert!(
            s.apply_batch_with_incoming(history)
                .await
                .unwrap()
                .1
                .is_empty()
        );
        let mut own = live.clone();
        if let MessageChange::Upsert(m) = &mut own.changes[0] {
            m.key.id = "own".into();
            m.key.from_me = true;
        }
        assert!(s.apply_batch_with_incoming(own).await.unwrap().1.is_empty());
        let key = incoming[0].key.clone();
        let changes = vec![
            MessageChange::Edit {
                key: key.clone(),
                text: "edit".into(),
                edited_at_ms: 5,
            },
            MessageChange::Reaction(Reaction {
                key: key.clone(),
                reactor: "other".into(),
                emoji: "👍".into(),
                at_ms: 6,
                event_id: "react".into(),
            }),
            MessageChange::Delete { key: key.clone() },
        ];
        assert!(
            s.apply_batch_with_incoming(MessageBatch {
                changes,
                ..live.clone()
            })
            .await
            .unwrap()
            .1
            .is_empty()
        );
        assert!(
            s.apply_batch_with_incoming(live)
                .await
                .unwrap()
                .1
                .is_empty()
        );
    }
    #[tokio::test]
    async fn incoming_signal_observes_whole_transaction_and_read_watermark() {
        let d = tempfile::tempdir().unwrap();
        let s = Store::open(d.path().join("db")).await.unwrap();
        let mut b = normalized();
        let MessageChange::Upsert(m) = &b.changes[0] else {
            panic!()
        };
        let key = m.key.clone();
        b.changes.push(MessageChange::Delete { key: key.clone() });
        assert!(s.apply_batch_with_incoming(b).await.unwrap().1.is_empty());
        let mut b = normalized();
        if let MessageChange::Upsert(m) = &mut b.changes[0] {
            m.key.id = "watermark".into();
        }
        let incoming = s.apply_batch_with_incoming(b.clone()).await.unwrap().1;
        assert_eq!(incoming.len(), 1);
        s.mark_read(key.account, key.chat, vec![incoming[0].key.clone()])
            .await
            .unwrap();
        if let MessageChange::Upsert(m) = &mut b.changes[0] {
            m.key.id = "earlier".into();
            m.created_at_ms -= 1000;
        }
        assert!(s.apply_batch_with_incoming(b).await.unwrap().1.is_empty());
    }
    #[tokio::test]
    async fn rolled_back_incoming_insert_can_be_notified_after_retry() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("db");
        let s = Store::open(p.clone()).await.unwrap();
        let mut c = diesel::SqliteConnection::establish(p.to_str().unwrap()).unwrap();
        c.batch_execute("CREATE TRIGGER fail_inbound BEFORE INSERT ON messages BEGIN SELECT RAISE(ABORT,'disk failure'); END;").unwrap();
        assert!(s.apply_batch_with_incoming(normalized()).await.is_err());
        c.batch_execute("DROP TRIGGER fail_inbound").unwrap();
        assert_eq!(
            s.apply_batch_with_incoming(normalized())
                .await
                .unwrap()
                .1
                .len(),
            1
        );
    }
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
