mod support;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Modifier};
use support::*;
use whatsapp_tui::{
    app::{model::*, *},
    config::Config,
    ui,
};

fn draw(body: MessageBody, width: u16, height: u16) -> (Buffer, ViewModel) {
    let mut view = ready_app().view();
    view.focus = Focus::Messages;
    view.messages[0].body = body;
    let mut t = Terminal::new(TestBackend::new(width, height)).unwrap();
    t.draw(|f| ui::render(f, &view, &Config::default()))
        .unwrap();
    (t.backend().buffer().clone(), view)
}
fn contents(b: &Buffer) -> String {
    b.content.iter().map(|c| c.symbol()).collect()
}
fn styled_word(b: &Buffer, word: &str, modifier: Modifier) -> bool {
    let chars: Vec<_> = word.chars().map(|c| c.to_string()).collect();
    b.content.windows(chars.len()).any(|cells| {
        cells
            .iter()
            .zip(&chars)
            .all(|(cell, c)| cell.symbol() == c && cell.modifier.contains(modifier))
    })
}

#[test]
fn balanced_nested_emphasis_is_styled_without_changing_source() {
    let source = "*bold _nested_* and _italic_ and ~strike~";
    let (b, view) = draw(MessageBody::Text(source.into()), 120, 40);
    for (word, style) in [
        ("bold", Modifier::BOLD),
        ("nested", Modifier::BOLD | Modifier::ITALIC),
        ("italic", Modifier::ITALIC),
        ("strike", Modifier::CROSSED_OUT),
    ] {
        assert!(styled_word(&b, word, style), "missing style for {word}");
    }
    assert!(!contents(&b).contains("*bold"));
    assert_eq!(view.messages[0].body, MessageBody::Text(source.into()));
}

#[test]
fn code_keeps_markup_literal_and_spans_lines() {
    let (b, _) = draw(
        MessageBody::Text("`*literal*`\n```\n_unchanged_\n> code\n```".into()),
        120,
        40,
    );
    assert!(styled_word(&b, "*literal*", Modifier::DIM));
    assert!(styled_word(&b, "_unchanged_", Modifier::DIM));
    assert!(contents(&b).contains("> code"));
    assert!(!contents(&b).contains('`'));
}

#[test]
fn malformed_and_intraword_delimiters_remain_literal() {
    let (b, _) = draw(
        MessageBody::Text("snake_case_name 2*3*4 *unfinished _italic_".into()),
        120,
        40,
    );
    let text = contents(&b);
    for expected in ["snake_case_name", "2*3*4", "*unfinished"] {
        assert!(text.contains(expected));
    }
    assert!(styled_word(&b, "italic", Modifier::ITALIC));
}

#[test]
fn quotes_lists_and_captions_are_formatted() {
    let (b, _) = draw(
        MessageBody::Unsupported {
            kind: "image".into(),
            caption: Some("> *quote*\n- first\n* second\n1. numbered".into()),
        },
        120,
        40,
    );
    let text = contents(&b);
    for expected in ["[image]", "│ quote", "• first", "• second", "1. numbered"] {
        assert!(text.contains(expected), "{expected}");
    }
    assert!(styled_word(&b, "quote", Modifier::BOLD));
}

#[test]
fn unicode_wraps_keep_styles_and_controls_are_sanitized() {
    let source = format!("*{}*", "界👩‍💻a\u{301}".repeat(18));
    let (b, _) = draw(MessageBody::Text(source), 40, 30);
    let styled: String = b
        .content
        .iter()
        .filter(|c| c.modifier.contains(Modifier::BOLD))
        .map(|c| c.symbol())
        .collect();
    assert_eq!(styled.replace(' ', ""), "界👩‍💻a\u{301}".repeat(18));
    let (b, _) = draw(
        MessageBody::Text("*bold*\u{1b}]52;c;payload\u{7}".into()),
        120,
        40,
    );
    assert!(!contents(&b).contains(['\u{1b}', '\u{7}']));
}
#[test]
fn formatting_delimiters_must_be_complete_graphemes() {
    for source in [
        "*️⃣ *bold*",
        "*\u{301}x* *bold*",
        "`\u{301}x` *bold*",
        "> \u{301}note *bold*",
        "* \u{301}note *bold*",
    ] {
        let (b, _) = draw(MessageBody::Text(source.into()), 120, 40);
        let literal = source.strip_suffix(" *bold*").unwrap();
        assert!(contents(&b).contains(literal), "lost content from {source}");
        assert!(styled_word(&b, "bold", Modifier::BOLD));
    }
}
#[test]
fn url_paths_remain_literal_inside_and_outside_emphasis() {
    for source in [
        "https://example.org/_path_",
        "_*https://example.org/_path_*_",
    ] {
        let (b, _) = draw(MessageBody::Text(source.into()), 120, 40);
        assert!(
            contents(&b).contains("https://example.org/_path_"),
            "{source}"
        );
        if source.starts_with('_') {
            assert!(styled_word(
                &b,
                "https://example.org/_path_",
                Modifier::BOLD | Modifier::ITALIC
            ));
        }
    }
}

#[test]
fn formatted_long_message_metrics_match_visible_scroll() {
    let (_, mut view) = draw(
        MessageBody::Text(format!("*{}*", "row\n".repeat(80).trim_end())),
        80,
        24,
    );
    let area = ratatui::layout::Rect::new(0, 0, 80, 24);
    let metrics = ui::timeline_viewport(area, &view, &Config::default()).unwrap();
    assert!(metrics.max_scroll > 60);
    view.message_scroll = metrics.max_scroll;
    let mut t = Terminal::new(TestBackend::new(80, 24)).unwrap();
    t.draw(|f| ui::render(f, &view, &Config::default()))
        .unwrap();
    assert!(styled_word(t.backend().buffer(), "row", Modifier::BOLD));
}
