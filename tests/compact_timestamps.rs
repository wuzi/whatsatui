mod support;

use chrono::TimeZone;
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
fn consecutive_messages_under_ten_minutes_share_a_header_without_empty_rows() {
    let mut view = conversation();
    for gap in [0, 59_999, 60_000, 300_000, 599_999] {
        let mut second = support::message(support::key("chat", "alice", "two"), "Second body");
        second.created_at_ms += gap;
        let mut third = support::message(support::key("chat", "alice", "three"), "Third body");
        third.created_at_ms += gap * 2;
        view.messages = vec![
            support::message(support::key("chat", "alice", "one"), "First body"),
            second,
            third,
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
            let text = rows.join("\n");
            assert_eq!(text.matches("Alice ❤️").count(), 1, "gap: {gap}");
            assert_eq!(
                text.matches(&clock(view.messages[0].created_at_ms)).count(),
                1
            );
            for message in &view.messages[1..] {
                let time = clock(message.created_at_ms);
                if time != clock(view.messages[0].created_at_ms) {
                    assert!(!text.contains(&time), "repeated time at gap {gap}");
                }
            }
            let first = rows.iter().position(|r| r.contains("First body")).unwrap();
            assert!(
                rows[first + 1].contains("Second body"),
                "a repeated timestamp must not leave a blank row"
            );
            assert!(rows[first + 2].contains("Third body"));
        }
    }
}

#[test]
fn ten_minute_gaps_sender_and_date_boundaries_keep_their_timestamps() {
    let mut view = conversation();
    let base = view.messages[0].created_at_ms;
    let midnight = chrono::Local
        .with_ymd_and_hms(2026, 10, 1, 0, 0, 0)
        .single()
        .unwrap()
        .timestamp_millis();
    for (sender, from_offset, to_offset) in [
        ("alice", 0, 600_000),
        ("alice", 0, 600_001),
        ("bob", 0, 1_000),
        ("test", 0, 1_000),
        ("alice", 0, 86_400_000),
        ("alice", midnight - base - 1_000, midnight - base),
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
        if sender == "alice" {
            assert_eq!(text.matches("Alice ❤️").count(), 2);
        }
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
    second.created_at_ms += 540_000;
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
    assert!(!text.contains(&clock(view.messages[1].created_at_ms)));
    for label in [
        "First body  ✓✓",
        "Second body  !",
        "edited",
        "delivered: 1 / read: 1",
    ] {
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
            message.created_at_ms += n * 60_000;
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
                view.messages
                    .iter()
                    .map(|m| text.matches(&clock(m.created_at_ms)).count())
                    .sum::<usize>(),
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

#[test]
fn compact_to_header_boundaries_keep_the_newest_body_at_the_bottom() {
    for (sender, offset, edited) in [
        ("alice", 601_000, false),
        ("alice", 2_000, true),
        ("bob", 2_000, false),
    ] {
        let mut view = conversation();
        let first = support::message(support::key("chat", "alice", "one"), "First");
        let mut second = support::message(support::key("chat", "alice", "two"), "Second");
        second.created_at_ms += 1_000;
        let mut third = support::message(
            support::key("chat", sender, "three"),
            "Third head\nThird tail",
        );
        third.created_at_ms += offset;
        third.edited_at_ms = edited.then_some(third.created_at_ms + 1);
        view.messages = vec![first, second, third];
        let screen = draw(&view, 40, 12);
        let area = ui::layout::calculate(screen.area, view.focus).messages;
        let rows = rows(&screen, view.focus);
        assert!(
            rows[rows.len() - 2].contains("Third tail"),
            "latest body shifted above the bottom: {rows:?}"
        );
        let viewport = ui::timeline_viewport(screen.area, &view, &Config::default()).unwrap();
        assert!(viewport.fully_visible.contains(&view.messages[2].key));
        // Mouse hit positions must include the same reserved row as rendering.
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        let mut map = None;
        terminal
            .draw(|frame| {
                map = Some(ui::render_interactive(
                    frame,
                    &view,
                    &Config::default(),
                    &mut ui::Images::default(),
                    &mut ui::Avatars::default(),
                ));
            })
            .unwrap();
        assert_eq!(
            map.unwrap().hit(area.x + 8, area.bottom() - 2),
            Some(&ui::interaction::Target::Message(
                view.messages[2].key.clone()
            ))
        );
    }
}
