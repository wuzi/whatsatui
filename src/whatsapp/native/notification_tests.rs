use super::*;
use crate::notifications::{self, Request};
use whatsapp_rust::{types::events::MuteUpdate, waproto::whatsapp::sync_action_value::MuteAction};

#[test]
fn subscribes_to_chat_mute_updates() {
    let (bridge, _receiver) = bridge::bounded(1);
    assert!(RawHandler(bridge).interest().wants(EventKind::MuteUpdate));
}

#[test]
fn history_mutes_use_milliseconds_and_do_not_guess_when_absent() {
    let now = 1_790_880_000_000;
    for (until, expected) in [
        (Some(u64::MAX), Some(true)),
        (Some((now + 1) as u64), Some(true)),
        (Some(now as u64), Some(false)),
        (Some(0), Some(false)),
        (None, None),
    ] {
        let conversation = whatsapp_rust::waproto::whatsapp::Conversation {
            id: "group@g.us".into(),
            mute_end_time: until,
            ..Default::default()
        };
        let summary = history_chat(&"self".into(), &conversation);
        assert_eq!(
            summary.mute.map(|mute| mute.is_muted(now)),
            expected,
            "{until:?}"
        );
    }
}

#[tokio::test]
async fn protocol_mutes_and_unmutes_control_popups() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path().join("db")).await.unwrap();
    let a = AccountId("self@s.whatsapp.net".into());
    let now = 1_790_880_000_000;
    let message = MessageRecord {
        key: MessageKey {
            account: a.clone(),
            chat: "group@g.us".into(),
            sender: "friend@s.whatsapp.net".into(),
            id: "one".into(),
            from_me: false,
        },
        body: MessageBody::Text("Synthetic group message".into()),
        created_at_ms: now,
        edited_at_ms: None,
        expires_at_ms: None,
        send_state: None,
        quote: None,
    };
    store
        .apply_batch(MessageBatch {
            account: a.clone(),
            source: MessageSource::Live,
            changes: vec![MessageChange::Upsert(message.clone())],
        })
        .await
        .unwrap();
    let request = Request {
        keys: vec![message.key],
        overflow: None,
        previews: true,
    };
    let (tx, _rx) = mpsc::channel(32);
    for (index, (muted, expiry, notify)) in [
        (Some(true), Some(-1), false),
        (Some(false), Some(-1), true),
        (Some(true), Some(now + 1), false),
        (Some(true), Some(now), true),
        (Some(true), None, false),
        (Some(false), None, true),
        (None, Some(-1), true),
    ]
    .into_iter()
    .enumerate()
    {
        let event = Event::MuteUpdate(
            MuteUpdate::builder()
                .jid("group@g.us".parse().unwrap())
                .timestamp(chrono::DateTime::from_timestamp_millis(now + index as i64).unwrap())
                .action(Box::new(MuteAction {
                    muted,
                    mute_end_timestamp: expiry,
                    ..Default::default()
                }))
                .from_full_sync(index == 0)
                .build(),
        );
        handle_event(Some(a.clone()), &store, &tx, &event)
            .await
            .unwrap();
        assert_eq!(
            notifications::prepare(request.clone(), &store, now)
                .await
                .unwrap()
                .is_some(),
            notify,
            "muted={muted:?}, expiry={expiry:?}"
        );
    }
}
