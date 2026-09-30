use super::*;
use crate::app::model::QuoteAvailability;
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    images: &mut super::Images,
    hits: &mut InteractionMap,
) {
    if area.is_empty() {
        return;
    }
    let title = if let Some(editing) = &view.editing {
        let save = search::key(
            config,
            Context::Composer,
            crate::config::bindings::ActionId::Send,
        );
        let cancel = search::key(
            config,
            Context::Composer,
            crate::config::bindings::ActionId::Back,
        );
        if editing.request.is_some() {
            " Editing · saving… ".into()
        } else {
            format!(" Editing · {save} save · {cancel} cancel ")
        }
    } else if view.draft.recovered.is_empty() {
        " Message ".into()
    } else {
        let key = search::key(
            config,
            Context::Composer,
            crate::config::bindings::ActionId::AttachImage,
        );
        format!(
            " Message · {} saved drafts ({key}) ",
            view.draft.recovered.len()
        )
    };
    let border = block(title, view.focus == Focus::Composer, view, config);
    let mut inner = border.inner(area);
    frame.render_widget(border, area);
    if let Some(image) = view
        .draft
        .attachment
        .as_ref()
        .filter(|_| view.editing.is_none())
    {
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
    if let Some(quote) = view.draft.reply.as_ref().filter(|_| view.editing.is_none()) {
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
    if let Some(error) = view.editing.as_ref().and_then(|e| e.error.as_ref()) {
        frame.render_widget(
            Paragraph::new(single(error)).style(style(config, view, ThemeRole::Accent)),
            Rect::new(inner.x, inner.y, inner.width, inner.height.min(1)),
        );
        inner.y += 1;
        inner.height = inner.height.saturating_sub(1);
    }
    if inner.is_empty() {
        return;
    }
    let text = view
        .editing
        .as_ref()
        .map_or(view.draft.text.as_str(), |e| e.editor.text());
    let layout = editor_layout::TextLayout::new(text, inner.width as usize);
    let (row, col) = layout.position(view.cursor);
    let top = row.saturating_sub(inner.height.saturating_sub(1) as usize);
    if text.is_empty() {
        frame.render_widget(
            Paragraph::new("Write a message…").style(style(config, view, ThemeRole::Inactive)),
            inner,
        );
    } else {
        frame.render_widget(
            Paragraph::new(
                layout
                    .lines
                    .iter()
                    .skip(top)
                    .cloned()
                    .map(Line::from)
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
    }
    for y in 0..inner.height {
        for x in 0..inner.width {
            hits.push(
                Rect::new(inner.x + x, inner.y + y, 1, 1),
                Target::Composer(layout.byte_at(top + y as usize, x as usize)),
            );
        }
    }
    if view.focus == Focus::Composer && view.overlay.is_none() {
        frame.set_cursor_position((inner.x + col as u16, inner.y + (row - top) as u16));
    }
}
