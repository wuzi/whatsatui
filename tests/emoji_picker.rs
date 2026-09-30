mod support;
use support::*;
use whatsapp_tui::app::{emoji, *};

fn paste(app: &mut App, text: &str) {
    app.update(
        Input::Terminal(crossterm::event::Event::Paste(text.into())),
        tokio::time::Instant::now(),
    );
}
#[test]
fn picker_inserts_at_caret_without_sending_and_cancel_keeps_draft() {
    let mut app = ready_app();
    press(&mut app, "enter");
    paste(&mut app, "Start  tail");
    press(&mut app, "home");
    for _ in 0..6 {
        press(&mut app, "right");
    }
    press(&mut app, "ctrl-e");
    assert!(matches!(app.view().overlay, Some(Overlay::Emoji { .. })));
    paste(&mut app, ":rocket:");
    let effects = press(&mut app, "enter");
    assert!(!effects.iter().any(|e| matches!(e, Effect::Prepare { .. })));
    assert_eq!(app.view().draft.text, "Start 🚀 tail");
    press(&mut app, "ctrl-e");
    paste(&mut app, "family_man_woman_girl_boy");
    press(&mut app, "enter");
    assert!(app.view().draft.text.contains("👨‍👩‍👧‍👦"));
    press(&mut app, "backspace");
    assert_eq!(app.view().draft.text, "Start 🚀 tail");
    press(&mut app, "ctrl-e");
    paste(&mut app, "heart");
    press(&mut app, "esc");
    assert_eq!(app.view().draft.text, "Start 🚀 tail");
    assert!(app.view().overlay.is_none());
}
#[test]
fn search_uses_names_shortcodes_and_unicode() {
    assert_eq!(emoji::search(":rocket:")[0].as_str(), "🚀");
    assert_eq!(emoji::search("woman technologist")[0].as_str(), "👩‍💻");
    assert_eq!(emoji::search("🚀")[0].as_str(), "🚀");
    assert!(emoji::search("no-such-emoji-xyz").is_empty());
}
