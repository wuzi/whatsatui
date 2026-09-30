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
