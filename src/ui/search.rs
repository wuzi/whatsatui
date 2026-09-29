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

pub(super) fn messages(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
    let Some(Overlay::MessageSearch(search)) = &view.overlay else {
        return;
    };
    let r = overlays::centered(area, 88, 24);
    frame.render_widget(Clear, r);
    let name = view
        .chats
        .iter()
        .find(|c| c.chat == search.chat)
        .map(|c| single(&c.name))
        .unwrap_or_default();
    let border = block(format!(" Cached messages · {name} "), true, view, config);
    let inner = border.inner(r);
    frame.render_widget(border, r);
    query(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        &search.editor,
        view,
        config,
    );
    let summary = if search.request.is_some() {
        "Searching cached history…".into()
    } else if search.submitted.is_none() {
        "Literal phrase · this conversation".into()
    } else {
        format!(
            "{}{} matches · {}",
            search.page.hits.len(),
            if search.page.has_more { "+" } else { "" },
            if search.page.has_more {
                "refine query"
            } else {
                "newest first"
            }
        )
    };
    frame.render_widget(
        Paragraph::new(summary).style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 3,
        inner.width,
        inner.height.saturating_sub(5),
    );
    let empty = if let Some(error) = &search.error {
        Some(single(error))
    } else if search.request.is_some() {
        Some("Searching…".into())
    } else if search.submitted.is_none() {
        Some("Type a phrase, then submit to search.\nSearches downloaded history only.".into())
    } else if search.page.hits.is_empty() {
        Some("No matches in cached history".into())
    } else {
        None
    };
    if let Some(text) = empty {
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).style(style(
                config,
                view,
                if search.error.is_some() {
                    ThemeRole::Error
                } else {
                    ThemeRole::Inactive
                },
            )),
            list,
        );
    } else {
        let items = search
            .page
            .hits
            .iter()
            .map(|hit| {
                let date = chrono::DateTime::from_timestamp_millis(hit.created_at_ms)
                    .map(|d| {
                        d.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()
                    })
                    .unwrap_or_else(|| "Unknown date".into());
                let who = if hit.key.from_me {
                    "You".into()
                } else {
                    sender(view, &hit.key.sender.0)
                };
                let mut lines = vec![Line::styled(
                    format!("{date} · {who}"),
                    style(config, view, ThemeRole::Inactive),
                )];
                lines.extend(
                    wrap(
                        &single(&hit.preview),
                        inner.width.saturating_sub(2) as usize,
                    )
                    .into_iter()
                    .take(2)
                    .map(Line::from),
                );
                ListItem::new(lines)
            })
            .collect::<Vec<_>>();
        let mut state = ListState::default().with_selected(Some(search.selected));
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
            "{} search/open · {} back",
            key(config, Context::MessageSearch, ActionId::Open),
            key(config, Context::MessageSearch, ActionId::Back)
        ))
        .style(style(config, view, ThemeRole::Hints)),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}
