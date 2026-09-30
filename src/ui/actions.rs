use super::*;
use crate::app::model::MessageBody;
use crate::{config::bindings::ActionId as A, message_actions};
use ratatui::widgets::{Clear, List, ListItem, ListState};

pub(super) fn menu(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
) {
    let Some(Overlay::MessageActions(menu)) = &view.overlay else {
        return;
    };
    let rect = overlays::centered(area, 64, 11);
    frame.render_widget(Clear, rect);
    let border = block(" Message actions ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    let body = message_actions::text(&menu.message, chrono::Utc::now().timestamp_millis())
        .map(str::to_owned)
        .unwrap_or_else(|| match &menu.message.body {
            MessageBody::Media(attachment) => attachment.label(),
            _ => String::new(),
        });
    frame.render_widget(
        Paragraph::new(single(&body)).style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let items = message_actions::available(&menu.message, chrono::Utc::now().timestamp_millis())
        .into_iter()
        .map(|action| {
            let label = match action {
                A::CopyText => "Copy text",
                A::OpenLinks => "Open links",
                A::DownloadMedia => "Download attachment",
                A::OpenMedia => "Open downloaded file",
                A::Reply => "Reply to message",
                A::Resend => "Resend message",
                _ => "",
            };
            ListItem::new(format!(
                "{label:22} {}",
                search::key(config, Context::MessageActions, action)
            ))
        })
        .collect::<Vec<_>>();
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(4),
    );
    let count = items.len();
    let mut state = ListState::default()
        .with_offset(
            view.list_offsets
                .get(&Context::MessageActions)
                .copied()
                .unwrap_or(0),
        )
        .with_selected(Some(menu.selected));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_symbol("> ")
            .highlight_style(style(config, view, ThemeRole::Accent)),
        list,
        &mut state,
    );
    hits.list(
        Context::MessageActions,
        list,
        state.offset(),
        std::iter::repeat_n(1, count),
        Target::Menu,
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} select · {} back",
            search::key(config, Context::MessageActions, A::Open),
            search::key(config, Context::MessageActions, A::Back)
        ))
        .style(style(config, view, ThemeRole::Hints)),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}

pub(super) fn links(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
) {
    let Some(Overlay::MessageLinks(links)) = &view.overlay else {
        return;
    };
    let rect = overlays::centered(area, 84, 22);
    frame.render_widget(Clear, rect);
    let border = block(" Message links ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    let copy_key = search::key(config, Context::MessageLinks, A::CopyText);
    frame.render_widget(
        Paragraph::new(format!(
            "{} links · choose a destination",
            links.links.len()
        ))
        .style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let detail_height = (inner.height / 2).clamp(2, 5);
    let list_height = inner.height.saturating_sub(detail_height + 3);
    let mut state = ListState::default()
        .with_offset(
            view.list_offsets
                .get(&Context::MessageLinks)
                .copied()
                .unwrap_or(0),
        )
        .with_selected(Some(links.selected));
    let list = Rect::new(inner.x, inner.y + 1, inner.width, list_height);
    frame.render_stateful_widget(
        List::new(links.links.iter().map(|url| ListItem::new(single(url))))
            .highlight_symbol("> ")
            .highlight_style(style(config, view, ThemeRole::Accent)),
        list,
        &mut state,
    );
    hits.list(
        Context::MessageLinks,
        list,
        state.offset(),
        std::iter::repeat_n(1, links.links.len()),
        Target::Menu,
    );
    if let Some(url) = links.links.get(links.selected) {
        let mut rows = wrap(&single(url), inner.width as usize);
        if rows.len() > detail_height as usize {
            rows.truncate(detail_height as usize);
            if let Some(last) = rows.last_mut() {
                *last = format!("… {copy_key} copies the full link");
            }
        }
        frame.render_widget(
            Paragraph::new(rows.into_iter().map(Line::from).collect::<Vec<_>>()),
            Rect::new(
                inner.x,
                inner.y + 1 + list_height,
                inner.width,
                detail_height,
            ),
        );
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} open · {copy_key} copy\n{} back",
            search::key(config, Context::MessageLinks, A::Open),
            search::key(config, Context::MessageLinks, A::Back)
        ))
        .style(style(config, view, ThemeRole::Hints)),
        Rect::new(inner.x, inner.bottom().saturating_sub(2), inner.width, 2),
    );
}
