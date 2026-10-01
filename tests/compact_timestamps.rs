mod support;

use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, layout::Rect};
use whatsapp_tui::{
    app::{Focus, ViewModel, model::*},
    config::Config,
    ui,
};

fn clock(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .unwrap()
        .with_timezone(&chrono::Local)
        .format("%H:%M")
        .to_string()
}
fn draw(view: &ViewModel, width: u16, height: u16) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| ui::render(f, view, &Config::default()))
        .unwrap()
        .buffer
        .clone()
}
fn rows(buffer: &Buffer, focus: Focus) -> Vec<String> {
    let area = ui::layout::calculate(buffer.area, focus).messages;
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect()
        })
        .collect()
}
fn conversation() -> ViewModel {
    let mut view = support::ready_app().view();
    view.focus = Focus::Messages;
    view.chats.push(ChatSummary {
        account: "test".into(),
        chat: "alice".into(),
        name: "Alice ❤️".into(),
        ..Default::default()
    });
    view
}

#[test]
fn consecutive_messages_in_one_minute_share_time_and_have_no_empty_header_row() {
    let mut view = conversation();
    let mut second = support::message(support::key("chat", "alice", "two"), "Second body");
    second.created_at_ms += 59_999;
    view.messages = vec![
        support::message(support::key("chat", "alice", "one"), "First body"),
        second,
    ];
    for avatars in [true, false] {
        let mut config = Config::default();
        config.media.avatars = avatars;
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        let buffer = terminal
            .draw(|f| ui::render(f, &view, &config))
            .unwrap()
            .buffer
            .clone();
        let rows = rows(&buffer, view.focus);
        assert_eq!(
            rows.join("\n")
                .matches(&clock(view.messages[0].created_at_ms))
                .count(),
            1
        );
        let first = rows.iter().position(|r| r.contains("First body")).unwrap();
        assert!(
            rows[first + 1].contains("Second body"),
            "a repeated timestamp must not leave a blank row"
        );
    }
}

#[test]
fn minute_sender_and_date_boundaries_keep_their_timestamps() {
    let mut view = conversation();
    let base = view.messages[0].created_at_ms;
    for (sender, from_offset, to_offset) in [
        ("alice", 59_999, 60_000),
        ("bob", 0, 1_000),
        ("test", 0, 1_000),
        ("alice", 0, 86_400_000),
        ("alice", 1_000, 0),
    ] {
        let mut first = support::message(support::key("chat", "alice", "one"), "First body");
        let mut second = support::message(support::key("chat", sender, "two"), "Second body");
        first.created_at_ms = base + from_offset;
        second.created_at_ms = base + to_offset;
        let first_time = clock(first.created_at_ms);
        let second_time = clock(second.created_at_ms);
        view.messages = vec![first, second];
        let text = rows(&draw(&view, 100, 30), view.focus).join("\n");
        assert!(text.contains(&first_time));
        assert!(text.contains(&second_time));
        if first_time == second_time {
            assert_eq!(
                text.matches(&first_time).count(),
                2,
                "boundary for {sender} must repeat the time"
            );
        }
    }
}

#[test]
fn compact_times_preserve_delivery_edits_and_group_receipts() {
    let mut view = conversation();
    let mut first = support::message(support::key("group@g.us", "test", "one"), "First body");
    first.send_state = Some(SendState::Delivered);
    let mut second = support::message(support::key("group@g.us", "test", "two"), "Second body");
    second.created_at_ms += 1_000;
    second.send_state = Some(SendState::Failed);
    second.edited_at_ms = Some(second.created_at_ms + 1);
    view.receipts = vec![Receipt {
        key: second.key.clone(),
        recipient: "alice".into(),
        state: ReceiptState::Read,
        at_ms: second.created_at_ms,
    }];
    view.messages = vec![first, second];
    let text = rows(&draw(&view, 120, 30), view.focus).join("\n");
    assert_eq!(
        text.matches(&clock(view.messages[0].created_at_ms)).count(),
        1
    );
    for label in ["Delivered", "Failed · edited", "delivered: 1 / read: 1"] {
        assert!(text.contains(label), "lost metadata: {label}");
    }
}

#[test]
fn scrolled_compact_messages_keep_context_and_the_newest_body_visible() {
    let mut view = conversation();
    view.messages = (0..20)
        .map(|n| {
            let mut message = support::message(
                support::key("chat", "alice", &n.to_string()),
                &format!("body-{n:02}"),
            );
            message.created_at_ms += n * 1_000;
            message
        })
        .collect();
    for (width, height) in [(40, 12), (100, 24)] {
        let area = Rect::new(0, 0, width, height);
        let max = ui::timeline_viewport(area, &view, &Config::default())
            .unwrap()
            .max_scroll;
        for scroll in 0..=max {
            view.message_scroll = scroll;
            let buffer = draw(&view, width, height);
            let text = rows(&buffer, view.focus).join("\n");
            assert!(
                text.contains("Alice ❤️"),
                "missing sender at scroll {scroll}"
            );
            assert_eq!(
                text.matches(&clock(view.messages[0].created_at_ms)).count(),
                1,
                "one context time at scroll {scroll}"
            );
            let viewport = ui::timeline_viewport(area, &view, &Config::default()).unwrap();
            for key in viewport.fully_visible {
                assert!(
                    text.contains(&format!("body-{:02}", key.id.0.parse::<usize>().unwrap())),
                    "visible message must have a body, not just a substituted header"
                );
            }
            if scroll == 0 {
                assert!(text.contains("body-19"), "latest body must remain visible");
            }
        }
        view.message_scroll = 0;
    }
}
