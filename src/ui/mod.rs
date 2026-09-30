mod actions;
mod avatars;
pub use avatars::Avatars;
mod attachments;
mod chat_list;
mod composer;
mod editor_layout;
mod emoji;
pub mod images;
pub mod interaction;
pub mod layout;
mod message_body;
pub use interaction::InteractionMap;
use interaction::Target;
mod overlays;
mod rich_text;
mod search;
mod stickers;
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
    render_with_media(frame, view, config, images, &mut Avatars::default());
}
pub fn render_with_media(
    frame: &mut Frame,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
    avatars: &mut Avatars,
) {
    let _ = render_interactive(frame, view, config, images, avatars);
}
pub fn render_interactive(
    frame: &mut Frame,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
    avatars: &mut Avatars,
) -> InteractionMap {
    let mut hits = InteractionMap::new(frame.area(), view);
    images.begin_frame();
    avatars.begin_frame();
    render_content(frame, view, config, images, avatars, &mut hits);
    images.end_frame();
    avatars.end_frame();
    hits
}
fn render_content(
    frame: &mut Frame,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
    avatars: &mut Avatars,
    hits: &mut InteractionMap,
) {
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
    let help = Rect::new(
        regions.header.right().saturating_sub(7),
        regions.header.y,
        6,
        1,
    );
    frame.render_widget(
        Paragraph::new(" Help ").style(style(config, view, ThemeRole::Accent)),
        help,
    );
    hits.push(
        help,
        Target::Action(crate::config::bindings::ActionId::Help),
    );
    for (rect, focus) in [
        (regions.chats, Focus::Chats),
        (regions.messages, Focus::Messages),
        (regions.composer, Focus::Composer),
    ] {
        hits.push(rect, Target::Pane(focus));
    }
    timeline::render(frame, regions.messages, view, config, images, avatars, hits);
    chat_list::render(frame, regions.chats, view, config, avatars, hits);
    composer::render(frame, regions.composer, view, config, images, hits);
    let second =
        view.notice
            .as_deref()
            .or(view.reason.as_deref())
            .unwrap_or(if view.account.is_none() {
                "Link your phone from WhatsApp → Linked devices"
            } else {
                ""
            });
    frame.render_widget(
        Paragraph::new(vec![Line::styled(
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
        )]),
        regions.footer,
    );
    if view.qr.is_some() || view.account.is_none() {
        hits.clear();
    }
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
    overlays::render(frame, area, view, config, images, hits);
}
