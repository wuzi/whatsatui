use super::*;
use crate::app::model::QuoteAvailability;
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    images: &mut super::Images,
) {
    if area.is_empty() {
        return;
    }
    let border = block(
        " Message ".into(),
        view.focus == Focus::Composer,
        view,
        config,
    );
    let mut inner = border.inner(area);
    frame.render_widget(border, area);
    if let Some(image) = &view.draft.attachment {
        let remove = search::key(
            config,
            Context::Composer,
            crate::config::bindings::ActionId::RemoveAttachment,
        );
        frame.render_widget(
            Paragraph::new(format!(
                "[image] {} · {remove} remove",
                single(&image.filename)
            ))
            .style(style(config, view, ThemeRole::Accent)),
            Rect::new(inner.x, inner.y, inner.width, inner.height.min(1)),
        );
        inner.y += 1;
        inner.height = inner.height.saturating_sub(1);
        if config.media.inline
            && view.overlay.is_none()
            && inner.height >= 2
            && inner.width >= 30
            && let (Some(account), Some(chat)) = (&view.account, &view.chat)
        {
            let preview = Rect::new(inner.right() - 12, inner.y, 12, inner.height);
            images.draw_local(frame, account, chat, image, preview);
            inner.width = inner.width.saturating_sub(14);
        }
    }
    if let Some(quote) = &view.draft.reply {
        let preview = if quote.availability == QuoteAvailability::Available {
            single(&quote.preview)
        } else {
            format!("original {:?}", quote.availability).to_lowercase()
        };
        frame.render_widget(
            Paragraph::new(format!("↪ {preview}")).style(style(config, view, ThemeRole::Inactive)),
            Rect::new(inner.x, inner.y, inner.width, 1),
        );
        inner.y += 1;
        inner.height = inner.height.saturating_sub(1);
    }
    if inner.is_empty() {
        return;
    }
    let text = safe_text(&view.draft.text);
    let lines = wrap(&text, inner.width as usize);
    let prefix = safe_text(&view.draft.text[..view.cursor.min(view.draft.text.len())]);
    let prefix_lines = wrap(&prefix, inner.width as usize);
    let mut row = prefix_lines.len().saturating_sub(1);
    let mut col = prefix_lines.last().map_or(0, |s| s.width());
    if col >= inner.width as usize {
        row += 1;
        col = 0;
    }
    let top = row.saturating_sub(inner.height.saturating_sub(1) as usize);
    if text.is_empty() {
        frame.render_widget(
            Paragraph::new("Write a message…").style(style(config, view, ThemeRole::Inactive)),
            inner,
        );
    } else {
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .skip(top)
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
    }
    if view.focus == Focus::Composer && view.overlay.is_none() {
        frame.set_cursor_position((inner.x + col as u16, inner.y + (row - top) as u16));
    }
}
