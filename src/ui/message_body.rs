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
    (lines, preview_at)
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
