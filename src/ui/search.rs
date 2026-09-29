use super::*;
use crate::{app::editor::Editor, config::bindings::ActionId};
use ratatui::widgets::{Clear, List, ListItem, ListState};

pub(super) fn key(config: &Config, context: Context, action: ActionId) -> String {
    config
        .bindings
        .help(context)
        .into_iter()
        .find(|(_, a)| *a == action)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "unbound".into())
}

pub(super) fn query(
    frame: &mut Frame,
    area: Rect,
    editor: &Editor,
    view: &ViewModel,
    config: &Config,
) {
    if area.is_empty() {
        return;
    }
    let cursor = single(&editor.text()[..editor.cursor()]).width();
    let offset = cursor.saturating_sub(area.width.saturating_sub(1) as usize);
    frame.render_widget(
        Paragraph::new(single(editor.text()))
            .scroll((0, offset.min(u16::MAX as usize) as u16))
            .style(style(config, view, ThemeRole::Accent)),
        area,
    );
    frame.set_cursor_position((area.x + (cursor - offset) as u16, area.y));
}

pub(super) fn chats(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
    let Some(Overlay::Search {
        editor,
        selected,
        unread_only,
    }) = &view.overlay
    else {
        return;
    };
    let r = overlays::centered(area, 74, 20);
    frame.render_widget(Clear, r);
    let border = block(
        format!(
            " Switch chat · {} ",
            if *unread_only { "Unread" } else { "All" }
        ),
        true,
        view,
        config,
    );
    let inner = border.inner(r);
    frame.render_widget(border, r);
    query(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        editor,
        view,
        config,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} chats · {} All/Unread",
            view.search_results.len(),
            key(config, Context::Search, ActionId::ToggleUnread)
        ))
        .style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 3,
        inner.width,
        inner.height.saturating_sub(5),
    );
    if view.search_results.is_empty() {
        frame.render_widget(
            Paragraph::new("No matching chats").style(style(config, view, ThemeRole::Inactive)),
            list,
        );
    } else {
        let items = view
            .search_results
            .iter()
            .map(|c| {
                let unread = if c.unread > 0 {
                    format!(" · {} unread", c.unread)
                } else {
                    String::new()
                };
                let draft = if c.has_draft { " · draft" } else { "" };
                ListItem::new(vec![
                    Line::from(single(&c.name)),
                    Line::styled(
                        format!(
                            "{}{}{}{}",
                            if c.is_group { "Group" } else { "Direct" },
                            unread,
                            draft,
                            c.phone
                                .as_deref()
                                .map(|p| format!(" · {}", single(p)))
                                .unwrap_or_default()
                        ),
                        style(config, view, ThemeRole::Inactive),
                    ),
                ])
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected(Some(*selected));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("> ")
                .highlight_style(style(config, view, ThemeRole::Accent)),
            list,
            &mut state,
        );
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} open · {} back",
            key(config, Context::Search, ActionId::Open),
            key(config, Context::Search, ActionId::Back)
        ))
        .style(style(config, view, ThemeRole::Hints)),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}
