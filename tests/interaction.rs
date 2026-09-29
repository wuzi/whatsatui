mod support;
use crossterm::event::{Event, KeyEventKind};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::app::{
    editor::{EditAction, Editor},
    *,
};
#[test]
fn composer_printable_keys_are_text() {
    let mut a = ready_app();
    press(&mut a, "enter");
    for c in ["j", "k", "/", "r", "?"] {
        press(&mut a, c);
    }
    assert_eq!(a.view().draft.text, "jk/r?");
}
#[test]
fn paste_never_submits() {
    let mut a = ready_app();
    press(&mut a, "enter");
    let effects = a.update(
        Input::Terminal(Event::Paste("first\nsecond".into())),
        Instant::now(),
    );
    assert_eq!(a.view().draft.text, "first\nsecond");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Prepare { .. } | Effect::Transmit(_)))
    );
}
#[test]
fn focus_cycle_matches_spec() {
    let mut a = ready_app();
    for focus in [Focus::Messages, Focus::Composer, Focus::Chats] {
        press(&mut a, "tab");
        assert_eq!(a.view().focus, focus);
    }
    press(&mut a, "shift-tab");
    assert_eq!(a.view().focus, Focus::Composer);
    press(&mut a, "esc");
    assert_eq!(a.view().focus, Focus::Messages);
    press(&mut a, "esc");
    assert_eq!(a.view().focus, Focus::Chats);
}
#[test]
fn search_cancel_restores_focus() {
    let mut a = ready_app();
    press(&mut a, "enter");
    press(&mut a, "ctrl-p");
    assert!(matches!(a.view().overlay, Some(Overlay::Search { .. })));
    press(&mut a, "j");
    press(&mut a, "/");
    if let Some(Overlay::Search { editor, .. }) = a.view().overlay {
        assert_eq!(editor.text(), "j/");
    } else {
        panic!("search closed");
    }
    press(&mut a, "esc");
    assert_eq!(a.view().focus, Focus::Composer);
    assert!(a.view().overlay.is_none());
    assert_eq!(a.view().chat.unwrap().0, "chat");
}
#[test]
fn reply_preserves_draft() {
    let mut a = ready_app();
    press(&mut a, "enter");
    a.update(
        Input::Terminal(Event::Paste("existing".into())),
        Instant::now(),
    );
    press(&mut a, "esc");
    press(&mut a, "r");
    assert_eq!(a.view().focus, Focus::Composer);
    assert_eq!(a.view().draft.text, "existing");
    assert_eq!(a.view().draft.reply.unwrap().key.id.0, "one");
    press(&mut a, "alt-r");
    assert!(a.view().draft.reply.is_none());
    assert_eq!(a.view().draft.text, "existing");
}
#[test]
fn grapheme_editing_is_valid() {
    for text in ["hi👩‍💻", "hia\u{301}"] {
        let mut e = Editor::new(text.into());
        e.apply(EditAction::Backspace);
        assert_eq!(e.text(), "hi");
        assert!(e.text().is_char_boundary(e.cursor()));
    }
    let mut e = Editor::new("界x\na\nlong".into());
    e.apply(EditAction::Up);
    assert_eq!(e.cursor(), 6);
    e.apply(EditAction::Home);
    e.apply(EditAction::Up);
    assert_eq!(e.cursor(), 0);
    e.apply(EditAction::Right);
    assert_eq!(e.cursor(), 3);
    e.apply(EditAction::Delete);
    assert_eq!(e.text(), "界\na\nlong");
}
#[test]
fn key_release_and_held_enter_do_not_submit() {
    let mut a = ready_app();
    press(&mut a, "enter");
    press(&mut a, "h");
    let mut key = whatsapp_tui::config::bindings::parse_key("enter").unwrap();
    key.kind = KeyEventKind::Repeat;
    assert!(
        a.update(Input::Terminal(Event::Key(key)), Instant::now())
            .is_empty()
    );
    key.kind = KeyEventKind::Release;
    assert!(
        a.update(Input::Terminal(Event::Key(key)), Instant::now())
            .is_empty()
    );
}
