use super::*;
use crate::app::model::*;
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
    let name = view
        .chat
        .as_ref()
        .and_then(|id| view.chats.iter().find(|c| &c.chat == id))
        .map(|c| single(&c.name))
        .unwrap_or_default();
    let new = if view.new_messages > 0 {
        " · new messages".into()
    } else {
        String::new()
    };
    let border = block(
        format!(" Messages · {name}{new} "),
        view.focus == Focus::Messages,
        view,
        config,
    );
    let inner = border.inner(area);
    frame.render_widget(border, area);
    if view.messages.is_empty() {
        frame.render_widget(
            Paragraph::new(if view.loading {
                "Loading conversation…"
            } else {
                "No cached messages yet.\nLinked-device history may still be syncing."
            })
            .wrap(Wrap { trim: false })
            .style(style(config, view, ThemeRole::Inactive)),
            inner,
        );
        return;
    }
    let selected = view
        .selected_message
        .as_ref()
        .and_then(|key| view.messages.iter().position(|m| &m.key == key))
        .unwrap_or(view.messages.len() - 1);
    let mut lines = Vec::new();
    let mut chosen = (0, 0);
    let mut previews = Vec::new();
    for (index, message) in view.messages.iter().enumerate().take(selected + 1) {
        let start = lines.len();
        let (rows, preview_at) = message_rows(
            message,
            index == selected,
            view,
            config,
            inner.width as usize,
        );
        if let Some(offset) = preview_at {
            previews.push((start + offset, message));
        }
        lines.extend(rows);
        if index == selected {
            chosen = (start, lines.len());
        }
    }
    let height = inner.height as usize;
    let indexes: Vec<usize> = if chosen.1 - chosen.0 > height && height > 1 {
        let mut indexes = vec![chosen.0];
        let max_scroll = (chosen.1 - chosen.0).saturating_sub(height);
        let end = chosen.1 - view.message_scroll.min(max_scroll);
        indexes.extend(end.saturating_sub(height - 1)..end);
        indexes
    } else {
        (chosen.1.saturating_sub(height)..chosen.1).collect()
    };
    frame.render_widget(
        Paragraph::new(
            indexes
                .iter()
                .map(|i| lines[*i].clone())
                .collect::<Vec<_>>(),
        ),
        inner,
    );
    if view.overlay.is_none() && view.qr.is_none() && view.account.is_some() {
        for (start, message) in previews {
            let span = start..start + super::images::PREVIEW_ROWS as usize;
            if let Some(first) = indexes.iter().position(|i| span.contains(i)) {
                let count = indexes[first..]
                    .iter()
                    .take_while(|i| span.contains(i))
                    .count();
                images.draw(
                    frame,
                    message,
                    Rect::new(inner.x, inner.y + first as u16, inner.width, count as u16),
                    (indexes[first] - start) as u16,
                    Size::new(inner.width, super::images::PREVIEW_ROWS),
                );
            }
        }
    }
}

fn message_rows(
    message: &MessageRecord,
    selected: bool,
    view: &ViewModel,
    config: &Config,
    width: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let mut lines = Vec::new();
    let mut preview_at = None;
    let who = if message.key.from_me {
        "You".into()
    } else {
        sender(view, &message.key.sender.0)
    };
    let status = message
        .send_state
        .map(|s| format!(" · {s:?}"))
        .unwrap_or_default();
    let receipts = view
        .receipts
        .iter()
        .filter(|r| r.key == message.key)
        .collect::<Vec<_>>();
    let receipt_text = if message.key.chat.0.ends_with("@g.us") && !receipts.is_empty() {
        format!(
            " · delivered: {} / read: {}",
            receipts.len(),
            receipts
                .iter()
                .filter(|r| r.state == ReceiptState::Read)
                .count()
        )
    } else {
        String::new()
    };
    let header = format!(
        "{}{} {who}{status}{receipt_text}{}",
        if selected { "> " } else { "  " },
        timestamp(message.created_at_ms),
        if message.edited_at_ms.is_some() {
            " · edited"
        } else {
            ""
        }
    );
    lines.push(Line::styled(
        header,
        style(
            config,
            view,
            if selected {
                ThemeRole::Accent
            } else {
                ThemeRole::Inactive
            },
        ),
    ));
    if let Some(quote) = &message.quote {
        let preview = match quote.availability {
            QuoteAvailability::Available => single(&quote.preview),
            QuoteAvailability::Missing => "[original missing]".into(),
            QuoteAvailability::Unsupported => "[unsupported original]".into(),
            QuoteAvailability::Deleted => "[original deleted]".into(),
            QuoteAvailability::Expired => "[original expired]".into(),
        };
        for line in wrap(&format!("> {preview}"), width) {
            lines.push(Line::styled(line, style(config, view, ThemeRole::Inactive)));
        }
    }
    let body = match &message.body {
        MessageBody::Text(t) => Some(t.as_str()),
        MessageBody::Media(attachment) => {
            lines.extend(
                wrap(&single(&attachment.label()), width)
                    .into_iter()
                    .map(Line::from),
            );
            if config.media.inline
                && matches!(
                    attachment.kind,
                    crate::media::AttachmentKind::Image | crate::media::AttachmentKind::Sticker
                )
            {
                preview_at = Some(lines.len());
                lines.extend((0..super::images::PREVIEW_ROWS).map(|_| Line::from("")));
            }
            attachment.caption.as_deref()
        }
        MessageBody::Unsupported { kind, caption } => {
            lines.extend(
                wrap(&format!("[{}]", single(kind)), width)
                    .into_iter()
                    .map(Line::from),
            );
            caption.as_deref()
        }
        MessageBody::Deleted => {
            lines.push(Line::from("[Message deleted]"));
            None
        }
        MessageBody::Expired => {
            lines.push(Line::from("[Message expired]"));
            None
        }
    };
    if let Some(body) = body {
        lines.extend(rich_text::lines(
            body,
            width,
            style(config, view, ThemeRole::Accent),
            style(config, view, ThemeRole::Inactive),
        ));
    }
    lines.push(Line::from(""));
    (lines, preview_at)
}

pub(super) fn viewport(
    area: Rect,
    view: &ViewModel,
    config: &Config,
) -> Option<crate::app::TimelineViewport> {
    let regions = layout::calculate(area, view.focus);
    if regions.too_small || regions.messages.width < 3 || regions.messages.height < 3 {
        return None;
    }
    let message = view
        .selected_message
        .as_ref()
        .and_then(|key| view.messages.iter().find(|m| &m.key == key))
        .or_else(|| view.messages.last())?;
    let height = regions.messages.height.saturating_sub(2) as usize;
    let width = regions.messages.width.saturating_sub(2) as usize;
    let total = message_rows(message, true, view, config, width).0.len();
    Some(crate::app::TimelineViewport {
        selected: view.selected_message.clone(),
        max_scroll: total.saturating_sub(height),
        page_rows: height.saturating_sub(1).max(1),
    })
}
