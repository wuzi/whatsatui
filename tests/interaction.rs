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

#[test]
fn insert_key_focuses_the_composer_and_remains_text_inside_editors() {
    let mut a = ready_app();
    for from_messages in [false, true] {
        if from_messages {
            press(&mut a, "esc");
        }
        press(&mut a, "i");
        assert_eq!(a.view().focus, Focus::Composer);
    }
    press(&mut a, "i");
    assert_eq!(a.view().draft.text, "i");
    press(&mut a, "ctrl-p");
    press(&mut a, "i");
    let Some(Overlay::Search { editor, .. }) = a.view().overlay else {
        panic!()
    };
    assert_eq!(editor.text(), "i");
    assert_eq!(a.view().draft.text, "i");
}

#[test]
fn shifted_enter_inserts_at_the_caret_without_sending() {
    let mut a = ready_app();
    press(&mut a, "enter");
    a.update(
        Input::Terminal(Event::Paste("one👩‍💻two".into())),
        Instant::now(),
    );
    for _ in 0..3 {
        press(&mut a, "left");
    }
    let effects = press(&mut a, "shift-enter");
    assert_eq!(a.view().draft.text, "one👩‍💻\ntwo");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::Prepare { .. } | Effect::Transmit(_)))
    );
    press(&mut a, "alt-enter");
    assert_eq!(a.view().draft.text, "one👩‍💻\n\ntwo");
    assert!(
        press(&mut a, "enter")
            .iter()
            .any(|e| matches!(e, Effect::Prepare { .. }))
    );
}

#[test]
fn clear_text_preserves_reply_persists_empty_draft_and_cancels_delayed_paste() {
    let mut a = ready_app();
    press(&mut a, "tab");
    press(&mut a, "r");
    a.update(
        Input::Terminal(Event::Paste("first\n👩‍💻second".into())),
        Instant::now(),
    );
    let quote = a.view().draft.reply;
    let effects = press(&mut a, "ctrl-v");
    let Effect::PasteClipboard {
        request,
        account,
        chat,
        ..
    } = effects
        .into_iter()
        .find(|e| matches!(e, Effect::PasteClipboard { .. }))
        .unwrap()
    else {
        panic!()
    };
    assert!(
        !press(&mut a, "ctrl-c")
            .iter()
            .any(|e| matches!(e, Effect::Shutdown))
    );
    assert_eq!(a.view().draft.text, "");
    assert_eq!(a.view().draft.reply, quote);
    a.update(
        Input::ClipboardRead {
            request,
            account,
            chat,
            result: Ok(whatsapp_tui::desktop::clipboard::Paste::Text(
                "too late".into(),
            )),
        },
        Instant::now(),
    );
    assert_eq!(a.view().draft.text, "");
    assert!(press(&mut a, "esc").iter().any(|e| matches!(e, Effect::SaveDraft { draft, .. } if draft.text.is_empty() && draft.reply == quote)));
    press(&mut a, "i");
    press(&mut a, "a");
    assert_eq!(a.view().draft.text, "a");
}

#[test]
fn local_text_controls_remain_available_while_syncing_is_under_pressure() {
    let mut a = ready_app();
    press(&mut a, "enter");
    press(&mut a, "a");
    for (key, expected) in [("shift-enter", "a\n"), ("ctrl-c", "")] {
        let event = Event::Key(whatsapp_tui::config::bindings::parse_key(key).unwrap());
        let effects = a.input_under_pressure(event, Instant::now());
        assert_eq!(a.view().draft.text, expected);
        assert!(
            !effects
                .iter()
                .any(|e| matches!(e, Effect::Prepare { .. } | Effect::Shutdown))
        );
    }
}
