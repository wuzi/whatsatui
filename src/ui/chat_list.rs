use super::*;
use ratatui::widgets::{List, ListItem, ListState};
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
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
            ListItem::new(vec![
                Line::from(format!("{}{unread}{draft}", single(&chat.name))),
                Line::styled(
                    format!(
                        "{} {}",
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
    let inner = border.inner(area);
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
}
