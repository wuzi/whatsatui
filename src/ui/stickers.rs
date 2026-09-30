use super::*;
use crate::{
    app::view_model::{StickerChoice, StickerPicker},
    config::bindings::ActionId,
};

pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    picker: &StickerPicker,
    images: &mut Images,
    hits: &mut InteractionMap,
) {
    let rect = overlays::centered(area, 76, 18);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let border = block(" Stickers · recent & pasted ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    if inner.height < 5 {
        return;
    }
    let paste = overlays::key(config, Context::Stickers, ActionId::PasteClipboard);
    let send = overlays::key(config, Context::Stickers, ActionId::Open);
    let back = overlays::key(config, Context::Stickers, ActionId::Back);
    frame.render_widget(
        Paragraph::new(format!("{paste} creates · {send} sends · {back} cancels")),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let status = if picker.loading.is_some() {
        "Loading recent stickers…"
    } else if picker.sending.is_some() {
        "Preparing sticker…"
    } else {
        picker
            .error
            .as_deref()
            .unwrap_or(if picker.items.is_empty() {
                "No recent stickers. Copy an image, then paste here."
            } else {
                "Your message draft stays intact."
            })
    };
    frame.render_widget(
        Paragraph::new(single(status))
            .wrap(Wrap { trim: false })
            .style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y + 1, inner.width, 2),
    );
    let available = inner.height.saturating_sub(4);
    let left_width = (inner.width / 2).saturating_sub(1);
    let start = view
        .list_offsets
        .get(&Context::Stickers)
        .copied()
        .unwrap_or(0)
        .min(picker.selected)
        .max((picker.selected + 1).saturating_sub(available.max(1) as usize));
    let lines = picker
        .items
        .iter()
        .enumerate()
        .skip(start)
        .take(available as usize)
        .map(|(i, item)| {
            let label = match item {
                StickerChoice::Local(_) => "Prepared sticker".into(),
                StickerChoice::Recent(m) => format!(
                    "{} · {}",
                    if m.key.from_me {
                        "You".into()
                    } else {
                        sender(view, &m.key.sender.0)
                    },
                    timestamp(m.created_at_ms)
                ),
            };
            Line::styled(
                format!(
                    "{} {}",
                    if i == picker.selected { "›" } else { " " },
                    single(&label)
                ),
                style(
                    config,
                    view,
                    if i == picker.selected {
                        ThemeRole::Accent
                    } else {
                        ThemeRole::Text
                    },
                ),
            )
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines),
        Rect::new(inner.x, inner.y + 4, left_width, available),
    );
    hits.list(
        Context::Stickers,
        Rect::new(inner.x, inner.y + 4, left_width, available),
        start,
        picker.items.iter().map(|_| 1),
        Target::Menu,
    );
    let preview = Rect::new(
        inner.x + left_width + 1,
        inner.y + 4,
        inner.width - left_width - 1,
        available.min(10),
    );
    if picker.loading.is_some() {
        return;
    }
    match picker.items.get(picker.selected) {
        Some(StickerChoice::Recent(message)) => {
            if message
                .expires_at_ms
                .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
            {
                return;
            }
            images.draw(frame, message, preview, 0, preview.as_size())
        }
        Some(StickerChoice::Local(image)) => {
            if let (Some(account), Some(chat)) = (&view.account, &view.chat) {
                images.draw_local(frame, account, chat, image, preview);
            }
        }
        None => {}
    }
}
