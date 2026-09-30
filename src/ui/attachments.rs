use super::*;
pub(super) fn dialog(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
    let Some(Overlay::Attachment {
        editor,
        selected,
        importing,
        error,
    }) = &view.overlay
    else {
        return;
    };
    let show_saved = editor.text().is_empty() && !view.draft.recovered.is_empty();
    let count = if show_saved {
        view.draft.recovered.len().min(5) as u16
    } else {
        0
    };
    let rect = overlays::centered(area, 76, 8 + count + u16::from(show_saved));
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
    let status = if importing.is_some() {
        "Preparing image…"
    } else {
        error
            .as_deref()
            .unwrap_or("Enter attaches · Esc cancels · ~/ and spaces supported")
    };
    let status_y = if show_saved {
        frame.render_widget(
            Paragraph::new("Saved drafts · ↑/↓ selects · Enter restores · type a path to attach"),
            Rect::new(inner.x, inner.y + 4, inner.width, 1),
        );
        let start = selected.saturating_sub(count.saturating_sub(1) as usize);
        let rows = view
            .draft
            .recovered
            .iter()
            .enumerate()
            .skip(start)
            .take(count as usize)
            .map(|(i, draft)| {
                let filename = draft
                    .attachment
                    .as_ref()
                    .map(|a| a.filename.as_str())
                    .unwrap_or("Text draft");
                Line::styled(
                    single(&format!(
                        "{} {filename} · {}",
                        if i == *selected { "›" } else { " " },
                        draft.text
                    )),
                    style(
                        config,
                        view,
                        if i == *selected {
                            ThemeRole::Accent
                        } else {
                            ThemeRole::Inactive
                        },
                    ),
                )
            })
            .collect::<Vec<_>>();
        frame.render_widget(
            Paragraph::new(rows),
            Rect::new(
                inner.x,
                inner.y + 5,
                inner.width,
                count.min(inner.height.saturating_sub(5)),
            ),
        );
        5 + count
    } else {
        4
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
            inner.y + status_y,
            inner.width,
            inner.height.saturating_sub(status_y),
        ),
    );
}
