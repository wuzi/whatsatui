use super::*;
use crate::{
    app::update::attachments::{browser_listed, format_entry_size},
    app::view_model::SendChoiceKind,
    config::bindings::ActionId,
};
pub(super) fn dialog(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
) {
    let Some(Overlay::Attachment {
        editor,
        selected,
        importing,
        error,
    }) = &view.overlay
    else {
        return;
    };
    // Saved drafts come first so old shortcuts keep working; files follow
    // and share the same selection index (offset by the draft count).
    let show_drafts = editor.text().trim().is_empty() && !view.draft.recovered.is_empty();
    let dcount = if show_drafts {
        view.draft.recovered.len()
    } else {
        0
    };
    let (browser_base, browser_entries) = browser_listed(editor.text());
    let file_len = browser_entries.len();
    let saved_count = if show_drafts { dcount.min(5) as u16 } else { 0 };
    let browser_count = file_len.min(8) as u16;
    let extra = 1 + u16::from(show_drafts);
    let rect = overlays::centered(area, 76, 8 + saved_count + browser_count + extra);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let border = block(" Attach image or file ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    let hint = "↑/↓ or click selects · Enter or double-click opens/attaches · up to 50 MiB";
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
        hits,
    );
    let status = if importing.is_some() {
        "Preparing file…"
    } else {
        error
            .as_deref()
            .unwrap_or("Enter attaches · Esc cancels · ~/ and spaces supported")
    };
    let mut row = 4u16;
    if show_drafts {
        frame.render_widget(
            Paragraph::new("Saved drafts · ↑/↓ selects · Enter restores"),
            Rect::new(inner.x, inner.y + row, inner.width, 1),
        );
        let dsel = (*selected).min(dcount.saturating_sub(1));
        let dstart = view
            .list_offsets
            .get(&Context::Attachment)
            .copied()
            .unwrap_or(0)
            .min(dsel)
            .max(
                dsel.saturating_add(1)
                    .saturating_sub(saved_count.max(1) as usize),
            );
        let rows = view
            .draft
            .recovered
            .iter()
            .enumerate()
            .skip(dstart)
            .take(saved_count as usize)
            .map(|(j, draft)| {
                let filename = draft
                    .attachment
                    .as_ref()
                    .map(|a| a.filename.as_str())
                    .or(draft.document.as_ref().map(|d| d.filename.as_str()))
                    .unwrap_or("Text draft");
                Line::styled(
                    single(&format!(
                        "{} {filename} · {}",
                        if j == *selected { "›" } else { " " },
                        draft.text
                    )),
                    style(
                        config,
                        view,
                        if j == *selected {
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
                inner.y + row + 1,
                inner.width,
                saved_count.min(inner.height.saturating_sub(row + 1)),
            ),
        );
        hits.list(
            Context::Attachment,
            Rect::new(
                inner.x,
                inner.y + row + 1,
                inner.width,
                saved_count.min(inner.height.saturating_sub(row + 1)),
            ),
            dstart,
            view.draft.recovered.iter().map(|_| 1),
            Target::Menu,
        );
        row += 1 + saved_count;
    }
    frame.render_widget(
        Paragraph::new(format!("Files in {} · .. goes up", browser_base.display())).style(style(
            config,
            view,
            ThemeRole::Inactive,
        )),
        Rect::new(inner.x, inner.y + row, inner.width, 1),
    );
    // Selection inside the drafts region keeps the file window on its top
    // rows; selection inside the files drives the window normally.
    let fsel = selected
        .saturating_sub(dcount)
        .min(file_len.saturating_sub(1));
    let start = view
        .list_offsets
        .get(&Context::Attachment)
        .copied()
        .unwrap_or(0)
        .min(fsel)
        .max(
            fsel.saturating_add(1)
                .saturating_sub(browser_count.max(1) as usize),
        );
    let rows = browser_entries
        .iter()
        .enumerate()
        .skip(start)
        .take(browser_count as usize)
        .map(|(i, entry)| {
            let label = if entry.is_dir {
                format!("{}/", single(&entry.name))
            } else {
                format!(
                    "{} · {}",
                    single(&entry.name),
                    format_entry_size(entry.size)
                )
            };
            Line::styled(
                single(&format!(
                    "{} {label}",
                    if dcount + i == *selected { "›" } else { " " },
                )),
                style(
                    config,
                    view,
                    if dcount + i == *selected {
                        ThemeRole::Accent
                    } else {
                        ThemeRole::Inactive
                    },
                ),
            )
        })
        .collect::<Vec<_>>();
    let rows_text = if rows.is_empty() {
        vec![Line::styled(
            "No matching files here · type another path or pick ..",
            style(config, view, ThemeRole::Inactive),
        )]
    } else {
        rows
    };
    frame.render_widget(
        Paragraph::new(rows_text),
        Rect::new(
            inner.x,
            inner.y + row + 1,
            inner.width,
            browser_count.min(inner.height.saturating_sub(row + 1)),
        ),
    );
    if !browser_entries.is_empty() {
        hits.list(
            Context::Attachment,
            Rect::new(
                inner.x,
                inner.y + row + 1,
                inner.width,
                browser_count.min(inner.height.saturating_sub(row + 1)),
            ),
            start,
            browser_entries.iter().map(|_| 1),
            |i| Target::Menu(i + dcount),
        );
    }
    let status_y = row + 1 + browser_count;
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
pub(super) fn choice(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
    choice: &Overlay,
) {
    let Overlay::SendChoice {
        kind,
        filename,
        selected,
    } = choice
    else {
        return;
    };
    let kind = *kind;
    let selected = *selected;
    let filename = filename.as_str();
    let (title, playable) = match kind {
        SendChoiceKind::Audio => (
            " Send music as… ".into(),
            "Music · plays here and on the phone",
        ),
        SendChoiceKind::Video => (
            " Send video as… ".into(),
            "Video · plays here and on the phone",
        ),
    };
    let rect = overlays::centered(area, 64, 8);
    frame.render_widget(ratatui::widgets::Clear, rect);
    let border = block(title, true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    frame.render_widget(
        Paragraph::new(single(filename)).style(style(config, view, ThemeRole::Accent)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let rows = [playable, "File · plain attachment"]
        .into_iter()
        .enumerate()
        .map(|(i, label)| {
            Line::styled(
                single(&format!(
                    "{} {label}",
                    if i == selected { "›" } else { " " }
                )),
                style(
                    config,
                    view,
                    if i == selected {
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
        Rect::new(inner.x, inner.y + 2, inner.width, 2),
    );
    hits.list(
        Context::Attachment,
        Rect::new(inner.x, inner.y + 2, inner.width, 2),
        0,
        [1, 1],
        Target::Menu,
    );
    let open = search::key(config, Context::Attachment, ActionId::Open);
    let back = search::key(config, Context::Attachment, ActionId::Back);
    frame.render_widget(
        Paragraph::new(format!("{open} send · {back} file")).style(style(
            config,
            view,
            ThemeRole::Hints,
        )),
        Rect::new(inner.x, inner.y + 5, inner.width, 1),
    );
}
