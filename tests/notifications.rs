mod support;
use crossterm::event::Event;
use support::*;
use tokio::time::{Duration, Instant};
use whatsapp_tui::app::model::*;
use whatsapp_tui::{
    app::*,
    config::Config,
    notifications::{self, Request},
    storage::Store,
    whatsapp::BackendEvent,
};

fn incoming(chat: &str, id: &str) -> MessageRecord {
    let mut m = message(key(chat, "alice", id), "Hello 👋");
    m.created_at_ms = chrono::Utc::now().timestamp_millis();
    m
}
fn push(a: &mut App, messages: Vec<MessageRecord>, now: Instant) {
    a.update(
        Input::Backend(BackendEvent::IncomingMessages(messages)),
        now,
    );
}
fn pop(a: &mut App, now: Instant) -> Vec<Request> {
    a.update(Input::Tick(chrono::Utc::now().timestamp_millis()), now)
        .into_iter()
        .filter_map(|e| {
            if let Effect::Notify(n, _) = e {
                Some(n)
            } else {
                None
            }
        })
        .collect()
}
#[test]
fn foreground_suppresses_only_the_conversation_being_read() {
    for (background, focus_composer, other_chat, alerts) in [
        (false, true, false, 0),
        (true, true, false, 1),
        (false, true, true, 1),
        (false, false, false, 1),
    ] {
        let mut a = ready_app();
        if focus_composer {
            press(&mut a, "i");
        }
        let now = Instant::now();
        a.update(
            Input::Terminal(if background {
                Event::FocusLost
            } else {
                Event::FocusGained
            }),
            now,
        );
        push(
            &mut a,
            vec![incoming(if other_chat { "other" } else { "chat" }, "new")],
            now,
        );
        assert_eq!(
            pop(&mut a, now + Duration::from_secs(3)).len(),
            alerts,
            "background={background}, composer={focus_composer}, other={other_chat}"
        );
    }
}
#[test]
fn emitted_effect_observes_live_reducer_context() {
    let mut a = ready_app();
    let now = Instant::now();
    push(&mut a, vec![incoming("chat", "queued")], now);
    let effects = a.update(
        Input::Tick(chrono::Utc::now().timestamp_millis()),
        now + Duration::from_secs(3),
    );
    let mut context = effects
        .into_iter()
        .find_map(|e| {
            if let Effect::Notify(_, c) = e {
                Some(c)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(context.borrow_and_update().account, Some(account("test")));
    press(&mut a, "i");
    assert_eq!(context.borrow_and_update().reading, Some("chat".into()));
    a.update(
        Input::Backend(BackendEvent::AccountKnown("different".into())),
        now + Duration::from_secs(4),
    );
    assert_eq!(
        context.borrow_and_update().account,
        Some(account("different"))
    );
    a.request_shutdown();
    assert!(!context.borrow().enabled);
}
#[test]
fn unknown_focus_and_scrollback_alert_but_opening_pending_chat_cancels() {
    let now = Instant::now();
    let mut a = ready_app();
    push(&mut a, vec![incoming("chat", "unknown")], now);
    assert_eq!(pop(&mut a, now + Duration::from_secs(3)).len(), 1);
    let mut a = ready_app();
    press(&mut a, "i");
    a.update(Input::Terminal(Event::FocusLost), now);
    push(&mut a, vec![incoming("chat", "cancel")], now);
    a.update(
        Input::Terminal(Event::FocusGained),
        now + Duration::from_secs(1),
    );
    assert!(pop(&mut a, now + Duration::from_secs(3)).is_empty());
    let mut a = ready_app();
    press(&mut a, "tab");
    let view = a.view();
    a.update(
        Input::TimelineViewport(TimelineViewport {
            selected: view.selected_message,
            anchor: view.timeline_anchor,
            tail_rows: 20,
            fully_visible: vec![],
            max_scroll: 19,
            page_rows: 5,
        }),
        now,
    );
    press(&mut a, "K");
    assert!(!a.view().at_bottom);
    push(&mut a, vec![incoming("chat", "scroll")], now);
    assert_eq!(pop(&mut a, now + Duration::from_secs(3)).len(), 1);
}
#[test]
fn helper_failure_reports_once_and_keeps_a_bounded_retry_queue() {
    let now = Instant::now();
    let mut a = ready_app();
    push(&mut a, vec![incoming("chat", "first")], now);
    assert_eq!(pop(&mut a, now + Duration::from_secs(2)).len(), 1);
    a.update(
        Input::NotificationResult(Err("Notifications unavailable".into())),
        now + Duration::from_secs(2),
    );
    assert_eq!(
        a.view().notice.as_deref(),
        Some("Notifications unavailable")
    );
    push(
        &mut a,
        vec![incoming("other", "next")],
        now + Duration::from_secs(3),
    );
    assert!(pop(&mut a, now + Duration::from_secs(61)).is_empty());
    assert_eq!(pop(&mut a, now + Duration::from_secs(62)).len(), 1);
    a.update(
        Input::NotificationResult(Err("Repeated notification error".into())),
        now + Duration::from_secs(62),
    );
    assert_eq!(
        a.view().notice.as_deref(),
        Some("Notifications unavailable")
    );
}
#[test]
fn bursts_are_bounded_coalesced_and_do_not_reset_the_deadline() {
    let mut a = ready_app();
    let now = Instant::now();
    push(&mut a, vec![incoming("chat", "one")], now);
    assert!(pop(&mut a, now + Duration::from_millis(1900)).is_empty());
    push(
        &mut a,
        (0..400)
            .map(|i| incoming("other", &format!("burst-{i}")))
            .collect(),
        now + Duration::from_millis(1950),
    );
    let n = pop(&mut a, now + Duration::from_secs(2));
    assert_eq!(n.len(), 1);
    assert!(n[0].keys.len() <= 128);
    assert!(n[0].overflow.is_some());
    assert!(pop(&mut a, now + Duration::from_secs(4)).is_empty());
}
#[test]
fn overflow_survives_opening_the_first_128_messages_chat() {
    let now = Instant::now();
    let mut a = ready_app();
    let mut messages = (0..128)
        .map(|i| incoming("chat", &format!("a-{i}")))
        .collect::<Vec<_>>();
    messages.push(incoming("other", "unseen"));
    push(&mut a, messages, now);
    press(&mut a, "i");
    assert_eq!(
        pop(&mut a, now + Duration::from_secs(3)).len(),
        1,
        "the untracked chat still needs its generic alert"
    );
}
#[tokio::test]
async fn overflow_survives_deleted_tracked_candidates() {
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path().join("db")).await.unwrap();
    let now = Instant::now();
    let mut a = ready_app();
    let mut messages = (0..128)
        .map(|i| incoming("chat", &format!("a-{i}")))
        .collect::<Vec<_>>();
    let deleted = messages
        .iter()
        .map(|m| MessageChange::Delete { key: m.key.clone() })
        .collect();
    messages.push(incoming("other", "unseen"));
    store.apply_batch(batch(messages.clone())).await.unwrap();
    push(&mut a, messages, now);
    store
        .apply_batch(MessageBatch {
            account: account("test"),
            source: MessageSource::Live,
            changes: deleted,
        })
        .await
        .unwrap();
    let request = pop(&mut a, now + Duration::from_secs(3)).pop().unwrap();
    assert!(
        notifications::prepare(request, &store, chrono::Utc::now().timestamp_millis())
            .await
            .unwrap()
            .is_some(),
        "overflow must be revalidated independently of deleted tracked records"
    );
}
#[test]
fn old_own_inactive_account_and_quit_do_not_alert() {
    let now = Instant::now();
    let mut a = ready_app();
    let mut old = incoming("chat", "old");
    old.created_at_ms -= 60_000;
    let mut own = incoming("chat", "own");
    own.key.from_me = true;
    let mut foreign = incoming("chat", "foreign");
    foreign.key.account = "different".into();
    push(&mut a, vec![old, own, foreign], now);
    assert!(pop(&mut a, now + Duration::from_secs(3)).is_empty());
    push(
        &mut a,
        vec![incoming("chat", "new")],
        now + Duration::from_secs(4),
    );
    a.update(
        Input::Backend(BackendEvent::AccountKnown("different".into())),
        now + Duration::from_secs(5),
    );
    assert!(pop(&mut a, now + Duration::from_secs(7)).is_empty());
    let mut a = ready_app();
    push(&mut a, vec![incoming("chat", "quit")], now);
    a.request_shutdown();
    assert!(pop(&mut a, now + Duration::from_secs(3)).is_empty());
}
#[test]
fn notification_configuration_controls_delivery_and_privacy() {
    let now = Instant::now();
    let mut a = ready_app();
    a.config = Config::parse("[notifications]\nenabled=false").expect("notification configuration");
    push(&mut a, vec![incoming("chat", "disabled")], now);
    assert!(pop(&mut a, now + Duration::from_secs(3)).is_empty());
    let mut a = ready_app();
    a.config = Config::parse("[notifications]\npreviews=false").unwrap();
    push(&mut a, vec![incoming("chat", "private")], now);
    assert!(!pop(&mut a, now + Duration::from_secs(3))[0].previews);
}
#[tokio::test]
async fn popup_uses_current_group_sender_content_and_hides_private_previews() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let m = incoming("friends@g.us", "one");
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    s.upsert_chats(
        account("test"),
        vec![
            ChatSummary {
                account: account("test"),
                chat: "friends@g.us".into(),
                name: "Friends".into(),
                is_group: true,
                ..Default::default()
            },
            ChatSummary {
                account: account("test"),
                chat: "alice".into(),
                name: "Alice".into(),
                ..Default::default()
            },
        ],
    )
    .await
    .unwrap();
    let request = Request {
        keys: vec![m.key],
        overflow: None,
        previews: true,
    };
    let n = notifications::prepare(request.clone(), &s, m.created_at_ms)
        .await
        .unwrap()
        .expect("popup");
    assert_eq!(n.title, "Friends");
    assert_eq!(n.body, "Alice: Hello 👋");
    let n = notifications::prepare(
        Request {
            previews: false,
            ..request
        },
        &s,
        m.created_at_ms,
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!format!("{} {}", n.title, n.body).contains("Alice"));
    assert!(!format!("{} {}", n.title, n.body).contains("Friends"));
    assert!(!n.body.contains("Hello"));
}

#[tokio::test]
async fn media_and_multi_chat_bursts_have_useful_compact_summaries() {
    use whatsapp_tui::media::{Attachment, AttachmentKind, AudioMetadata};
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let mut voice = incoming("voicechat", "voice");
    voice.body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Audio,
        audio: Some(AudioMetadata {
            seconds: Some(10),
            voice: true,
        }),
        filename: None,
        mime: Some("audio/ogg".into()),
        caption: None,
        size: 10,
        direct_path: "/v/test".into(),
        media_key: [1; 32],
        sha256: [2; 32],
        encrypted_sha256: [3; 32],
    }));
    s.apply_batch(batch(vec![voice.clone()])).await.unwrap();
    let request = Request {
        keys: vec![voice.key.clone()],
        previews: true,
        overflow: None,
    };
    let popup = notifications::prepare(request.clone(), &s, voice.created_at_ms)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(popup.body, "[voice message]");
    let other = incoming("other", "text");
    s.apply_batch(batch(vec![other.clone()])).await.unwrap();
    let popup = notifications::prepare(
        Request {
            keys: vec![voice.key, other.key],
            ..request
        },
        &s,
        voice.created_at_ms,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(popup.title, "2 new messages in 2 chats");
    assert!(popup.body.contains("[voice message]"));
    assert!(popup.body.contains("Hello 👋"));
}
#[tokio::test]
async fn dispatch_revalidates_deleted_expired_read_and_aliased_messages() {
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path().join("db")).await.unwrap();
    let mut expired = incoming("chat", "expires");
    expired.expires_at_ms = Some(expired.created_at_ms + 10);
    let deleted = incoming("chat", "deleted");
    let read = incoming("readchat", "read");
    s.apply_batch(batch(vec![expired.clone(), deleted.clone(), read.clone()]))
        .await
        .unwrap();
    s.apply_batch(MessageBatch {
        account: account("test"),
        source: MessageSource::Live,
        changes: vec![MessageChange::Delete {
            key: deleted.key.clone(),
        }],
    })
    .await
    .unwrap();
    s.mark_read(
        account("test"),
        read.key.chat.clone(),
        vec![read.key.clone()],
    )
    .await
    .unwrap();
    let request = Request {
        keys: vec![expired.key, deleted.key, read.key],
        previews: true,
        overflow: None,
    };
    assert!(
        notifications::prepare(request, &s, expired.created_at_ms + 100)
            .await
            .unwrap()
            .is_none()
    );
    let m = incoming("123@lid", "alias");
    s.apply_batch(batch(vec![m.clone()])).await.unwrap();
    s.merge_alias(
        account("test"),
        "123@lid".into(),
        "5511@s.whatsapp.net".into(),
    )
    .await
    .unwrap();
    let n = notifications::prepare(
        Request {
            keys: vec![m.key],
            previews: true,
            overflow: None,
        },
        &s,
        m.created_at_ms,
    )
    .await
    .unwrap();
    assert!(n.is_some(), "canonicalized messages must still notify");
}
