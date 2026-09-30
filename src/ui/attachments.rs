use super::*;
pub(super) fn dialog(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    editor: &crate::app::editor::Editor,
    importing: bool,
    error: Option<&str>,
) {
    let rect = overlays::centered(area, 76, 8);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let border = block(" Attach image ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    let hint = "Image path · PNG, JPEG, WebP · up to 16 MiB";
    frame.render_widget(
        Paragraph::new(hint).style(style(config, view, ThemeRole::Inactive)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    search::query(
        frame,
        Rect::new(inner.x, inner.y + 2, inner.width, 1),
        editor,
        view,
        config,
    );
    let status = if importing {
        "Preparing image…"
    } else {
        error.unwrap_or("Enter attaches · Esc cancels · ~/ and spaces supported")
    };
    frame.render_widget(
        Paragraph::new(single(status))
            .wrap(Wrap { trim: false })
            .style(style(
                config,
                view,
                if error.is_some() {
                    ThemeRole::Error
                } else {
                    ThemeRole::Hints
                },
            )),
        Rect::new(
            inner.x,
            inner.y + 4,
            inner.width,
            inner.height.saturating_sub(4),
        ),
    );
}
