mod support;
use crossterm::event::Event;
use ratatui::{Terminal, backend::TestBackend};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    message_actions::DesktopAction,
    ui,
    whatsapp::BackendEvent,
};

fn refresh(app: &mut App, messages: Vec<MessageRecord>) {
    let view = app.view();
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["chat".into()],
        })),
        Instant::now(),
    );
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChat { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chat {
            request,
            account: account("test"),
            chat: "chat".into(),
            cursor: None,
            result: Ok(Box::new(ChatSnapshot {
                interactions: Default::default(),
                summary: view.chats[0].clone(),
                messages,
                draft: view.draft,
                receipts: vec![],
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
}
fn with_body(body: MessageBody) -> App {
    let mut app = ready_app();
    let mut m = app.view().messages[0].clone();
    m.body = body;
    refresh(&mut app, vec![m]);
    press(&mut app, "tab");
    app
}
fn text_app(text: &str) -> App {
    with_body(MessageBody::Text(text.into()))
}
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::render(f, &app.view(), &app.config)).unwrap();
    t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn desktop(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::DesktopAction { .. }))
        .expect("desktop action")
}
fn completed(effect: &Effect, result: Result<String, String>) -> Input {
    let Effect::DesktopAction {
        request, message, ..
    } = effect
    else {
        panic!()
    };
    Input::DesktopAction {
        request: *request,
        account: message.key.account.clone(),
        result,
    }
}

#[test]
fn menu_and_cancel_preserve_writing_and_reading_position() {
    let mut app = ready_app();
    press(&mut app, "enter");
    app.update(
        Input::Terminal(Event::Paste("unsent café".into())),
        Instant::now(),
    );
    press(&mut app, "esc");
    let before = app.view();
    assert!(press(&mut app, "enter").is_empty());
    let text = screen(&app, 120, 40);
    for label in ["Message actions", "Copy text", "Reply"] {
        assert!(text.contains(label), "{label}");
    }
    assert!(!text.contains("Resend message"));
    press(&mut app, "esc");
    assert!(app.view().overlay.is_none());
    assert_eq!(app.view().draft, before.draft);
    assert_eq!(app.view().focus, before.focus);
    assert_eq!(app.view().selected_message, before.selected_message);
    assert_eq!(app.view().message_scroll, before.message_scroll);
}
#[test]
fn copy_is_explicit_bounded_and_retries_after_failure() {
    let mut app = text_app("*original*\n👩‍💻");
    let first = desktop(press(&mut app, "y"));
    let Effect::DesktopAction {
        message, action, ..
    } = &first
    else {
        panic!()
    };
    assert_eq!(message.body, MessageBody::Text("*original*\n👩‍💻".into()));
    assert_eq!(*action, DesktopAction::CopyText);
    assert!(
        !press(&mut app, "y")
            .iter()
            .any(|e| matches!(e, Effect::DesktopAction { .. }))
    );
    app.update(
        completed(&first, Err("Clipboard unavailable".into())),
        Instant::now(),
    );
    assert!(app.view().notice.unwrap().contains("Clipboard unavailable"));
    let retry = desktop(press(&mut app, "y"));
    app.update(
        completed(&retry, Ok("Copied message text".into())),
        Instant::now(),
    );
    assert!(app.view().notice.unwrap().contains("Copied"));
}
#[test]
fn links_require_selection_and_cancel_returns_to_the_menu() {
    let mut app = text_app("https://example.org/one https://example.org/two file:///tmp/no");
    assert!(press(&mut app, "o").is_empty());
    assert!(screen(&app, 120, 40).contains("Message links"));
    press(&mut app, "j");
    let effect = desktop(press(&mut app, "enter"));
    assert!(
        matches!(effect,Effect::DesktopAction {action:DesktopAction::OpenLink(ref url),..} if url=="https://example.org/two")
    );
    app.update(
        completed(&effect, Ok("Browser request sent".into())),
        Instant::now(),
    );
    press(&mut app, "enter");
    press(&mut app, "down");
    press(&mut app, "enter");
    assert!(screen(&app, 120, 40).contains("Message links"));
    press(&mut app, "esc");
    assert!(screen(&app, 120, 40).contains("Message actions"));
    press(&mut app, "esc");
    press(&mut app, "o");
    press(&mut app, "esc");
    assert!(app.view().overlay.is_none());
}
#[test]
fn picker_can_copy_the_selected_url() {
    let mut app = text_app("https://example.org/one https://example.org/two");
    press(&mut app, "o");
    press(&mut app, "down");
    assert!(
        matches!(desktop(press(&mut app,"y")),Effect::DesktopAction {action:DesktopAction::CopyLink(url),..} if url=="https://example.org/two")
    );
}
#[test]
fn menu_reply_preserves_draft_and_resend_still_requires_confirmation() {
    let mut app = text_app("Hello");
    press(&mut app, "tab");
    app.update(
        Input::Terminal(Event::Paste("keep me".into())),
        Instant::now(),
    );
    press(&mut app, "esc");
    press(&mut app, "enter");
    press(&mut app, "down");
    press(&mut app, "enter");
    assert_eq!(app.view().focus, Focus::Composer);
    assert_eq!(app.view().draft.text, "keep me");
    assert_eq!(app.view().draft.reply.unwrap().key.id.0, "one");
    let mut failed = message(key("chat", "test", "failed"), "try again");
    failed.send_state = Some(SendState::Unconfirmed);
    refresh(&mut app, vec![failed]);
    press(&mut app, "esc");
    press(&mut app, "enter");
    assert!(screen(&app, 120, 40).contains("Resend message"));
    let effects = press(&mut app, "R");
    assert!(matches!(app.view().overlay, Some(Overlay::Resend { .. })));
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Prepare { .. } | Effect::Transmit(_)))
    );
}
#[test]
fn incoming_messages_do_not_retarget_an_open_menu() {
    let mut app = text_app("chosen");
    let chosen = app.view().messages[0].clone();
    press(&mut app, "enter");
    let mut incoming = message(key("chat", "alice", "new"), "arrival");
    incoming.created_at_ms += 1000;
    refresh(&mut app, vec![chosen, incoming]);
    let effect = desktop(press(&mut app, "y"));
    assert!(matches!(effect,Effect::DesktopAction {message,..} if message.key.id.0=="one"));
}
#[test]
fn changed_deleted_and_expired_messages_cannot_keep_action_menus() {
    for body in [
        MessageBody::Text("edited".into()),
        MessageBody::Deleted,
        MessageBody::Expired,
    ] {
        let mut app = text_app("old https://example.org/");
        press(&mut app, "o");
        let mut changed = app.view().messages[0].clone();
        changed.body = body;
        refresh(&mut app, vec![changed]);
        assert!(app.view().overlay.is_none());
    }
    let mut app = text_app("expires");
    let mut m = app.view().messages[0].clone();
    m.expires_at_ms = Some(1);
    refresh(&mut app, vec![m]);
    assert!(press(&mut app, "enter").is_empty());
    assert!(app.view().overlay.is_none());
    assert!(
        !press(&mut app, "y")
            .iter()
            .any(|e| matches!(e, Effect::DesktopAction { .. }))
    );
}
#[test]
fn old_account_completion_cannot_replace_current_notice() {
    let mut app = text_app("old account");
    let effect = desktop(press(&mut app, "y"));
    app.update(
        Input::Backend(BackendEvent::AccountKnown(account("different"))),
        Instant::now(),
    );
    let notice = app.view().notice;
    app.update(completed(&effect, Ok("old success".into())), Instant::now());
    assert_eq!(app.view().notice, notice);
}
#[test]
fn menus_render_remapped_controls_at_narrow_and_wide_sizes() {
    let mut app = text_app("https://example.org/");
    app.config=Config::parse("[bindings.messages]\nmessage_actions=['ctrl-a']\n[bindings.message_actions]\nopen=['ctrl-e']\nback=['ctrl-b']\n[bindings.message_links]\nopen=['ctrl-e']\nback=['ctrl-b']\ncopy_text=['ctrl-y']").unwrap();
    press(&mut app, "ctrl-a");
    for (w, h) in [(40, 12), (120, 40)] {
        let text = screen(&app, w, h);
        for label in ["Copy text", "ctrl-e", "ctrl-b"] {
            assert!(text.contains(label), "{label} at {w}x{h}");
        }
    }
    press(&mut app, "down");
    press(&mut app, "ctrl-e");
    for (w, h) in [(40, 12), (120, 40)] {
        let text = screen(&app, w, h);
        for label in ["https://example.org/", "ctrl-e", "ctrl-b", "ctrl-y"] {
            assert!(text.contains(label), "{label} at {w}x{h}");
        }
    }
    press(&mut app, "ctrl-b");
    press(&mut app, "ctrl-b");
    assert!(app.view().overlay.is_none());
}
#[test]
fn captions_offer_copy_and_printable_keys_stay_text_in_the_composer() {
    let mut app = with_body(MessageBody::Unsupported {
        kind: "image".into(),
        caption: Some("*caption*".into()),
    });
    press(&mut app, "enter");
    let text = screen(&app, 120, 40);
    assert!(text.contains("Copy text"));
    assert!(!text.contains("Reply to message"));
    press(&mut app, "esc");
    press(&mut app, "tab");
    for key in ["y", "o", "a", "r", "R"] {
        press(&mut app, key);
    }
    assert_eq!(app.view().draft.text, "yoarR");
}
