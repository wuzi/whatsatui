mod support;
use crossterm::event::{Event, KeyEventKind};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    media::{Attachment, AttachmentKind, MediaAction},
    ui,
    whatsapp::BackendEvent,
};

fn app() -> App {
    let mut app = ready_app();
    let mut view = app.view();
    view.messages[0].body = MessageBody::Media(Box::new(Attachment {
        kind: AttachmentKind::Document,
        filename: Some("report.pdf".into()),
        mime: Some("application/pdf".into()),
        caption: None,
        size: 6,
        direct_path: "/v/doc".into(),
        media_key: [1; 32],
        sha256: [2; 32],
        encrypted_sha256: [3; 32],
    }));
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
                summary: view.chats[0].clone(),
                messages: view.messages,
                draft: view.draft,
                receipts: vec![],
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
    press(&mut app, "tab");
    app
}
fn screen(app: &App, width: u16, height: u16) -> String {
    let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    t.draw(|f| ui::render(f, &app.view(), &app.config)).unwrap();
    t.backend()
        .buffer()
        .content
        .iter()
        .map(|c| c.symbol())
        .collect()
}
fn action(effects: Vec<Effect>) -> Effect {
    effects
        .into_iter()
        .find(|e| matches!(e, Effect::MediaAction { .. }))
        .expect("explicit media action")
}
fn completion(effect: &Effect, result: Result<String, String>) -> Input {
    let Effect::MediaAction {
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
fn captionless_media_has_explicit_download_and_open_actions() {
    let mut app = app();
    let before = app.view();
    assert!(press(&mut app, "enter").is_empty());
    let text = screen(&app, 120, 40);
    assert!(text.contains("Download attachment"));
    assert!(text.contains("Open downloaded file"));
    assert!(text.contains("report.pdf"));
    let effect = action(press(&mut app, "d"));
    assert!(
        matches!(&effect,Effect::MediaAction{action:MediaAction::Download,message,..}if message.key==before.messages[0].key)
    );
    assert!(press(&mut app, "v").is_empty());
    let effects = app.update(
        completion(&effect, Ok("Downloaded attachment".into())),
        Instant::now(),
    );
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::MediaAction { .. }))
    );
    assert!(matches!(
        action(press(&mut app, "v")),
        Effect::MediaAction {
            action: MediaAction::Open,
            ..
        }
    ));
    assert_eq!(app.view().draft, before.draft);
    assert_eq!(app.view().selected_message, before.selected_message);
    assert_eq!(app.view().message_scroll, before.message_scroll);
}
#[test]
fn media_keys_are_configurable_and_remain_text_in_the_composer() {
    let mut app = app();
    app.config=Config::parse("[bindings.messages]\ndownload_media=['ctrl-d']\nopen_media=['ctrl-v']\n[bindings.message_actions]\ndownload_media=['ctrl-d']\nopen_media=['ctrl-v']\n").unwrap();
    assert!(press(&mut app, "d").is_empty());
    press(&mut app, "enter");
    for (w, h) in [(40, 12), (120, 40)] {
        let s = screen(&app, w, h);
        assert!(s.contains("ctrl-d"));
        assert!(s.contains("ctrl-v"));
    }
    press(&mut app, "esc");
    let effect = action(press(&mut app, "ctrl-d"));
    app.update(completion(&effect, Err("offline".into())), Instant::now());
    let mut repeat = whatsapp_tui::config::bindings::parse_key("ctrl-d").unwrap();
    repeat.kind = KeyEventKind::Repeat;
    assert!(
        app.update(Input::Terminal(Event::Key(repeat)), Instant::now())
            .is_empty()
    );
    press(&mut app, "tab");
    press(&mut app, "d");
    press(&mut app, "v");
    assert_eq!(app.view().draft.text, "dv");
}
#[test]
fn account_switch_drops_old_media_menus_and_ignores_old_notices() {
    let mut app = app();
    let effect = action(press(&mut app, "d"));
    press(&mut app, "enter");
    app.update(
        Input::Backend(BackendEvent::AccountKnown(account("other"))),
        Instant::now(),
    );
    assert!(app.view().overlay.is_none());
    let notice = app.view().notice;
    app.update(
        completion(&effect, Ok("old account download".into())),
        Instant::now(),
    );
    assert_eq!(app.view().notice, notice);
}
