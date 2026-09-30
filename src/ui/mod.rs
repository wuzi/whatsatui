mod actions;
mod attachments;
mod chat_list;
mod composer;
pub mod images;
pub mod layout;
mod overlays;
mod rich_text;
mod search;
mod timeline;
use crate::{
    app::{Focus, Overlay, ViewModel},
    config::{Config, bindings::Context, theme::ThemeRole},
};
pub use images::Images;
use ratatui::{
    prelude::*,
    widgets::{Block, BorderType, Paragraph, Wrap},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub fn timeline_viewport(
    area: Rect,
    view: &ViewModel,
    config: &Config,
) -> Option<crate::app::TimelineViewport> {
    timeline::viewport(area, view, config)
}

pub fn safe_text(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            '\n' => out.push('\n'),
            '\t' => out.push_str("    "),
            '\u{1b}' => out.push('␛'),
            c if c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') => {
                out.push('�')
            }
            c => out.push(c),
        }
    }
    out
}
fn single(text: &str) -> String {
    safe_text(text).replace('\n', " ")
}
fn style(config: &Config, view: &ViewModel, role: ThemeRole) -> Style {
    Style::default().fg(config.theme.color(role, view.truecolor))
}
fn block(title: String, focused: bool, view: &ViewModel, config: &Config) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .border_style(style(
            config,
            view,
            if focused {
                ThemeRole::Focus
            } else {
                ThemeRole::Inactive
            },
        ))
}
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = vec![];
    for source in text.split('\n') {
        let mut line = String::new();
        let mut columns = 0;
        for g in source.graphemes(true) {
            let size = g.width();
            if columns + size > width && !line.is_empty() {
                lines.push(std::mem::take(&mut line));
                columns = 0;
            }
            line.push_str(g);
            columns += size;
        }
        lines.push(line);
    }
    lines
}
fn timestamp(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| d.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".into())
}
fn sender(view: &ViewModel, id: &str) -> String {
    view.chats
        .iter()
        .find(|c| c.chat.0 == id)
        .map(|c| single(&c.name))
        .unwrap_or_else(|| {
            single(
                id.strip_suffix("@s.whatsapp.net")
                    .or(id.strip_suffix("@lid"))
                    .unwrap_or(id),
            )
        })
}
pub fn render(frame: &mut Frame, view: &ViewModel, config: &Config) {
    render_with_images(frame, view, config, &mut Images::default());
}
pub fn render_with_images(
    frame: &mut Frame,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
) {
    images.begin_frame();
    render_content(frame, view, config, images);
    images.end_frame();
}
fn render_content(frame: &mut Frame, view: &ViewModel, config: &Config, images: &mut Images) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(
            style(config, view, ThemeRole::Text)
                .bg(config.theme.color(ThemeRole::Background, view.truecolor)),
        ),
        area,
    );
    let regions = layout::calculate(area, view.focus);
    if regions.too_small {
        frame.render_widget(
            Paragraph::new("Resize to at least 40 columns × 12 rows")
                .wrap(Wrap { trim: false })
                .style(style(config, view, ThemeRole::Hints)),
            area,
        );
        return;
    }
    let history = if view.syncing {
        format!(
            "  /  Syncing {}",
            view.progress
                .map(|p| format!("{p}%"))
                .unwrap_or_else(|| "history…".into())
        )
    } else {
        String::new()
    };
    frame.render_widget(
        Paragraph::new(format!(" whatsapp-tui  /  {:?}{history}", view.connection)).style(style(
            config,
            view,
            ThemeRole::Accent,
        )),
        regions.header,
    );
    chat_list::render(frame, regions.chats, view, config);
    timeline::render(frame, regions.messages, view, config, images);
    composer::render(frame, regions.composer, view, config, images);
    let context = match &view.overlay {
        Some(Overlay::Attachment { .. }) => Context::Attachment,
        Some(Overlay::MessageActions(_)) => Context::MessageActions,
        Some(Overlay::MessageLinks(_)) => Context::MessageLinks,
        Some(Overlay::MessageSearch(_)) => Context::MessageSearch,
        Some(Overlay::Search { .. }) => Context::Search,
        Some(Overlay::Help) => Context::Help,
        Some(Overlay::Resend { .. }) => Context::Resend,
        None => match view.focus {
            Focus::Chats => Context::Chats,
            Focus::Messages => Context::Messages,
            Focus::Composer => Context::Composer,
        },
    };
    let mut actions = std::collections::BTreeSet::new();
    let mut bindings = config.bindings.help(context);
    // Keep escape hatches visible even when a custom binding was appended last.
    bindings.sort_by_key(|(_, action)| match action {
        crate::config::bindings::ActionId::Quit => 0,
        crate::config::bindings::ActionId::Help => 1,
        crate::config::bindings::ActionId::MessageActions => 2,
        crate::config::bindings::ActionId::CopyText
        | crate::config::bindings::ActionId::OpenLinks => 3,
        _ => 4,
    });
    let hints = bindings
        .into_iter()
        .filter(|(_, a)| actions.insert(*a))
        .map(|(key, a)| format!("{key} {}", a.label()))
        .collect::<Vec<_>>()
        .join("  ·  ");
    let second =
        view.notice
            .as_deref()
            .or(view.reason.as_deref())
            .unwrap_or(if view.account.is_none() {
                "Link your phone from WhatsApp → Linked devices"
            } else {
                "Local history · drafts saved while you type"
            });
    frame.render_widget(
        Paragraph::new(vec![
            Line::styled(format!(" {hints}"), style(config, view, ThemeRole::Hints)),
            Line::styled(
                format!(" {}", single(second)),
                style(
                    config,
                    view,
                    if view.notice.is_some() {
                        ThemeRole::Error
                    } else {
                        ThemeRole::Inactive
                    },
                ),
            ),
        ]),
        regions.footer,
    );
    if view.qr.is_some() {
        overlays::pairing(frame, area, view, config);
    } else if view.account.is_none() {
        let rect = overlays::centered(area, 52, 5);
        frame.render_widget(ratatui::widgets::Clear, rect);
        frame.render_widget(
            Paragraph::new("Connecting to WhatsApp…\nYour pairing QR will appear here.")
                .block(block(" Welcome ".into(), true, view, config)),
            rect,
        );
    }
    overlays::render(frame, area, view, config);
}
