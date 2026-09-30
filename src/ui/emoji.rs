use super::*;
use ratatui::widgets::{Clear, List, ListItem, ListState};
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    editor: &crate::app::editor::Editor,
    selected: usize,
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
    let border = block(
        " Emoji · search names or :shortcodes: ".into(),
        true,
        view,
        config,
    );
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    search::query(
        frame,
        Rect::new(inner.x, inner.y, inner.width, 1),
        editor,
        view,
        config,
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
        let rows = results.into_iter().map(|emoji| {
            ListItem::new(format!(
                "{}  :{}:  {}",
                emoji.as_str(),
                emoji.shortcode().unwrap_or("emoji"),
                emoji.name()
            ))
        });
        frame.render_stateful_widget(
            List::new(rows)
                .highlight_symbol("> ")
                .highlight_style(style(config, view, ThemeRole::Accent)),
            list,
            &mut ListState::default().with_selected(Some(selected)),
        );
    }
    frame.render_widget(
        Paragraph::new(format!(
            "{} insert · {} close",
            search::key(
                config,
                Context::Emoji,
                crate::config::bindings::ActionId::Open
            ),
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
