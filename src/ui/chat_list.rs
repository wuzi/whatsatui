use super::*;
use ratatui::widgets::{HighlightSpacing, List, ListItem, ListState};
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    avatars: &mut Avatars,
    hits: &mut InteractionMap,
) {
    if area.is_empty() {
        return;
    }
    let border = block(
        format!(" Chats · {} ", view.chats.len()),
        view.focus == Focus::Chats,
        view,
        config,
    );
    if view.chats.is_empty() {
        frame.render_widget(
            Paragraph::new(
                "No cached chats yet.\nNames and history will appear as your phone syncs.",
            )
            .wrap(Wrap { trim: false })
            .block(border),
            area,
        );
        return;
    }
    let inner = border.inner(area);
    let photo_padding = if config.media.avatars { "     " } else { "" };
    let text_width = (inner.width as usize).saturating_sub(2 + photo_padding.len());
    let items = view
        .chats
        .iter()
        .map(|chat| {
            let unread = if chat.unread > 0 {
                format!(" ({})", chat.unread)
            } else {
                String::new()
            };
            let draft = if chat.has_draft { " · draft" } else { "" };
            let suffix = format!("{unread}{draft}");
            let name = shorten(
                &single(&chat.name),
                text_width.saturating_sub(suffix.width()),
            );
            ListItem::new(vec![
                Line::from(format!("{photo_padding}{name}{suffix}")),
                Line::styled(
                    format!(
                        "{photo_padding}{} {}",
                        if chat.latest_at_ms > 0 {
                            timestamp(chat.latest_at_ms)
                        } else {
                            String::new()
                        },
                        single(&chat.preview)
                    ),
                    style(config, view, ThemeRole::Inactive),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default()
        .with_offset(view.list_offsets.get(&Context::Chats).copied().unwrap_or(0))
        .with_selected(
            view.chats
                .iter()
                .position(|c| Some(&c.chat) == view.chat.as_ref()),
        );
    frame.render_stateful_widget(
        List::new(items)
            .block(border)
            .highlight_symbol("> ")
            .highlight_spacing(HighlightSpacing::Always)
            .highlight_style(style(config, view, ThemeRole::Accent).add_modifier(Modifier::BOLD)),
        area,
        &mut state,
    );
    hits.list(
        Context::Chats,
        inner,
        state.offset(),
        view.chats.iter().map(|_| 2),
        |i| Target::Chat(view.chats[i].chat.clone()),
    );
    if config.media.avatars
        && view.overlay.is_none()
        && view.qr.is_none()
        && let Some(account) = &view.account
    {
        for (row, chat) in view
            .chats
            .iter()
            .skip(state.offset())
            .take(inner.height as usize / 2)
            .enumerate()
        {
            avatars.draw(
                frame,
                crate::avatars::Identity {
                    account: account.clone(),
                    jid: chat.chat.0.clone(),
                },
                &chat.name,
                Rect::new(inner.x + 2, inner.y + row as u16 * 2, 4, 2),
                0,
            );
        }
    }
}

fn shorten(name: &str, width: usize) -> String {
    if name.width() <= width {
        return name.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut short = String::new();
    let mut columns = 0;
    for grapheme in name.graphemes(true) {
        if columns + grapheme.width() >= width {
            break;
        }
        short.push_str(grapheme);
        columns += grapheme.width();
    }
    short.push('…');
    short
}
