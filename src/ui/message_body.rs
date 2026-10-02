use super::*;
use crate::app::model::*;
pub(super) fn rows(
    message: &MessageRecord,
    view: &ViewModel,
    config: &Config,
    width: usize,
) -> (Vec<Line<'static>>, Option<usize>) {
    let mut lines = Vec::new();
    let mut preview_at = None;
    if let Some(quote) = &message.quote {
        lines.extend(quote_rows(quote, view, config, width));
    }
    let content_start = lines.len();
    let body = match &message.body {
        MessageBody::LocalImage { image, caption } => {
            lines.extend(
                wrap(
                    &format!(
                        "[{}] {} · {}×{}",
                        image.label(),
                        single(&image.filename),
                        image.width,
                        image.height
                    ),
                    width,
                )
                .into_iter()
                .map(Line::from),
            );
            if config.media.inline {
                preview_at = Some(lines.len());
                lines.extend((0..super::images::PREVIEW_ROWS).map(|_| Line::from("")));
            }
            Some(caption.as_str())
        }
        MessageBody::Text(t) => Some(t.as_str()),
        MessageBody::Media(attachment) => {
            if matches!(
                attachment.kind,
                crate::media::AttachmentKind::Audio | crate::media::AttachmentKind::Video
            ) {
                lines.extend(
                    wrap(&audio::label(message, view), width)
                        .into_iter()
                        .map(|s| Line::styled(s, style(config, view, ThemeRole::Accent))),
                );
            } else {
                lines.extend(
                    wrap(&single(&attachment.label()), width)
                        .into_iter()
                        .map(Line::from),
                );
            }
            if config.media.inline && attachment.has_inline_preview() {
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
    let label_end = preview_at.unwrap_or(lines.len());
    if let Some(body) = body {
        lines.extend(rich_text::lines(
            body,
            width,
            style(config, view, ThemeRole::Accent),
            style(config, view, ThemeRole::Inactive),
        ));
    }
    if message.key.from_me
        && let Some(state) = message.send_state
    {
        // Status belongs to the caption or label, never to a quote or a
        // reserved image row that the terminal graphics renderer will cover.
        let target = if body.is_some_and(|text| !text.is_empty()) {
            lines.len() - 1
        } else if label_end > content_start {
            label_end - 1
        } else {
            if lines.len() == content_start {
                lines.push(Line::default());
            }
            lines.len() - 1
        };
        let mark = status_mark(state, view, config);
        let separator = if lines[target].width() > 0 { "  " } else { "" };
        if lines[target].width() + separator.len() + mark.width() <= width {
            lines[target].spans.push(Span::raw(separator));
            lines[target].spans.push(mark);
        } else {
            lines.insert(target + 1, Line::from(mark));
            if let Some(preview) = &mut preview_at
                && *preview > target
            {
                *preview += 1;
            }
        }
    }
    (lines, preview_at)
}

pub(super) fn status_mark(state: SendState, view: &ViewModel, config: &Config) -> Span<'static> {
    let (symbol, role) = match state {
        SendState::Sending => ("…", ThemeRole::Inactive),
        SendState::Sent => ("✓", ThemeRole::Inactive),
        SendState::Delivered => ("✓✓", ThemeRole::Inactive),
        SendState::Read => ("✓✓", ThemeRole::Accent),
        SendState::Failed => ("!", ThemeRole::Error),
        SendState::Unconfirmed => ("?", ThemeRole::Hints),
    };
    Span::styled(symbol, style(config, view, role))
}

pub(super) fn quote_rows(
    quote: &Quote,
    view: &ViewModel,
    config: &Config,
    width: usize,
) -> Vec<Line<'static>> {
    let preview = match quote.availability {
        QuoteAvailability::Available => single(&quote.preview),
        QuoteAvailability::Missing => "[original missing]".into(),
        QuoteAvailability::Unsupported => "[unsupported original]".into(),
        QuoteAvailability::Deleted => "[original deleted]".into(),
        QuoteAvailability::Expired => "[original expired]".into(),
    };
    let author = if quote.key.from_me {
        "You".into()
    } else {
        sender(view, &quote.key.sender.0)
    };
    wrap(&format!("> {author}: {preview}"), width)
        .into_iter()
        .map(|line| Line::styled(line, style(config, view, ThemeRole::Inactive)))
        .collect()
}
