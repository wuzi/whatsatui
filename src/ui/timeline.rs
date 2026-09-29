use super::*;
use crate::app::model::*;
pub(super) fn render(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
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
    for (index, message) in view.messages.iter().enumerate().take(selected + 1) {
        let start = lines.len();
        lines.extend(message_rows(
            message,
            index == selected,
            view,
            config,
            inner.width as usize,
        ));
        if index == selected {
            chosen = (start, lines.len());
        }
    }
    let height = inner.height as usize;
    let visible = if chosen.1 - chosen.0 > height && height > 1 {
        let mut v = vec![lines[chosen.0].clone()];
        let max_scroll = (chosen.1 - chosen.0).saturating_sub(height);
        let end = chosen.1 - view.message_scroll.min(max_scroll);
        v.extend_from_slice(&lines[end.saturating_sub(height - 1)..end]);
        v
    } else {
        lines
            .into_iter()
            .skip(chosen.1.saturating_sub(height))
            .collect()
    };
    frame.render_widget(Paragraph::new(visible), inner);
}

fn message_rows(
    message: &MessageRecord,
    selected: bool,
    view: &ViewModel,
    config: &Config,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
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
    lines
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
    let total = message_rows(message, true, view, config, width).len();
    Some(crate::app::TimelineViewport {
        selected: view.selected_message.clone(),
        max_scroll: total.saturating_sub(height),
        page_rows: height.saturating_sub(1).max(1),
    })
}
