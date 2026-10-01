mod support;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use support::*;
use tokio::time::Instant;
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    ui,
    whatsapp::BackendEvent,
};

fn draw(view: &ViewModel, config: &Config, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| ui::render(f, view, config)).unwrap();
    terminal.backend().buffer().clone()
}
fn timeline_text(buffer: &Buffer, focus: Focus) -> String {
    let r = ui::layout::calculate(buffer.area, focus).messages;
    (r.y..r.bottom())
        .flat_map(|y| (r.x..r.right()).map(move |x| (x, y)))
        .map(|p| buffer[p].symbol())
        .collect()
}
fn load(app: &mut App, messages: Vec<MessageRecord>) {
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
                summary: app.view().chats[0].clone(),
                messages,
                receipts: vec![],
                draft: Draft::default(),
                has_older: false,
                has_newer: false,
            })),
        }),
        Instant::now(),
    );
}
fn metrics(app: &mut App) {
    let viewport =
        ui::timeline_viewport(Rect::new(0, 0, 100, 24), &app.view(), &app.config).unwrap();
    app.update(Input::TimelineViewport(viewport), Instant::now());
}

#[test]
fn consecutive_senders_are_grouped_without_hiding_message_times() {
    let mut view = ready_app().view();
    view.chats[0].name = "Friends".into();
    view.chats[0].is_group = true;
    view.chats.push(ChatSummary {
        account: account("test"),
        chat: "alice".into(),
        name: "Alice".into(),
        ..Default::default()
    });
    view.chats.push(ChatSummary {
        account: account("test"),
        chat: "bob".into(),
        name: "Bob".into(),
        ..Default::default()
    });
    let mut second = message(key("chat", "alice", "two"), "Second message");
    second.created_at_ms += 60_000;
    let mut third = message(key("chat", "bob", "three"), "Different person");
    third.created_at_ms += 120_000;
    view.messages = vec![
        message(key("chat", "alice", "one"), "First message"),
        second,
        third,
    ];
    view.selected_message = Some(view.messages[2].key.clone());
    let text = timeline_text(&draw(&view, &Config::default(), 120, 40), view.focus);
    assert_eq!(
        text.matches("Alice").count(),
        1,
        "one sender heading per consecutive block"
    );
    assert!(
        text.contains("Bob") && text.contains("First message") && text.contains("Second message")
    );
    let date = chrono::DateTime::from_timestamp_millis(view.messages[0].created_at_ms)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%Y-%m-%d")
        .to_string();
    assert!(text.contains(&date));
}

#[test]
fn own_sender_stays_distinct_when_another_message_is_selected() {
    let mut view = ready_app().view();
    view.messages = vec![
        message(key("chat", "test", "mine"), "My message"),
        message(key("chat", "alice", "theirs"), "Their message"),
    ];
    view.selected_message = Some(view.messages[1].key.clone());
    let b = draw(&view, &Config::default(), 120, 40);
    let colors: Vec<_> = b
        .content
        .windows(3)
        .filter(|w| w[0].symbol() == "Y" && w[1].symbol() == "o" && w[2].symbol() == "u")
        .map(|w| w[0].fg)
        .collect();
    assert!(colors.contains(&ratatui::style::Color::Rgb(111, 220, 163)));
}

#[test]
fn keyboard_selects_whole_messages_and_scrolling_keeps_selection() {
    let mut app = ready_app();
    let long = message(
        key("chat", "alice", "long"),
        &(0..80).map(|i| format!("ROW-{i:02}\n")).collect::<String>(),
    );
    load(
        &mut app,
        vec![message(key("chat", "alice", "short"), "Before"), long],
    );
    press(&mut app, "tab");
    metrics(&mut app);
    press(&mut app, "k");
    assert_eq!(app.view().selected_message.unwrap().id.0, "short");
    metrics(&mut app);
    press(&mut app, "j");
    metrics(&mut app);
    let selected = app.view().selected_message;
    press(&mut app, "K");
    assert_eq!(app.view().selected_message, selected);
    assert!(app.view().message_scroll > 0);
    press(&mut app, "end");
    assert_eq!(app.view().message_scroll, 0);
    assert!(app.view().at_bottom);
}

fn interactive(app: &mut App) -> Buffer {
    interactive_size(app, 100, 24)
}
fn interactive_size(app: &mut App, width: u16, height: u16) -> Buffer {
    let view = app.view();
    let config = app.config.clone();
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut map = None;
    terminal
        .draw(|f| {
            map = Some(ui::render_interactive(
                f,
                &view,
                &config,
                &mut ui::Images::default(),
                &mut ui::Avatars::default(),
            ));
        })
        .unwrap();
    app.update(Input::Rendered(map.unwrap()), Instant::now());
    if let Some(viewport) =
        ui::timeline_viewport(Rect::new(0, 0, width, height), &app.view(), &app.config)
    {
        app.update(Input::TimelineViewport(viewport), Instant::now());
    }
    terminal.backend().buffer().clone()
}
fn point(buffer: &Buffer, text: &str) -> (u16, u16) {
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            let line: String = (x..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if line.starts_with(text) {
                return (x, y);
            }
        }
    }
    panic!("{text} not on screen");
}
fn mouse(app: &mut App, point: (u16, u16), kind: crossterm::event::MouseEventKind) -> Vec<Effect> {
    app.update(
        Input::Terminal(crossterm::event::Event::Mouse(
            crossterm::event::MouseEvent {
                kind,
                column: point.0,
                row: point.1,
                modifiers: crossterm::event::KeyModifiers::NONE,
            },
        )),
        Instant::now(),
    )
}

