use super::*;
use crate::app::model::*;

struct Run {
    index: usize,
    start: usize,
    end: usize,
    avatar: bool,
    preview: Option<usize>,
}
pub(super) struct Timeline {
    pub area: Rect,
    lines: Vec<Line<'static>>,
    runs: Vec<Run>,
    visible: Vec<usize>,
    sticky: Option<usize>,
    gutter: u16,
    anchor_end: usize,
}
fn day(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms)
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown date".into())
}
fn grouped(previous: &MessageRecord, current: &MessageRecord) -> bool {
    previous.key.sender == current.key.sender
        && previous.key.from_me == current.key.from_me
        && previous.key.chat == current.key.chat
        && current.created_at_ms >= previous.created_at_ms
        && current.created_at_ms.saturating_sub(previous.created_at_ms) < 300_000
        && day(previous.created_at_ms) == day(current.created_at_ms)
}
fn who(message: &MessageRecord, view: &ViewModel) -> String {
    if message.key.from_me {
        "You".into()
    } else {
        sender(view, &message.key.sender.0)
    }
}
fn header(
    message: &MessageRecord,
    show_name: bool,
    width: usize,
    view: &ViewModel,
    config: &Config,
) -> Line<'static> {
    let mut details = timestamp(message.created_at_ms);
    if let Some(state) = message.send_state {
        details.push_str(&format!(" · {state:?}"));
    }
    if message.edited_at_ms.is_some() {
        details.push_str(" · edited");
    }
    if message.key.chat.0.ends_with("@g.us") {
        let receipts: Vec<_> = view
            .receipts
            .iter()
            .filter(|r| r.key == message.key)
            .collect();
        if !receipts.is_empty() {
            details.push_str(&format!(
                " · delivered: {} / read: {}",
                receipts.len(),
                receipts
                    .iter()
                    .filter(|r| r.state == ReceiptState::Read)
                    .count()
            ));
        }
    }
    let mut spans = vec![];
    if show_name {
        // Keep timing/status visible even when the contact has a long name.
        let name = who(message, view);
        let budget = width.saturating_sub(details.width() + 2).max(3).min(width);
        let name = if name.width() > budget {
            let mut short = String::new();
            let mut columns = 0;
            for grapheme in name.graphemes(true) {
                if columns + grapheme.width() > budget.saturating_sub(1) {
                    break;
                }
                short.push_str(grapheme);
                columns += grapheme.width();
            }
            short.push('…');
            short
        } else {
            name
        };
        spans.push(Span::styled(
            format!("{name}  "),
            style(
                config,
                view,
                if message.key.from_me {
                    ThemeRole::Own
                } else {
                    ThemeRole::Accent
                },
            )
            .add_modifier(Modifier::BOLD),
        ));
    }
    spans.push(Span::styled(
        details,
        style(config, view, ThemeRole::Inactive),
    ));
    Line::from(spans)
}
fn gutter(
    mut line: Line<'static>,
    message: &MessageRecord,
    first: bool,
    view: &ViewModel,
    config: &Config,
    width: u16,
) -> Line<'static> {
    let selected = view.selected_message.as_ref() == Some(&message.key);
    let marker = if selected {
        if first { "▸" } else { "┃" }
    } else {
        " "
    };
    let mut spans = vec![
        Span::styled(marker, style(config, view, ThemeRole::Focus)),
        Span::styled(
            if message.key.from_me { "│" } else { " " },
            style(config, view, ThemeRole::Own),
        ),
        Span::raw(" ".repeat(width.saturating_sub(2) as usize)),
    ];
    spans.append(&mut line.spans);
    Line::from(spans)
}
fn inner(area: Rect, config: &Config) -> Rect {
    let mut rect = area.inner(Margin::new(1, 1));
    if config.media.avatars && rect.height >= 7 {
        rect.y += 3;
        rect.height -= 3;
    }
    rect
}
pub(super) fn layout(area: Rect, view: &ViewModel, config: &Config) -> Timeline {
    let area = inner(area, config);
    let gutter_width = if config.media.avatars && area.width >= 20 {
        7
    } else {
        2
    };
    let width = area.width.saturating_sub(gutter_width).max(1) as usize;
    let mut lines = vec![];
    let mut runs = vec![];
    for (index, message) in view.messages.iter().enumerate() {
        let previous = index.checked_sub(1).map(|i| &view.messages[i]);
        let new_day = previous.is_none_or(|p| day(p.created_at_ms) != day(message.created_at_ms));
        let avatar = previous.is_none_or(|p| !grouped(p, message));
        if index > 0 && avatar {
            lines.push(Line::from(""));
        }
        if new_day {
            lines.push(
                Line::styled(
                    format!("── {} ──", day(message.created_at_ms)),
                    style(config, view, ThemeRole::Inactive),
                )
                .centered(),
            );
        }
        let start = lines.len();
        lines.push(gutter(
            header(message, avatar, width, view, config),
            message,
            true,
            view,
            config,
            gutter_width,
        ));
        let (body, preview) = message_body::rows(message, view, config, width);
        let preview = preview.map(|offset| lines.len() + offset);
        for line in body {
            lines.push(gutter(line, message, false, view, config, gutter_width));
        }
        if lines.len() == start + 1 {
            lines.push(gutter(
                Line::from(""),
                message,
                false,
                view,
                config,
                gutter_width,
            ));
        }
        runs.push(Run {
            index,
            start,
            end: lines.len(),
            avatar,
            preview,
        });
    }
    let anchor_end = view
        .timeline_anchor
        .as_ref()
        .and_then(|key| runs.iter().find(|r| &view.messages[r.index].key == key))
        .map_or(lines.len(), |r| r.end)
        .max((area.height as usize).min(lines.len()));
    let height = area.height as usize;
    let max_scroll = anchor_end.saturating_sub(height);
    let end = anchor_end.saturating_sub(view.message_scroll.min(max_scroll));
    let start = end.saturating_sub(height);
    let mut visible: Vec<_> = (start..end).collect();
    let sticky = if height > 1 {
        runs.iter()
            .find(|r| r.start <= start && start < r.end && (r.start < start || !r.avatar))
            .map(|r| r.index)
    } else {
        None
    };
    if let Some(index) = sticky
        && let Some(row) = visible.first_mut()
    {
        *row = runs[index].start;
    }
    Timeline {
        area,
        lines,
        runs,
        visible,
        sticky,
        gutter: gutter_width,
        anchor_end,
    }
}
impl Timeline {
    fn portion(&self, start: usize, height: usize) -> Option<(Rect, u16)> {
        let span = start..start + height;
        let first = self.visible.iter().position(|row| span.contains(row))?;
        let count = self.visible[first..]
            .iter()
            .enumerate()
            .take_while(|(n, row)| span.contains(row) && **row == self.visible[first] + n)
            .count();
        Some((
            Rect::new(
                self.area.x,
                self.area.y + first as u16,
                self.area.width,
                count as u16,
            ),
            (self.visible[first] - start) as u16,
        ))
    }
}
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
    avatars: &mut Avatars,
    hits: &mut InteractionMap,
) {
    if area.is_empty() {
        return;
    }
    let chat = view
        .chat
        .as_ref()
        .and_then(|id| view.chats.iter().find(|c| &c.chat == id));
    let name = chat.map(|c| single(&c.name)).unwrap_or_default();
    let new = if view.new_messages > 0 {
        " · new messages"
    } else {
        ""
    };
    let border = block(
        format!(" Messages · {name}{new} "),
        view.focus == Focus::Messages,
        view,
        config,
    );
    let border_inner = border.inner(area);
    frame.render_widget(border, area);
    let layout = layout(area, view, config);
    for (y, row) in layout.visible.iter().enumerate() {
        if let Some(run) = layout.runs.iter().find(|r| r.start <= *row && *row < r.end) {
            hits.push(
                Rect::new(
                    layout.area.x,
                    layout.area.y + y as u16,
                    layout.area.width,
                    1,
                ),
                Target::Message(view.messages[run.index].key.clone()),
            );
        }
    }
    let graphics = view.overlay.is_none() && view.qr.is_none() && view.account.is_some();
    if layout.area.y > border_inner.y {
        let title = Rect::new(
            border_inner.x + 6,
            border_inner.y,
            border_inner.width.saturating_sub(6),
            2,
        );
        frame.render_widget(
            Paragraph::new(vec![
                Line::styled(
                    name.clone(),
                    style(config, view, ThemeRole::Accent).add_modifier(Modifier::BOLD),
                ),
                Line::styled(
                    if chat.is_some_and(|c| c.is_group) {
                        "Group conversation"
                    } else {
                        "Direct conversation"
                    },
                    style(config, view, ThemeRole::Inactive),
                ),
            ]),
            title,
        );
        if graphics && let (Some(account), Some(chat)) = (&view.account, &view.chat) {
            avatars.draw(
                frame,
                crate::avatars::Identity {
                    account: account.clone(),
                    jid: chat.0.clone(),
                },
                &name,
                Rect::new(border_inner.x + 1, border_inner.y, 4, 2),
                0,
            );
        }
    }
    if view.messages.is_empty() {
        frame.render_widget(
            Paragraph::new(if view.loading {
                "Loading conversation…"
            } else {
                "No cached messages yet.\nLinked-device history may still be syncing."
            })
            .wrap(Wrap { trim: false })
            .style(style(config, view, ThemeRole::Inactive)),
            layout.area,
        );
        return;
    }
    let mut visible = layout
        .visible
        .iter()
        .map(|row| layout.lines[*row].clone())
        .collect::<Vec<_>>();
    if let Some(index) = layout.sticky
        && let Some(first) = visible.first_mut()
    {
        *first = gutter(
            header(
                &view.messages[index],
                true,
                layout.area.width.saturating_sub(layout.gutter) as usize,
                view,
                config,
            ),
            &view.messages[index],
            true,
            view,
            config,
            layout.gutter,
        );
    }
    frame.render_widget(Paragraph::new(visible), layout.area);
    if !graphics {
        return;
    }
    for run in &layout.runs {
        let message = &view.messages[run.index];
        if let Some(start) = run.preview
            && let Some((mut rect, skip)) = layout.portion(start, images::PREVIEW_ROWS as usize)
        {
            rect.x += layout.gutter;
            rect.width = rect.width.saturating_sub(layout.gutter);
            images.draw(
                frame,
                message,
                rect,
                skip,
                Size::new(rect.width, images::PREVIEW_ROWS),
            );
        }
        if config.media.avatars
            && layout.gutter >= 7
            && run.avatar
            && let Some((rect, skip)) = layout.portion(run.start, 2)
        {
            let jid = if message.key.from_me {
                message.key.account.0.clone()
            } else {
                message.key.sender.0.clone()
            };
            avatars.draw(
                frame,
                crate::avatars::Identity {
                    account: message.key.account.clone(),
                    jid,
                },
                &who(message, view),
                Rect::new(rect.x + 2, rect.y, 4, rect.height),
                skip,
            );
        }
    }
}
pub(super) fn viewport(
    area: Rect,
    view: &ViewModel,
    config: &Config,
) -> Option<crate::app::TimelineViewport> {
    let regions = layout::calculate(area, view.focus);
    if regions.too_small
        || regions.messages.width < 3
        || regions.messages.height < 3
        || view.messages.is_empty()
    {
        return None;
    }
    let layout = layout(regions.messages, view, config);
    Some(crate::app::TimelineViewport {
        selected: view.selected_message.clone(),
        anchor: view.timeline_anchor.clone(),
        max_scroll: layout
            .anchor_end
            .saturating_sub(layout.area.height as usize),
        page_rows: (layout.area.height as usize).saturating_sub(1).max(1),
        tail_rows: layout.lines.len().saturating_sub(layout.anchor_end),
        fully_visible: layout
            .runs
            .iter()
            .filter(|r| (r.start..r.end).all(|n| layout.visible.contains(&n)))
            .map(|r| view.messages[r.index].key.clone())
            .collect(),
    })
}
