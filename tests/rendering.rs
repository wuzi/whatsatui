mod support;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{app::model::*, app::*, config::Config, ui};
fn draw(view: &ViewModel, config: &Config, w: u16, h: u16) -> Buffer {
    let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
    t.draw(|f| ui::render(f, view, config)).unwrap();
    t.backend().buffer().clone()
}
fn text(b: &Buffer) -> String {
    b.content.iter().map(|c| c.symbol()).collect()
}
#[test]
fn ordinary_and_narrow_layouts() {
    let mut a = ready_app();
    let c = Config::default();
    for (w, h) in [(80, 24), (120, 40)] {
        let b = draw(&a.view(), &c, w, h);
        let t = text(&b);
        for expected in ["Chats", "Messages", "Message", "Alice", "Hello"] {
            assert!(t.contains(expected), "{expected} missing at {w}x{h}");
        }
        assert!(b.content.iter().any(|c| c.fg == Color::Rgb(0, 180, 180)));
        assert!(b.content.iter().any(|c| c.fg == Color::Rgb(0, 200, 200)));
        assert!(b.content.iter().all(|c| c.bg == Color::Reset));
    }
    let t = text(&draw(&a.view(), &c, 60, 20));
    assert!(t.contains("Chats"));
    assert!(!t.contains("Messages"));
    press(&mut a, "enter");
    let t = text(&draw(&a.view(), &c, 60, 20));
    assert!(t.contains("Messages"));
    assert!(!t.contains("Chats"));
    for (w, h) in [(39, 24), (80, 11)] {
        assert!(text(&draw(&a.view(), &c, w, h)).contains("Resize"));
    }
}
#[test]
fn shortcuts_live_in_help_instead_of_the_footer() {
    let c = Config::parse("[bindings.global]\nquit=['ctrl-x']").unwrap();
    let t = text(&draw(&ready_app().view(), &c, 120, 40));
    assert!(!t.contains("ctrl-x"));
    let mut view = ready_app().view();
    view.overlay = Some(Overlay::Help);
    assert!(text(&draw(&view, &c, 120, 40)).contains("ctrl-x"));
    assert!(!t.contains("ctrl-q"));
}
#[test]
fn incoming_text_cannot_emit_terminal_controls() {
    let mut v = ready_app().view();
    v.messages[0].body = MessageBody::Text("hello\u{1b}]52;c;secret\u{7}\nworld".into());
    v.chats[0].name = "Alice\u{1b}[2J".into();
    let t = text(&draw(&v, &Config::default(), 120, 40));
    assert!(!t.contains('\u{1b}'));
    assert!(!t.contains('\u{7}'));
    assert!(t.contains("hello"));
}
#[test]
fn qr_is_never_clipped() {
    let mut v = App::new(Config::default()).view();
    v.connection = ConnectionState::PairingRequired;
    v.qr = Some((
        "synthetic-pairing-payload".repeat(8),
        Instant::now() + std::time::Duration::from_secs(60),
    ));
    let small = text(&draw(&v, &Config::default(), 40, 12));
    assert!(small.contains("Resize"));
    assert!(!small.contains('█'));
    let large = text(&draw(&v, &Config::default(), 120, 60));
    assert!(large.contains("Linked devices"));
    assert!(large.contains('█') || large.contains('▀'));
}
#[test]
fn expired_qr_is_not_displayed_as_valid() {
    let mut v = App::new(Config::default()).view();
    v.qr = Some((
        "synthetic".into(),
        Instant::now() - std::time::Duration::from_secs(1),
    ));
    let t = text(&draw(&v, &Config::default(), 120, 50));
    assert!(t.contains("expired"));
    assert!(!t.contains('█'));
}
#[test]
fn wrapping_keeps_sender_and_status() {
    let mut v = ready_app().view();
    v.messages[0].key.from_me = true;
    v.messages[0].send_state = Some(SendState::Unconfirmed);
    v.messages[0].body = MessageBody::Text("wide 界 👩‍💻 words ".repeat(80));
    let t = text(&draw(&v, &Config::default(), 80, 24));
    assert!(t.contains("You"));
    assert!(t.contains("Unconfirmed"));
}
#[test]
fn renders_empty_sync_offline_quotes_media_and_group_counts() {
    let c = Config::default();
    let mut v = ready_app().view();
    v.messages.clear();
    v.connection = ConnectionState::Disconnected;
    v.syncing = true;
    v.progress = Some(42);
    let t = text(&draw(&v, &c, 120, 40));
    assert!(t.contains("Disconnected"));
    assert!(t.contains("42%"));
    assert!(t.contains("cached messages"));
    v.messages = vec![message(key("g@g.us", "test", "one"), "text")];
    v.messages[0].send_state = Some(SendState::Failed);
    v.messages[0].quote = Some(Quote {
        key: key("g@g.us", "alice", "quote"),
        preview: String::new(),
        availability: QuoteAvailability::Missing,
    });
    v.messages[0].body = MessageBody::Unsupported {
        kind: "image".into(),
        caption: Some("my caption".into()),
    };
    v.receipts = vec![Receipt {
        key: v.messages[0].key.clone(),
        recipient: "alice".into(),
        state: ReceiptState::Read,
        at_ms: 0,
    }];
    v.selected_message = Some(v.messages[0].key.clone());
    let t = text(&draw(&v, &c, 120, 40));
    for s in ["Failed", "missing", "my caption", "read: 1"] {
        assert!(t.contains(s), "missing {s}");
    }
}
#[test]
fn overlays_use_configured_confirmation_and_dismissal_keys() {
    let config = Config::parse(
        "[bindings.help]\nback=['ctrl-b']\n[bindings.resend]\nconfirm=['ctrl-y']\nback=['ctrl-n']",
    )
    .unwrap();
    let mut view = ready_app().view();
    view.overlay = Some(Overlay::Help);
    let screen = text(&draw(&view, &config, 120, 40));
    assert!(screen.contains("ctrl-b to close"));
    view.overlay = Some(Overlay::Resend {
        message: Box::new(view.messages[0].clone()),
    });
    let screen = text(&draw(&view, &config, 120, 40));
    assert!(screen.contains("ctrl-y confirms"));
    assert!(screen.contains("ctrl-n cancels"));
}

#[test]
fn sidebar_photos_leave_unread_counts_and_draft_state_visible() {
    let mut view = ready_app().view();
    view.chats[0].name = "Alexandria 👩‍💻 Very Long Contact Name".into();
    view.chats[0].unread = 42;
    view.chats[0].has_draft = true;
    view.chats[0].preview = "Recent message".into();
    for avatars in [true, false] {
        let mut config = Config::default();
        config.media.avatars = avatars;
        let screen = draw(&view, &config, 80, 24);
        let sidebar = ui::layout::calculate(screen.area, view.focus).chats;
        let row: String = (sidebar.x..sidebar.right())
            .map(|x| screen[(x, sidebar.y + 1)].symbol())
            .collect();
        assert!(
            row.contains("(42)"),
            "unread count hidden behind name: {row}"
        );
        assert!(
            row.contains("draft"),
            "draft state hidden behind name: {row}"
        );
    }
}
