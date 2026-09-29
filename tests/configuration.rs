use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Color;
use std::path::Path;
use whatsapp_tui::config::{bindings::*, theme::*, *};
#[test]
fn defaults_match_spec() {
    let c = Config::default();
    for (role, color) in [
        (ThemeRole::Focus, Color::Rgb(0, 180, 180)),
        (ThemeRole::Accent, Color::Rgb(0, 200, 200)),
        (ThemeRole::Inactive, Color::Rgb(128, 128, 128)),
        (ThemeRole::Hints, Color::Rgb(200, 200, 0)),
        (ThemeRole::Error, Color::Rgb(255, 100, 100)),
        (ThemeRole::ErrorBorder, Color::Rgb(200, 0, 0)),
        (ThemeRole::Background, Color::Reset),
        (ThemeRole::Text, Color::Reset),
    ] {
        assert_eq!(c.theme.color(role, true), color);
    }
    for context in [Context::Chats, Context::Messages, Context::Composer] {
        for (key, action) in [
            ("tab", ActionId::FocusNext),
            ("shift-tab", ActionId::FocusPrevious),
            ("ctrl-p", ActionId::Search),
            ("f1", ActionId::Help),
            ("ctrl-q", ActionId::Quit),
        ] {
            assert_eq!(
                c.bindings.lookup(context, parse_key(key).unwrap()),
                Some(action)
            );
        }
    }
    for context in [Context::Chats, Context::Messages] {
        for (key, action) in [
            ("j", ActionId::Next),
            ("down", ActionId::Next),
            ("k", ActionId::Previous),
            ("up", ActionId::Previous),
            ("/", ActionId::Search),
            ("?", ActionId::Help),
        ] {
            assert_eq!(
                c.bindings.lookup(context, parse_key(key).unwrap()),
                Some(action)
            );
        }
    }
    for (context, key, action) in [
        (Context::Chats, "enter", ActionId::Open),
        (Context::Composer, "enter", ActionId::Send),
        (Context::Composer, "alt-enter", ActionId::Newline),
        (Context::Composer, "alt-r", ActionId::RemoveReply),
        (Context::Composer, "esc", ActionId::Back),
        (Context::Messages, "esc", ActionId::Back),
        (Context::Messages, "r", ActionId::Reply),
        (Context::Messages, "R", ActionId::Resend),
        (Context::Messages, "pageup", ActionId::PageUp),
        (Context::Messages, "pagedown", ActionId::PageDown),
        (Context::Messages, "end", ActionId::Bottom),
        (Context::Search, "enter", ActionId::Open),
        (Context::Search, "esc", ActionId::Back),
        (Context::Help, "esc", ActionId::Back),
        (Context::Resend, "enter", ActionId::Confirm),
        (Context::Resend, "esc", ActionId::Back),
    ] {
        assert_eq!(
            c.bindings.lookup(context, parse_key(key).unwrap()),
            Some(action)
        );
    }
    assert_eq!(
        c.bindings.lookup(
            Context::Composer,
            KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE)
        ),
        None
    );
    assert_eq!(c.theme.color(ThemeRole::Focus, false), Color::Cyan);
    let d = tempfile::tempdir().unwrap();
    assert!(Config::load(&d.path().join("missing.toml")).is_ok());
}
#[test]
fn relative_xdg_uses_home() {
    let p = Paths::resolve(
        Path::new("/home/test"),
        &XdgDirs {
            config: Some("relative".into()),
            data: Some("".into()),
            state: Some("/state".into()),
        },
    );
    assert_eq!(
        p.config,
        Path::new("/home/test/.config/whatsapp-tui/config.toml")
    );
    assert_eq!(p.data, Path::new("/home/test/.local/share/whatsapp-tui"));
    assert_eq!(p.state, Path::new("/state/whatsapp-tui"));
}
#[test]
fn duplicate_context_binding_is_rejected() {
    let err = Config::parse("[bindings.messages]\nreply=['j']")
        .unwrap_err()
        .to_string();
    assert!(err.contains('j'));
    assert!(Config::parse("[bindings.chats]\nunknown=['x']").is_err());
    assert!(Config::parse("typo=true").is_err());
    assert!(Config::parse("[theme]\nfocus='nope'").is_err());
    assert!(Config::parse("[bindings.composer]\nsend=['j']").is_err());
}
#[test]
fn required_actions_remain_reachable() {
    assert!(Config::parse("[bindings.global]\nquit=[]").is_err());
    assert!(Config::parse("[bindings.global]\nquit=['tab']").is_err());
    assert!(Config::parse("[bindings.composer]\nfocus_next=[]").is_err());
    let c = Config::parse("[bindings.global]\nquit=['ctrl-x']").unwrap();
    assert!(
        c.bindings
            .help(Context::Composer)
            .contains(&("ctrl-x".into(), ActionId::Quit))
    );
}
