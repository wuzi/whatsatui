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

fn chat(id: &str, name: &str, unread: u32) -> ChatSummary {
    ChatSummary {
        account: account("test"),
        chat: id.into(),
        name: name.into(),
        unread,
        ..Default::default()
    }
}
fn refresh(app: &mut App, chats: Vec<ChatSummary>) {
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["other".into()],
        })),
        Instant::now(),
    );
    let request = effects
        .iter()
        .find_map(|e| match e {
            Effect::LoadChats { request, .. } => Some(*request),
            _ => None,
        })
        .unwrap();
    app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(chats),
        }),
        Instant::now(),
    );
}
fn paste(app: &mut App, text: &str) {
    app.update(Input::Terminal(Event::Paste(text.into())), Instant::now());
}
fn ids(app: &App) -> Vec<String> {
    app.view()
        .search_results
        .iter()
        .map(|c| c.chat.0.clone())
        .collect()
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

#[test]
fn switcher_ranks_exact_before_fuzzy() {
    let mut app = ready_app();
    refresh(
        &mut app,
        vec![
            chat("fuzzy", "Alicia E", 0),
            chat("smith", "Alice Smith", 0),
            chat("exact", "Alice", 0),
        ],
    );
    press(&mut app, "ctrl-p");
    paste(&mut app, "alice");
    assert_eq!(ids(&app), ["exact", "smith", "fuzzy"]);
    press(&mut app, "esc");
    press(&mut app, "ctrl-p");
    paste(&mut app, "asm");
    assert_eq!(ids(&app), ["smith"]);
    press(&mut app, "esc");
    press(&mut app, "ctrl-p");
    paste(&mut app, "smith ali");
    assert_eq!(ids(&app), ["smith"]);
}

#[test]
fn unicode_and_phone_queries_match() {
    let mut app = ready_app();
    let mut cafe = chat("cafe", "CAFÉ 👩‍💻", 0);
    cafe.phone = Some("+55 (11) 9988-7766".into());
    refresh(&mut app, vec![cafe]);
    press(&mut app, "ctrl-p");
    paste(&mut app, "cfé");
    assert_eq!(ids(&app), ["cafe"]);
    press(&mut app, "esc");
    press(&mut app, "ctrl-p");
    paste(&mut app, "55119988");
    assert_eq!(ids(&app), ["cafe"]);
}

#[test]
fn unread_filter_preserves_query_and_draft() {
    let mut app = ready_app();
    refresh(
        &mut app,
        vec![chat("chat", "Alice", 0), chat("bob", "Bob", 4)],
    );
    press(&mut app, "enter");
    paste(&mut app, "unfinished 👋");
    press(&mut app, "ctrl-p");
    paste(&mut app, "b");
    press(&mut app, "ctrl-u");
    assert_eq!(ids(&app), ["bob"]);
    assert!(screen(&app, 80, 24).contains("Unread"));
    press(&mut app, "ctrl-u");
    assert_eq!(ids(&app), ["bob"]);
    press(&mut app, "esc");
    assert_eq!(app.view().focus, Focus::Composer);
    assert_eq!(app.view().draft.text, "unfinished 👋");
    press(&mut app, "esc");
    press(&mut app, "esc");
    press(&mut app, "u");
    assert_eq!(ids(&app), ["bob"]);
    press(&mut app, "enter");
    assert_eq!(app.view().chat.unwrap().0, "bob");
}

#[test]
fn switcher_preserves_identity_during_reorder() {
    let mut app = ready_app();
    let alice = chat("chat", "Alice", 0);
    let bob = chat("bob", "Bob", 2);
    refresh(&mut app, vec![alice.clone(), bob.clone()]);
    press(&mut app, "ctrl-p");
    press(&mut app, "down");
    refresh(&mut app, vec![bob, alice]);
    press(&mut app, "enter");
    assert_eq!(app.view().chat.unwrap().0, "bob");
}

#[test]
fn switcher_clamps_selection_when_result_disappears() {
    let mut app = ready_app();
    refresh(
        &mut app,
        vec![chat("chat", "Alice", 0), chat("bob", "Bob", 2)],
    );
    press(&mut app, "ctrl-p");
    press(&mut app, "down");
    refresh(&mut app, vec![chat("chat", "Alice", 0)]);
    press(&mut app, "enter");
    assert!(app.view().overlay.is_none());
    assert_eq!(app.view().chat.unwrap().0, "chat");
}

#[test]
fn switcher_shows_local_drafts() {
    let mut app = ready_app();
    press(&mut app, "enter");
    paste(&mut app, "keep this");
    press(&mut app, "ctrl-p");
    assert!(app.view().search_results[0].has_draft);
    assert!(screen(&app, 80, 24).contains("draft"));
}

#[test]
fn switcher_normalizes_paste() {
    let mut app = ready_app();
    press(&mut app, "ctrl-p");
    paste(&mut app, &format!("hi\n{}", "界".repeat(400)));
    let Some(Overlay::Search { editor, .. }) = app.view().overlay else {
        panic!("finder closed")
    };
    assert!(editor.text().starts_with("hi "));
    assert_eq!(editor.text().chars().count(), 256);
    assert!(!editor.text().contains('\n'));
}

#[test]
fn switcher_renders_empty_counts_and_effective_keys() {
    let mut app = ready_app();
    app.config =
        Config::parse("[bindings.search]\ntoggle_unread=['ctrl-y']\nback=['ctrl-b']").unwrap();
    press(&mut app, "ctrl-p");
    for (w, h) in [(40, 12), (120, 40)] {
        let text = screen(&app, w, h);
        for expected in ["1 chat", "ctrl-y", "ctrl-b"] {
            assert!(text.contains(expected), "{expected} at {w}x{h}");
        }
    }
    paste(&mut app, "nonexistent");
    for (w, h) in [(40, 12), (120, 40)] {
        assert!(screen(&app, w, h).contains("No matching chats"));
    }
}
