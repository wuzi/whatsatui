mod support;

use ratatui::{
    Terminal,
    backend::{Backend, CrosstermBackend, TestBackend},
    buffer::Buffer,
    layout::Rect,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use whatsapp_tui::{
    app::model::*,
    app::{Focus, Overlay, ViewModel},
    config::Config,
    ui,
};

// TestBackend records the requested cells, not the cursor movements actually
// sent to a terminal. Replay real Crossterm output to catch misplaced writes.
// This deliberately supports only the output commands used by Backend::draw;
// unknown commands fail the test rather than silently weakening the check.
struct TextScreen {
    cells: Buffer,
    x: u16,
    y: u16,
}
impl TextScreen {
    fn new(area: Rect) -> Self {
        Self {
            cells: Buffer::empty(area),
            x: 0,
            y: 0,
        }
    }
    fn apply(&mut self, bytes: &[u8]) {
        let mut rest = std::str::from_utf8(bytes).unwrap();
        while !rest.is_empty() {
            if let Some(command) = rest.strip_prefix("\x1b[") {
                let end = command.find(|c: char| ('@'..='~').contains(&c)).unwrap();
                match &command[end..=end] {
                    "H" => {
                        let (row, col) = command[..end].split_once(';').unwrap();
                        self.y = row.parse::<u16>().unwrap() - 1;
                        self.x = col.parse::<u16>().unwrap() - 1;
                    }
                    "m" => {} // Colors and attributes do not move the cursor.
                    other => panic!("unsupported CSI {other}: {command:?}"),
                }
                rest = &command[end + 1..];
            } else if let Some(command) = rest.strip_prefix("\x1b_G") {
                let end = command.find("\x1b\\").unwrap();
                rest = &command[end + 2..]; // Virtual Kitty placements do not move it either.
            } else {
                assert!(!rest.starts_with('\x1b'), "unsupported escape: {rest:?}");
                let end = rest.find('\x1b').unwrap_or(rest.len());
                for grapheme in rest[..end].graphemes(true) {
                    let width = grapheme.width() as u16;
                    assert!(width > 0, "standalone zero-width text: {grapheme:?}");
                    self.cells[(self.x, self.y)].set_symbol(grapheme);
                    for trailing in 1..width {
                        self.cells[(self.x + trailing, self.y)].set_symbol(" ");
                    }
                    self.x += width;
                }
                rest = &rest[end..];
            }
        }
    }
    fn assert_matches(&self, expected: &Buffer, step: &str) {
        for y in expected.area.rows().map(|r| r.y) {
            let mut x = 0;
            while x < expected.area.width {
                let symbol = expected[(x, y)].symbol();
                assert_eq!(
                    self.cells[(x, y)].symbol(),
                    symbol,
                    "{step}: misplaced terminal text at ({x}, {y}); row: {:?}",
                    (0..expected.area.width)
                        .map(|col| self.cells[(col, y)].symbol())
                        .collect::<String>()
                );
                x += (symbol.width() as u16).max(1);
            }
        }
    }
}

struct Replay {
    terminal: Terminal<TestBackend>,
    previous: Buffer,
    screen: TextScreen,
}
impl Replay {
    fn new(width: u16, height: u16) -> Self {
        let area = Rect::new(0, 0, width, height);
        Self {
            terminal: Terminal::new(TestBackend::new(width, height)).unwrap(),
            previous: Buffer::empty(area),
            screen: TextScreen::new(area),
        }
    }
    fn draw(&mut self, render: impl FnOnce(&mut ratatui::Frame)) -> Vec<u8> {
        let expected = self.terminal.draw(render).unwrap().buffer.clone();
        let mut bytes = Vec::new();
        CrosstermBackend::new(&mut bytes)
            .draw(self.previous.diff(&expected).into_iter())
            .unwrap();
        self.screen.apply(&bytes);
        self.previous = expected;
        bytes
    }
    fn assert_matches(&self, step: &str) {
        self.screen.assert_matches(&self.previous, step);
    }
}

fn conversation() -> (ViewModel, Vec<MessageRecord>) {
    let mut view = support::ready_app().view();
    view.focus = Focus::Messages;
    view.chats[0].name = "Friend ❤️".into();
    let mut sender = view.chats[0].clone();
    sender.chat = "alice".into();
    view.chats.push(sender);
    let messages: Vec<_> = (0..14)
        .map(|i| {
            let from_me = i % 3 == 0;
            let mut message = support::message(
                support::key(
                    "chat",
                    if from_me { "test" } else { "alice" },
                    &i.to_string(),
                ),
                "A short message",
            );
            message.created_at_ms += i * 60_000;
            message.send_state = from_me.then_some(SendState::Delivered);
            message
        })
        .collect();
    (view, messages)
}

#[test]
fn scrolling_emoji_sender_headers_keeps_timestamps_and_blank_rows_clean() {
    let mut replay = Replay::new(100, 30);
    let (mut view, messages) = conversation();
    for count in 1..=messages.len() {
        view.messages = messages[..count].to_vec();
        for scroll in [0, 1, 2, 5, 0] {
            view.message_scroll = scroll;
            replay.draw(|frame| ui::render(frame, &view, &Config::default()));
            replay.assert_matches(&format!("{count} messages, scroll {scroll}"));
        }
    }
}

#[test]
fn scrolling_and_clearing_emoji_headers_leaves_no_stray_digits() {
    let mut replay = Replay::new(100, 30);
    let (mut view, messages) = conversation();
    for count in 1..=messages.len() {
        view.messages = messages[..count].to_vec();
        for scroll in [0, 1, 2, 5, 0] {
            view.message_scroll = scroll;
            replay.draw(|frame| ui::render(frame, &view, &Config::default()));
            for (expected, actual) in replay
                .previous
                .content
                .iter()
                .zip(&replay.screen.cells.content)
            {
                assert!(
                    expected.symbol() != " "
                        || !actual.symbol().chars().any(|c| c.is_ascii_digit()),
                    "{count} messages, scroll {scroll}: stray digit {:?} in a blank cell",
                    actual.symbol()
                );
            }
        }
    }
    view.messages.clear();
    view.loading = true;
    replay.draw(|frame| ui::render(frame, &view, &Config::default()));
    replay.assert_matches("cleared conversation");
}

#[test]
fn emoji_text_survives_overlays_and_narrow_conversation_transitions() {
    let config = Config::default();
    for (width, height) in [(40, 12), (60, 20), (100, 30)] {
        let mut replay = Replay::new(width, height);
        let (mut view, mut messages) = conversation();
        for (i, message) in messages.iter_mut().enumerate() {
            message.body = MessageBody::Text(
                [
                    "*Ready ❤️ at 12:39*",
                    "Wide 界 👩🏽‍💻 🤷🏽‍♀️ text",
                    "Family 👨‍👩‍👧‍👦 and plain ASCII",
                    "_Weather ☀️_ and e\u{301}",
                ][i % 4]
                    .into(),
            );
        }
        view.messages = messages;
        for focus in [
            Focus::Messages,
            Focus::Chats,
            Focus::Composer,
            Focus::Messages,
        ] {
            view.focus = focus;
            for overlay in [None, Some(Overlay::Help), None] {
                view.overlay = overlay;
                for scroll in [0, 1, 3, 9, 0] {
                    view.message_scroll = scroll;
                    replay.draw(|frame| ui::render(frame, &view, &config));
                    replay.assert_matches(&format!("{width}x{height}, {focus:?}, scroll {scroll}"));
                }
            }
        }
        // Changing chats can replace emoji and wide text with shorter ASCII.
        for chat in &mut view.chats {
            chat.name = "A".into();
        }
        for message in &mut view.messages {
            message.body = MessageBody::Text("Short".into());
        }
        replay.draw(|frame| ui::render(frame, &view, &config));
        replay.assert_matches("ASCII replaces emoji");
    }
}