#[test]
fn right_click_targets_wrapped_message_and_menu_uses_that_identity() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    load(
        &mut app,
        vec![
            message(key("chat", "alice", "first"), "first line\ncontinuation"),
            message(key("chat", "alice", "second"), "second message"),
        ],
    );
    let screen = interactive(&mut app);
    mouse(
        &mut app,
        point(&screen, "continuation"),
        Down(MouseButton::Right),
    );
    assert_eq!(app.view().selected_message.unwrap().id.0, "first");
    assert!(
        matches!(app.view().overlay, Some(Overlay::MessageActions(m)) if m.message.key.id.0 == "first")
    );
    let screen = interactive(&mut app);
    let copy = point(&screen, "Copy text");
    mouse(&mut app, copy, Down(MouseButton::Left));
    interactive(&mut app);
    let effects = mouse(&mut app, copy, Down(MouseButton::Left));
    assert!(effects.iter().any(
        |e| matches!(e, Effect::DesktopAction { message, .. } if message.key.id.0 == "first")
    ));
}

#[test]
fn composer_click_snaps_inside_wide_emoji_to_a_grapheme_boundary() {
    use crossterm::event::{Event, MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    press(&mut app, "enter");
    app.update(Input::Terminal(Event::Paste("A👩‍💻B".into())), Instant::now());
    let screen = interactive(&mut app);
    let (x, y) = point(&screen, "A👩‍💻");
    mouse(&mut app, (x + 2, y), Down(MouseButton::Left));
    press(&mut app, "X");
    assert_eq!(app.view().draft.text, "AX👩‍💻B");
}

#[test]
fn popups_capture_clicks_and_close_control_is_clickable() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    press(&mut app, "f1");
    let screen = interactive(&mut app);
    mouse(&mut app, (99, 22), Down(MouseButton::Left));
    assert!(matches!(app.view().overlay, Some(Overlay::Help)));
    interactive(&mut app);
    mouse(&mut app, point(&screen, "[×]"), Down(MouseButton::Left));
    assert!(app.view().overlay.is_none());
}

#[test]
fn disabled_mouse_and_stale_resize_maps_cannot_change_selection() {
    use crossterm::event::{Event, MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    let screen = interactive(&mut app);
    app.update(Input::Terminal(Event::Resize(40, 12)), Instant::now());
    mouse(&mut app, point(&screen, "Hello"), Down(MouseButton::Right));
    assert!(app.view().overlay.is_none());
    app.config = Config::parse("[ui]\nmouse = false").unwrap();
    interactive(&mut app);
    mouse(&mut app, point(&screen, "Hello"), Down(MouseButton::Right));
    assert!(app.view().overlay.is_none());
}

#[test]
fn wheel_scroll_preserves_the_action_selection() {
    use crossterm::event::MouseEventKind::ScrollUp;
    let mut app = ready_app();
    load(
        &mut app,
        vec![message(
            key("chat", "alice", "long"),
            &"many lines\n".repeat(80),
        )],
    );
    interactive(&mut app);
    let before = app.view().selected_message;
    mouse(&mut app, (70, 10), ScrollUp);
    assert_eq!(app.view().selected_message, before);
    assert!(app.view().message_scroll > 0);
}

#[test]
fn selecting_first_message_keeps_following_context_visible() {
    let mut app = ready_app();
    load(
        &mut app,
        vec![
            message(key("chat", "alice", "one"), "Earlier context"),
            message(key("chat", "alice", "two"), "Following context"),
        ],
    );
    press(&mut app, "tab");
    press(&mut app, "k");
    let screen = draw(&app.view(), &app.config, 100, 24);
    assert!(timeline_text(&screen, app.view().focus).contains("Following context"));
}

#[test]
fn compact_messages_keep_keyboard_selection_and_mouse_actions_on_the_body() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    load(
        &mut app,
        vec![
            message(key("chat", "alice", "one"), "First body"),
            message(key("chat", "alice", "two"), "Second body"),
            message(key("chat", "alice", "three"), "Third body"),
        ],
    );
    press(&mut app, "tab");
    press(&mut app, "k");
    let screen = interactive(&mut app);
    assert_eq!(app.view().selected_message.unwrap().id.0, "two");
    let second = point(&screen, "Second body");
    let area = ui::layout::calculate(screen.area, app.view().focus).messages;
    assert_eq!(
        screen[(area.x + 1, second.1)].symbol(),
        "▸",
        "compact messages select their first body row"
    );
    mouse(
        &mut app,
        point(&screen, "Third body"),
        Down(MouseButton::Right),
    );
    assert!(
        matches!(app.view().overlay, Some(Overlay::MessageActions(m)) if m.message.key.id.0 == "three")
    );
}

