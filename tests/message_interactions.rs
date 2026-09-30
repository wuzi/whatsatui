mod support;
use crossterm::event::Event;
use ratatui::{Terminal, backend::TestBackend};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    ui,
    whatsapp::BackendEvent,
};

fn refresh(app: &mut App, messages: Vec<MessageRecord>, interactions: MessageInteractions) {
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
                summary: view.chats[0].clone(),
                messages,
                draft: view.draft,
                receipts: vec![],
                interactions,
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
}
fn own_app() -> App {
    let mut app = ready_app();
    let mut m = message(key("chat", "test", "own"), "original");
    m.created_at_ms = chrono::Utc::now().timestamp_millis() - 1000;
    m.send_state = Some(SendState::Read);
    refresh(&mut app, vec![m], Default::default());
    press(&mut app, "enter");
    paste(&mut app, "draft kept");
    press(&mut app, "esc");
    app
}
fn paste(app: &mut App, text: &str) {
    app.update(Input::Terminal(Event::Paste(text.into())), Instant::now());
}
fn screen(app: &App, w: u16, h: u16) -> String {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::render(f, &app.view(), &app.config)).unwrap();
    let buffer = t.backend().buffer();
    let mut text = String::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let symbol = buffer[(x, y)].symbol();
            text.push_str(symbol);
            x += unicode_width::UnicodeWidthStr::width(symbol).max(1) as u16;
        }
        text.push('\n');
    }
    text
}

fn mutation(effects: Vec<Effect>) -> (RequestId, MessageRecord, MutationKind) {
    effects
        .into_iter()
        .find_map(|e| {
            if let Effect::Mutate {
                request,
                message,
                kind,
            } = e
            {
                Some((request, *message, kind))
            } else {
                None
            }
        })
        .expect("mutation effect")
}
#[test]
fn editing_cancel_and_save_preserve_the_normal_draft() {
    let mut app = own_app();
    let draft = app.view().draft;
    press(&mut app, "e");
    assert!(screen(&app, 120, 35).contains("Editing"));
    paste(&mut app, " plus");
    assert_eq!(
        app.view().editing.as_ref().unwrap().editor.text(),
        "original plus"
    );
    assert_eq!(app.view().draft, draft);
    press(&mut app, "esc");
    assert!(app.view().editing.is_none());
    assert_eq!(app.view().draft, draft);
    press(&mut app, "esc"); // Messages
    press(&mut app, "e");
    paste(&mut app, " changed");
    let (request, message, kind) = mutation(press(&mut app, "enter"));
    assert_eq!(message.key.id.0, "own");
    assert_eq!(
        kind,
        MutationKind::Edit {
            text: "original changed".into()
        }
    );
    assert!(
        !press(&mut app, "enter")
            .iter()
            .any(|e| matches!(e, Effect::Mutate { .. }))
    );
    app.update(
        Input::Backend(BackendEvent::MutationOutcome {
            request,
            account: account("test"),
            result: Ok(MutationState::Sent),
        }),
        Instant::now(),
    );
    assert!(app.view().editing.is_none());
    assert_eq!(app.view().draft, draft);
}
#[test]
fn failed_and_stale_edits_keep_proposed_text_but_cannot_overwrite_an_incoming_edit() {
    let mut app = own_app();
    press(&mut app, "e");
    paste(&mut app, " proposed");
    let (request, _, _) = mutation(press(&mut app, "enter"));
    app.update(
        Input::Backend(BackendEvent::MutationOutcome {
            request,
            account: account("test"),
            result: Ok(MutationState::Failed),
        }),
        Instant::now(),
    );
    assert_eq!(
        app.view().editing.as_ref().unwrap().editor.text(),
        "original proposed"
    );
    let mut changed = app.view().messages[0].clone();
    changed.body = MessageBody::Text("edited on phone".into());
    changed.edited_at_ms = Some(chrono::Utc::now().timestamp_millis());
    refresh(&mut app, vec![changed], Default::default());
    assert!(
        !press(&mut app, "enter")
            .iter()
            .any(|e| matches!(e, Effect::Mutate { .. }))
    );
    assert_eq!(app.view().draft.text, "draft kept");
    assert_eq!(
        app.view().editing.as_ref().unwrap().editor.text(),
        "original proposed"
    );
}
#[test]
fn reaction_picker_keeps_target_through_arrivals_and_same_emoji_removes() {
    let mut app = ready_app();
    press(&mut app, "tab");
    let original = app.view().messages[0].clone();
    press(&mut app, "a");
    paste(&mut app, "thumbsup");
    let mut arrival = message(key("chat", "alice", "new"), "new");
    arrival.created_at_ms += 1000;
    refresh(
        &mut app,
        vec![original.clone(), arrival],
        MessageInteractions {
            reactions: vec![Reaction {
                key: original.key.clone(),
                reactor: "test".into(),
                emoji: "👍".into(),
                at_ms: 10,
                event_id: "r".into(),
            }],
            ..Default::default()
        },
    );
    let (_, target, kind) = mutation(press(&mut app, "enter"));
    assert_eq!(target.key.id.0, "one");
    assert_eq!(
        kind,
        MutationKind::Reaction {
            emoji: String::new()
        }
    );
    assert!(app.view().draft.text.is_empty());
}
#[test]
fn reaction_details_show_people_counts_and_mine_with_mouse_targets() {
    let mut app = ready_app();
    let m = app.view().messages[0].clone();
    let reactions = [("test", "👍"), ("alice", "👍"), ("bob", "❤️")]
        .into_iter()
        .map(|(p, e)| Reaction {
            key: m.key.clone(),
            reactor: p.into(),
            emoji: e.into(),
            at_ms: 1,
            event_id: p.into(),
        })
        .collect();
    refresh(
        &mut app,
        vec![m.clone()],
        MessageInteractions {
            reactions,
            ..Default::default()
        },
    );
    press(&mut app, "tab");
    for (w, h) in [(40, 16), (120, 35)] {
        let text = screen(&app, w, h);
        assert!(text.contains("👍 2"), "{w}x{h}: {text:?}");
        assert!(text.contains("You"));
    }
    press(&mut app, "i");
    let text = screen(&app, 120, 35);
    for label in ["Reactions", "You", "alice", "bob", "Remove mine"] {
        assert!(text.contains(label), "{label}");
    }
    let (_, target, kind) = mutation(press(&mut app, "x"));
    assert_eq!(target.key, m.key);
    assert_eq!(
        kind,
        MutationKind::Reaction {
            emoji: String::new()
        }
    );
}
#[test]
fn composer_printable_keys_and_remapped_action_keys_work() {
    let mut app = own_app();
    app.config =
        Config::parse("[bindings.messages]\nreact=['ctrl-a']\nedit_message=['ctrl-e']").unwrap();
    press(&mut app, "ctrl-e");
    assert!(app.view().editing.is_some());
    press(&mut app, "esc");
    for k in ["a", "e", "i", "q", "x"] {
        press(&mut app, k);
    }
    assert_eq!(app.view().draft.text, "draft keptaeiqx");
    press(&mut app, "esc");
    press(&mut app, "ctrl-a");
    assert!(matches!(app.view().overlay, Some(Overlay::Emoji { .. })));
}

