mod support;
use async_trait::async_trait;
use support::*;
use whatsapp_tui::{
    app::model::*,
    storage::Store,
    whatsapp::interactions::{self, Transport},
};

fn attempt(message: MessageRecord, kind: MutationKind, id: &str) -> MutationAttempt {
    MutationAttempt {
        id: id.into(),
        target: message,
        kind,
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        state: MutationState::Pending,
    }
}
async fn setup() -> (tempfile::TempDir, Store, MessageRecord) {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let mut m = message(key("chat", "test", "original"), "before");
    m.created_at_ms = chrono::Utc::now().timestamp_millis() - 1000;
    m.send_state = Some(SendState::Read);
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    s.save_draft(account("test"), "chat".into(), draft("keep my draft", 4))
        .await
        .unwrap();
    (d, s, m)
}
struct CheckDurable {
    store: Store,
    outcome: MutationState,
}
#[async_trait]
impl Transport for CheckDurable {
    async fn send(&self, attempt: &MutationAttempt) -> MutationState {
        let snap = self
            .store
            .snapshot(
                attempt.target.key.account.clone(),
                attempt.target.key.chat.clone(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(snap.interactions.mutations[0].id, attempt.id);
        assert_eq!(snap.interactions.mutations[0].state, MutationState::Pending);
        assert_eq!(snap.messages[0].body, MessageBody::Text("before".into()));
        self.outcome
    }
}
#[tokio::test]
async fn accepted_edit_is_durable_and_keeps_quote_receipt_and_normal_draft() {
    let (_d, s, m) = setup().await;
    let t = CheckDurable {
        store: s.clone(),
        outcome: MutationState::Sent,
    };
    assert_eq!(
        interactions::execute(
            &s,
            &t,
            attempt(
                m.clone(),
                MutationKind::Edit {
                    text: "after".into()
                },
                "edit-1"
            )
        )
        .await
        .unwrap(),
        MutationState::Sent
    );
    let snap = s
        .snapshot(account("test"), "chat".into(), None)
        .await
        .unwrap();
    assert_eq!(snap.messages.len(), 1);
    assert_eq!(snap.messages[0].key, m.key);
    assert_eq!(snap.messages[0].body, MessageBody::Text("after".into()));
    assert_eq!(snap.messages[0].send_state, Some(SendState::Read));
    assert!(snap.messages[0].edited_at_ms.is_some());
    assert_eq!(snap.draft, draft("keep my draft", 4));
    assert_eq!(snap.interactions.mutations[0].state, MutationState::Sent);
}
#[tokio::test]
async fn failed_and_uncertain_operations_preserve_content_and_recovery_never_replays() {
    for outcome in [MutationState::Failed, MutationState::Unconfirmed] {
        let (_d, s, m) = setup().await;
        let t = CheckDurable {
            store: s.clone(),
            outcome,
        };
        interactions::execute(
            &s,
            &t,
            attempt(
                m.clone(),
                MutationKind::Edit {
                    text: "after".into(),
                },
                "edit-1",
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            s.get_message(m.key.clone()).await.unwrap().unwrap().body,
            m.body
        );
        let a = attempt(
            m.clone(),
            MutationKind::Reaction {
                emoji: "👍".into()
            },
            "pending-2",
        );
        s.stage_mutation(a.clone(), a.created_at_ms).await.unwrap();
        assert!(
            s.stage_mutation(
                attempt(
                    m.clone(),
                    MutationKind::Edit {
                        text: "busy".into()
                    },
                    "edit-3"
                ),
                a.created_at_ms
            )
            .await
            .is_err()
        );
        s.recover_sends(account("test")).await.unwrap();
        let snap = s
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap();
        assert_eq!(
            snap.interactions.mutations[0].state,
            MutationState::Unconfirmed
        );
        assert!(snap.interactions.reactions.is_empty());
    }
}
#[tokio::test]
async fn stage_checks_ownership_age_expiry_and_captured_version() {
    let (_d, s, m) = setup().await;
    let now = m.created_at_ms + 900_000;
    assert!(
        s.stage_mutation(
            attempt(
                m.clone(),
                MutationKind::Edit {
                    text: "too late".into()
                },
                "late"
            ),
            now
        )
        .await
        .is_err()
    );
    let mut stale = m.clone();
    stale.body = MessageBody::Text("stale snapshot".into());
    assert!(
        s.stage_mutation(
            attempt(stale, MutationKind::Edit { text: "no".into() }, "stale"),
            now - 1
        )
        .await
        .is_err()
    );
    let mut other = message(key("chat", "alice", "incoming"), "hello");
    other.created_at_ms = m.created_at_ms;
    s.apply_batch(batch(vec![other.clone()])).await.unwrap();
    assert!(
        s.stage_mutation(
            attempt(
                other.clone(),
                MutationKind::Edit { text: "no".into() },
                "other"
            ),
            now - 1
        )
        .await
        .is_err()
    );
    other.expires_at_ms = Some(now - 1);
    s.apply_batch(batch(vec![other.clone()])).await.unwrap();
    assert!(
        s.stage_mutation(
            attempt(
                other,
                MutationKind::Reaction {
                    emoji: "👍".into()
                },
                "expired"
            ),
            now
        )
        .await
        .is_err()
    );
    s.stage_mutation(
        attempt(
            m,
            MutationKind::Edit {
                text: "last second".into(),
            },
            "valid",
        ),
        now - 1,
    )
    .await
    .unwrap();
}
#[tokio::test]
async fn accepted_reactions_replace_and_remove_own_entry() {
    let (_d, s, m) = setup().await;
    let t = CheckDurable {
        store: s.clone(),
        outcome: MutationState::Sent,
    };
    for (id, emoji, count) in [("r1", "👍", 1), ("r2", "❤️", 1), ("r3", "", 0)] {
        let mut a = attempt(
            m.clone(),
            MutationKind::Reaction {
                emoji: emoji.into(),
            },
            id,
        );
        a.created_at_ms += match id {
            "r2" => 1,
            "r3" => 2,
            _ => 0,
        };
        interactions::execute(&s, &t, a).await.unwrap();
        let snap = s
            .snapshot(account("test"), "chat".into(), None)
            .await
            .unwrap();
        assert_eq!(snap.interactions.reactions.len(), count);
        if count > 0 {
            assert_eq!(snap.interactions.reactions[0].emoji, emoji);
            assert_eq!(snap.interactions.reactions[0].reactor.0, "test");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn stalled_transport_becomes_unconfirmed_without_applying_edit() {
    struct Stalled(tokio::sync::Notify);
    #[async_trait]
    impl Transport for Stalled {
        async fn send(&self, _: &MutationAttempt) -> MutationState {
            self.0.notify_one();
            std::future::pending().await
        }
    }
    let (_d, s, m) = setup().await;
    let stalled = std::sync::Arc::new(Stalled(tokio::sync::Notify::new()));
    let run = tokio::spawn({
        let s = s.clone();
        let t = stalled.clone();
        let a = attempt(
            m.clone(),
            MutationKind::Edit {
                text: "after".into(),
            },
            "timeout",
        );
        async move { interactions::execute(&s, t.as_ref(), a).await }
    });
    stalled.0.notified().await;
    tokio::time::advance(std::time::Duration::from_secs(31)).await;
    assert_eq!(run.await.unwrap().unwrap(), MutationState::Unconfirmed);
    assert_eq!(
        s.get_message(m.key).await.unwrap().unwrap().body,
        MessageBody::Text("before".into())
    );
}