#[test]
fn selecting_past_the_cached_page_requests_next_history_page() {
    let mut app = ready_app();
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
                summary: app.view().chats[0].clone(),
                messages: vec![message(key("chat", "alice", "older"), "Cached page")],
                receipts: vec![],
                draft: Draft::default(),
                has_older: true,
                has_newer: true,
            })),
        }),
        Instant::now(),
    );
    press(&mut app, "tab");
    metrics(&mut app);
    let effects = press(&mut app, "j");
    assert!(effects.iter().any(|e| matches!(e, Effect::LoadChat {cursor: Some(PageCursor {direction: PageDirection::After, key, ..}), ..} if key.id.0 == "older")));
}

#[test]
fn clicking_a_scrolled_popup_item_keeps_it_under_the_pointer() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let mut app = many_chats();
    press(&mut app, "ctrl-p");
    for _ in 0..25 {
        press(&mut app, "down");
    }
    let screen = interactive(&mut app);
    let target = point(&screen, "Person 24");
    mouse(&mut app, target, Down(MouseButton::Left));
    let screen = interactive(&mut app);
    assert_eq!(
        point(&screen, "Person 24"),
        target,
        "clicking must not shift the item before the second click"
    );
    mouse(&mut app, target, Down(MouseButton::Left));
    assert_eq!(app.view().chat, Some("person24".into()));
    assert!(app.view().overlay.is_none());
}

fn many_chats() -> App {
    let mut app = ready_app();
    let effects = app.update(
        Input::Backend(BackendEvent::StoreChanged(StoreChange {
            account: account("test"),
            chats: vec!["other".into()],
        })),
        Instant::now(),
    );
    let request = effects
        .iter()
        .find_map(|e| {
            if let Effect::LoadChats { request, .. } = e {
                Some(*request)
            } else {
                None
            }
        })
        .unwrap();
    let chats = (0..30)
        .map(|n| ChatSummary {
            account: account("test"),
            chat: if n == 0 {
                "chat".into()
            } else {
                format!("person{n}").into()
            },
            name: format!("Person {n:02}"),
            ..Default::default()
        })
        .collect();
    app.update(
        Input::Store(StoreCompletion::Chats {
            request,
            account: account("test"),
            result: Ok(chats),
        }),
        Instant::now(),
    );
    app
}

#[test]
fn unused_list_rows_never_select_hidden_conversations() {
    use crossterm::event::{MouseButton, MouseEventKind::Down};
    let mut app = many_chats();
    press(&mut app, "ctrl-p");
    let screen = interactive_size(&mut app, 80, 20);
    let (x, y) = point(&screen, "Person 05");
    assert_eq!(screen[(x, y + 2)].symbol(), " ");
    mouse(&mut app, (x, y + 2), Down(MouseButton::Left));
    assert!(matches!(
        app.view().overlay,
        Some(Overlay::Search { selected: 0, .. })
    ));

    press(&mut app, "esc");
    let screen = interactive_size(&mut app, 80, 21);
    let (x, y) = point(&screen, "Person 07");
    assert_eq!(screen[(x, y + 2)].symbol(), " ");
    mouse(&mut app, (x, y + 2), Down(MouseButton::Left));
    assert_eq!(app.view().chat, Some("chat".into()));
}

#[test]
fn crlf_paste_renders_lines_and_clicks_preserve_original_byte_offsets() {
    use crossterm::event::{Event, MouseButton, MouseEventKind::Down};
    let mut app = ready_app();
    press(&mut app, "enter");
    app.update(
        Input::Terminal(Event::Paste("first\r\nA👩‍💻second".into())),
        Instant::now(),
    );
    let screen = interactive(&mut app);
    let first = point(&screen, "first");
    let second = point(&screen, "A👩‍💻");
    assert_eq!(
        second.1,
        first.1 + 1,
        "CRLF must occupy one visual line break"
    );
    mouse(&mut app, (second.0 + 1, second.1), Down(MouseButton::Left));
    press(&mut app, "X");
    assert_eq!(app.view().draft.text, "first\r\nAX👩‍💻second");
}

#[test]
fn long_sender_names_keep_time_and_edited_state_visible_at_forty_columns() {
    let mut view = ready_app().view();
    view.focus = Focus::Messages;
    view.chats.push(ChatSummary {
        account: account("test"),
        chat: "alice".into(),
        name: "Alexandria 👩‍💻 李 Elizabeth Very Long Contact Name".into(),
        ..Default::default()
    });
    let mut original = message(key("chat", "alice", "long-name"), "Readable message");
    original.edited_at_ms = Some(original.created_at_ms + 1);
    let time = chrono::DateTime::from_timestamp_millis(original.created_at_ms)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string();
    view.selected_message = Some(original.key.clone());
    view.messages = vec![original];
    let text = timeline_text(&draw(&view, &Config::default(), 40, 20), view.focus);
    assert!(
        text.contains(&time),
        "sender names cannot hide message time"
    );
    assert!(text.contains("edited"), "edited state must remain visible");
    assert!(text.contains("Alexandria"));
}
