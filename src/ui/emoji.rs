use super::*;
use ratatui::widgets::{Clear, List, ListItem, ListState};
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    editor: &crate::app::editor::Editor,
    selected: usize,
    hits: &mut InteractionMap,
) {
    let regions = layout::calculate(area, view.focus);
    let right = if regions.composer.is_empty() {
        area
    } else {
        regions.composer
    };
    let bottom = right.y.max(area.y + 6);
    let height = (bottom - area.y).min(13);
    let rect = Rect::new(right.x, bottom - height, right.width, height);
    frame.render_widget(Clear, rect);
    let reacting = matches!(
        view.overlay,
        Some(Overlay::Emoji {
            target: Some(_),
            ..
        })
    );
    let border = block(
        if reacting {
            " React · search emoji "
        } else {
            " Emoji · search names or :shortcodes: "
        }
        .into(),
        true,
        view,
        config,
    );
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    search::query(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        editor,
        view,
        config,
        hits,
    );
    let results = crate::app::emoji::search(editor.text());
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    if results.is_empty() {
        frame.render_widget(Paragraph::new("No emoji found"), list);
    } else {
        let count = results.len();
        let rows = results.into_iter().map(|emoji| {
            ListItem::new(format!(
                "{}  :{}:  {}",
                emoji.as_str(),
                emoji.shortcode().unwrap_or("emoji"),
                emoji.name()
            ))
        });
        let mut state = ListState::default()
            .with_offset(view.list_offsets.get(&Context::Emoji).copied().unwrap_or(0))
            .with_selected(Some(selected));
        frame.render_stateful_widget(
            List::new(rows)
                .highlight_symbol("> ")
                .highlight_style(style(config, view, ThemeRole::Accent)),
            list,
            &mut state,
        );
        hits.list(
            Context::Emoji,
            list,
            state.offset(),
            std::iter::repeat_n(1, count),
            Target::Menu,
        );
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} {} · {} close",
            search::key(
                config,
                Context::Emoji,
                crate::config::bindings::ActionId::Open
            ),
            if reacting {
                "react (same removes)"
            } else {
                "insert"
            },
            search::key(
                config,
                Context::Emoji,
                crate::config::bindings::ActionId::Back
            )
        ))
        .style(style(config, view, ThemeRole::Hints)),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}
