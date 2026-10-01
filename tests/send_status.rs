mod support;

use ratatui::{
    Terminal,
    backend::TestBackend,
    buffer::Buffer,
    style::{Color, Modifier},
};
use whatsapp_tui::{
    app::{Focus, Overlay, ViewModel, model::*},
    config::Config,
    media::{Attachment, AttachmentKind, outgoing::LocalImage},
    ui,
};

fn view() -> ViewModel {
    let mut view = support::ready_app().view();
    view.focus = Focus::Messages;
    view.truecolor = true;
    view.messages = vec![support::message(
        support::key("chat", "test", "own"),
        "Hello there",
    )];
    view
}

fn draw(
    view: &ViewModel,
    config: &Config,
    width: u16,
) -> (Buffer, ui::interaction::InteractionMap) {
    let mut terminal = Terminal::new(TestBackend::new(width, 40)).unwrap();
    let mut hits = None;
    let screen = terminal
        .draw(|frame| {
            hits = Some(ui::render_interactive(
                frame,
                view,
                config,
                &mut ui::Images::default(),
                &mut ui::Avatars::default(),
            ));
        })
        .unwrap()
        .buffer
        .clone();
    (screen, hits.unwrap())
}

fn rows(buffer: &Buffer, focus: Focus) -> Vec<(u16, String)> {
    let area = ui::layout::calculate(buffer.area, focus).messages;
    (area.y..area.bottom())
        .map(|y| {
            (
                y,
                (area.x..area.right())
                    .map(|x| buffer[(x, y)].symbol())
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn outgoing_statuses_share_the_body_line_and_keep_distinct_meanings() {
    let mut view = view();
    let config = Config::default();
    for (state, glyph, color) in [
        (SendState::Sending, "…", Color::Rgb(128, 128, 128)),
        (SendState::Sent, "✓", Color::Rgb(128, 128, 128)),
        (SendState::Delivered, "✓✓", Color::Rgb(128, 128, 128)),
        (SendState::Read, "✓✓", Color::Rgb(0, 200, 200)),
        (SendState::Failed, "!", Color::Rgb(255, 100, 100)),
        (SendState::Unconfirmed, "?", Color::Rgb(200, 200, 0)),
    ] {
        view.messages[0].send_state = Some(state);
        for width in [40, 120] {
            let (screen, hits) = draw(&view, &config, width);
            let rows = rows(&screen, view.focus);
            let (y, body) = rows
                .iter()
                .find(|(_, text)| text.contains("Hello there"))
                .unwrap();
            assert!(
                body.contains(&format!("Hello there  {glyph}")),
                "{state:?}: {body}"
            );
            assert!(
                !rows
                    .iter()
                    .any(|(_, text)| text.contains(&format!("{state:?}")))
            );
            let x = (0..screen.area.width)
                .find(|&x| {
                    screen[(x, *y)].symbol() == &glyph[..glyph.chars().next().unwrap().len_utf8()]
                })
                .unwrap();
            assert_eq!(screen[(x, *y)].fg, color);
            assert_eq!(
                hits.hit(x, *y),
                Some(&ui::interaction::Target::Message(
                    view.messages[0].key.clone()
                ))
            );
        }
    }
    view.messages[0].key.from_me = false;
    view.messages[0].send_state = Some(SendState::Read);
    assert!(
        !rows(&draw(&view, &config, 120).0, view.focus)
            .iter()
            .any(|(_, row)| row.contains('✓'))
    );
}

#[test]
fn status_follows_multiline_unicode_text_and_wraps_without_losing_the_body() {
    let mut view = view();
    view.messages[0].send_state = Some(SendState::Delivered);
    view.messages[0].body = MessageBody::Text("*First ❤️*\nLast 界".into());
    let config = Config::parse("[media]\navatars=false").unwrap();
    let (screen, _) = draw(&view, &config, 40);
    let rendered = rows(&screen, view.focus);
    let (first_y, _) = rendered
        .iter()
        .find(|(_, row)| row.contains("First ❤️"))
        .unwrap();
    let (last_y, last) = rendered
        .iter()
        .find(|(_, row)| row.contains("Last 界"))
        .unwrap();
    assert_eq!(*last_y, first_y + 1);
    assert!(last.contains("✓✓"));
    let x = (0..40)
        .find(|&x| screen[(x, *first_y)].symbol() == "F")
        .unwrap();
    assert!(screen[(x, *first_y)].modifier.contains(Modifier::BOLD));
    let area = ui::layout::calculate(screen.area, view.focus).messages;
    let full_line = "x".repeat(area.width as usize - 4);
    view.messages[0].body = MessageBody::Text(full_line.clone());
    let (screen, hits) = draw(&view, &config, 40);
    let rendered = rows(&screen, view.focus);
    let (body_y, _) = rendered
        .iter()
        .find(|(_, row)| row.contains(&full_line))
        .unwrap();
    let (status_y, _) = rendered.iter().find(|(_, row)| row.contains("✓✓")).unwrap();
    assert_eq!(*status_y, body_y + 1, "status wraps after a full body line");
    assert_eq!(
        hits.hit(area.x + 3, *status_y),
        Some(&ui::interaction::Target::Message(
            view.messages[0].key.clone()
        ))
    );
}

#[test]
fn media_status_uses_the_caption_or_label_outside_quotes_and_previews() {
    let mut view = view();
    view.messages[0].send_state = Some(SendState::Delivered);
    view.messages[0].quote = Some(Quote {
        key: support::key("chat", "alice", "quoted"),
        preview: "Quoted body".into(),
        media_kind: None,
        availability: QuoteAvailability::Available,
    });
    for caption in [None, Some(""), Some("My caption")] {
        for local in [false, true] {
            view.messages[0].body = if local {
                MessageBody::LocalImage {
                    image: Box::new(LocalImage {
                        id: "a".repeat(64),
                        filename: "photo.png".into(),
                        size: 128,
                        width: 32,
                        height: 32,
                        sticker: None,
                    }),
                    caption: caption.unwrap_or_default().into(),
                }
            } else {
                MessageBody::Media(Box::new(Attachment {
                    audio: None,
                    kind: AttachmentKind::Image,
                    filename: Some("photo.png".into()),
                    mime: Some("image/png".into()),
                    caption: caption.map(str::to_owned),
                    size: 128,
                    direct_path: "/synthetic".into(),
                    media_key: [1; 32],
                    sha256: [2; 32],
                    encrypted_sha256: [3; 32],
                }))
            };
            for inline in [false, true] {
                let mut config = Config::default();
                config.media.inline = inline;
                let (screen, hits) = draw(&view, &config, 120);
                let rendered = rows(&screen, view.focus);
                let (status_y, row) = rendered
                    .iter()
                    .find(|(_, row)| row.contains("✓✓"))
                    .expect("media status");
                assert!(
                    row.contains(if caption == Some("My caption") {
                        "My caption"
                    } else {
                        "photo.png"
                    }),
                    "status in wrong row: {row}"
                );
                assert!(
                    !rendered
                        .iter()
                        .find(|(_, row)| row.contains("Quoted body"))
                        .unwrap()
                        .1
                        .contains('✓')
                );
                let area = ui::layout::calculate(screen.area, view.focus).messages;
                assert_eq!(
                    hits.hit(area.x + 8, *status_y),
                    Some(&ui::interaction::Target::Message(
                        view.messages[0].key.clone()
                    ))
                );
                if inline {
                    let (preview_y, _) = rendered
                        .iter()
                        .find(|(_, row)| row.contains("Loading preview"))
                        .expect("reserved preview");
                    assert_ne!(status_y, preview_y);
                }
            }
        }
    }
}

#[test]
fn wrapped_media_status_keeps_preview_and_quote_targets_aligned() {
    let mut view = view();
    view.messages[0].send_state = Some(SendState::Delivered);
    view.messages[0].quote = Some(Quote {
        key: support::key("chat", "alice", "quote"),
        preview: "Quoted body".into(),
        media_kind: None,
        availability: QuoteAvailability::Available,
    });
    // The label fills all 31 body columns in the narrow avatar layout.
    view.messages[0].body = MessageBody::LocalImage {
        image: Box::new(LocalImage {
            id: "a".repeat(64),
            filename: "12345678901.png".into(),
            size: 128,
            width: 32,
            height: 32,
            sticker: None,
        }),
        caption: String::new(),
    };
    let (screen, hits) = draw(&view, &Config::default(), 40);
    let rendered = rows(&screen, view.focus);
    let (label_y, _) = rendered
        .iter()
        .find(|(_, row)| row.contains("12345678901.png"))
        .unwrap();
    let (status_y, _) = rendered.iter().find(|(_, row)| row.contains("✓✓")).unwrap();
    let (preview_y, _) = rendered
        .iter()
        .find(|(_, row)| row.contains("Loading preview"))
        .unwrap();
    let (quote_y, _) = rendered
        .iter()
        .find(|(_, row)| row.contains("Quoted body"))
        .unwrap();
    assert_eq!(*status_y, label_y + 1);
    assert_eq!(*preview_y, status_y + 1);
    let area = ui::layout::calculate(screen.area, view.focus).messages;
    assert_eq!(
        hits.hit(area.x + 8, *status_y),
        Some(&ui::interaction::Target::Message(
            view.messages[0].key.clone()
        ))
    );
    assert_eq!(
        hits.hit(area.x + 8, *quote_y),
        Some(&ui::interaction::Target::Quote(
            view.messages[0].key.clone()
        ))
    );
}

#[test]
fn help_explains_status_symbols_with_their_colors() {
    let mut view = view();
    view.overlay = Some(Overlay::Help);
    view.help_scroll = usize::MAX;
    let (screen, _) = draw(&view, &Config::default(), 80);
    let text: String = screen.content.iter().map(|cell| cell.symbol()).collect();
    for label in [
        "Message status",
        "Sending",
        "Sent",
        "Delivered",
        "Read",
        "Failed",
        "Unconfirmed",
    ] {
        assert!(text.contains(label), "missing help: {label}");
    }
    let checks: Vec<_> = screen
        .content
        .iter()
        .filter(|cell| cell.symbol() == "✓")
        .map(|cell| cell.fg)
        .collect();
    assert!(checks.contains(&Color::Rgb(128, 128, 128)));
    assert!(checks.contains(&Color::Rgb(0, 200, 200)));
}