#[test]
fn clicking_the_rendered_reaction_row_opens_details_at_narrow_and_wide_sizes() {
    use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
    for (w, h) in [(40, 16), (120, 35)] {
        let mut app = ready_app();
        let m = app.view().messages[0].clone();
        refresh(
            &mut app,
            vec![m.clone()],
            MessageInteractions {
                reactions: vec![Reaction {
                    key: m.key.clone(),
                    reactor: "alice".into(),
                    emoji: "👍".into(),
                    at_ms: 1,
                    event_id: "r".into(),
                }],
                ..Default::default()
            },
        );
        press(&mut app, "tab");
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        let mut map = None;
        terminal
            .draw(|f| {
                map = Some(ui::render_interactive(
                    f,
                    &app.view(),
                    &app.config,
                    &mut ui::Images::default(),
                    &mut ui::Avatars::default(),
                ))
            })
            .unwrap();
        let map = map.unwrap();
        let (x,y)=(0..h).flat_map(|y|(0..w).map(move|x|(x,y))).find(|&(x,y)|matches!(map.hit(x,y),Some(ui::interaction::Target::Reactions(key)) if key==&m.key)).unwrap();
        app.update(Input::Rendered(map), Instant::now());
        app.update(
            Input::Terminal(Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: x,
                row: y,
                modifiers: KeyModifiers::NONE,
            })),
            Instant::now(),
        );
        assert!(matches!(app.view().overlay, Some(Overlay::Reactions(_))));
    }
}
#[test]
fn completion_from_a_previous_account_cannot_touch_the_current_draft() {
    let mut app = own_app();
    press(&mut app, "e");
    paste(&mut app, " edit");
    let (request, _, _) = mutation(press(&mut app, "enter"));
    app.update(
        Input::Backend(BackendEvent::AccountKnown("different".into())),
        Instant::now(),
    );
    let before = app.view();
    app.update(
        Input::Backend(BackendEvent::MutationOutcome {
            request,
            account: account("test"),
            result: Ok(MutationState::Sent),
        }),
        Instant::now(),
    );
    assert_eq!(app.view().draft, before.draft);
    assert_eq!(app.view().notice, before.notice);
    assert!(app.view().editing.is_none());
}
