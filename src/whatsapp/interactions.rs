//! Durable message mutations. A successful transport result must mean server acknowledgement.
use crate::{app::model::*, storage::Store};
use std::{sync::Arc, time::Duration};
use whatsapp_rust::{
    Client,
    prelude::{Event, EventHandler, EventInterest, EventKind, MessageBuilderExt, wa},
};

#[async_trait::async_trait]
pub trait Transport: Send + Sync {
    async fn send(&self, attempt: &MutationAttempt) -> MutationState;
}
pub async fn execute(
    store: &Store,
    transport: &dyn Transport,
    attempt: MutationAttempt,
) -> Result<MutationState, String> {
    let attempt = store
        .stage_mutation(attempt, chrono::Utc::now().timestamp_millis())
        .await
        .map_err(|e| e.to_string())?;
    let state = tokio::time::timeout(Duration::from_secs(30), transport.send(&attempt))
        .await
        .unwrap_or(MutationState::Unconfirmed);
    let state = if state == MutationState::Pending {
        MutationState::Unconfirmed
    } else {
        state
    };
    store
        .finish_mutation(attempt.target.key.account, attempt.id, state)
        .await
        .map_err(|e| e.to_string())?;
    Ok(state)
}
pub(super) struct Native<'a>(pub &'a Client);
struct Acks(tokio::sync::mpsc::Sender<(String, bool)>);
impl EventHandler for Acks {
    fn handle_event(&self, event: Arc<Event>) {
        if let Event::ServerAck(ack) = event.as_ref()
            && ack.class.as_deref() == Some("message")
        {
            // Overflow conservatively times out. Never block the protocol event bus.
            let _ = self.0.try_send((ack.id.clone(), ack.error.is_none()));
        }
    }
    fn interest(&self) -> EventInterest {
        EventInterest::of(&[EventKind::ServerAck])
    }
}
async fn acknowledgement(
    rx: &mut tokio::sync::mpsc::Receiver<(String, bool)>,
    id: &str,
) -> MutationState {
    while let Some((received, accepted)) = rx.recv().await {
        if received == id {
            return if accepted {
                MutationState::Sent
            } else {
                MutationState::Failed
            };
        }
    }
    MutationState::Unconfirmed
}
pub fn target_key(
    key: &MessageKey,
) -> Result<(whatsapp_rust::Jid, wa::MessageKey), super::BackendError> {
    use whatsapp_rust::wacore_binary::JidExt;
    let to: whatsapp_rust::Jid = key
        .chat
        .0
        .parse()
        .map_err(|_| super::BackendError::InvalidIdentity)?;
    let sender: whatsapp_rust::Jid = key
        .sender
        .0
        .parse()
        .map_err(|_| super::BackendError::InvalidIdentity)?;
    if key.id.0.is_empty() || to.is_status_broadcast() || to.is_newsletter() {
        return Err(super::BackendError::InvalidIdentity);
    }
    let wire = wa::MessageKey {
        remote_jid: Some(to.to_string()),
        from_me: Some(key.from_me),
        id: Some(key.id.0.clone()),
        participant: to.is_group().then(|| sender.to_string()),
    };
    Ok((to, wire))
}
#[async_trait::async_trait]
impl Transport for Native<'_> {
    async fn send(&self, attempt: &MutationAttempt) -> MutationState {
        if super::native::account(self.0).as_ref() != Some(&attempt.target.key.account) {
            return MutationState::Failed;
        }
        let Ok((to, key)) = target_key(&attempt.target.key) else {
            return MutationState::Failed;
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(64);
        let handler = Arc::new(Acks(tx));
        let _subscription = self.0.subscribe_handler(handler);
        let sent = match &attempt.kind {
            MutationKind::Reaction { emoji } => self
                .0
                .send_reaction(to, key, emoji)
                .await
                .map(|sent| sent.message_id),
            MutationKind::Edit { text } => self
                .0
                .edit_message_with_options(
                    to,
                    attempt.target.key.id.0.clone(),
                    wa::Message::text(text),
                    whatsapp_rust::send::EditOptions::default().with_stanza_id(&attempt.id),
                )
                .await
                .map(|_| attempt.id.clone()),
        };
        match sent {
            Ok(id) => acknowledgement(&mut rx, &id).await,
            Err(error) => match super::encode::classify_send_error(&error) {
                SendState::Failed => MutationState::Failed,
                _ => MutationState::Unconfirmed,
            },
        }
    }
}
pub(super) struct Demo;
#[async_trait::async_trait]
impl Transport for Demo {
    async fn send(&self, _: &MutationAttempt) -> MutationState {
        MutationState::Sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn acknowledgements_ignore_other_stanzas_and_distinguish_rejection() {
        for (accepted, expected) in [(true, MutationState::Sent), (false, MutationState::Failed)] {
            let (tx, mut rx) = tokio::sync::mpsc::channel(4);
            tx.send(("unrelated".into(), true)).await.unwrap();
            tx.send(("mutation".into(), accepted)).await.unwrap();
            assert_eq!(acknowledgement(&mut rx, "mutation").await, expected);
        }
    }
    #[test]
    fn reaction_wire_key_preserves_group_author_and_target_ownership() {
        let key = MessageKey {
            account: "111@s.whatsapp.net".into(),
            chat: "123@g.us".into(),
            sender: "222@s.whatsapp.net".into(),
            id: "original".into(),
            from_me: false,
        };
        let (to, wire) = target_key(&key).unwrap();
        assert_eq!(to.to_string(), "123@g.us");
        assert_eq!(wire.participant.as_deref(), Some("222@s.whatsapp.net"));
        assert_eq!(wire.id.as_deref(), Some("original"));
        assert_eq!(wire.from_me, Some(false));
    }
}
