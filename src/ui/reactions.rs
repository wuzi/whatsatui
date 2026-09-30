use super::*;
use crate::{app::model::MessageRecord, config::bindings::ActionId as A};
use ratatui::widgets::Clear;

pub(super) fn summary(
    message: &MessageRecord,
    view: &ViewModel,
    config: &Config,
    width: usize,
) -> Vec<Line<'static>> {
    let mut groups = std::collections::BTreeMap::<&str, (usize, bool)>::new();
    for reaction in view
        .interactions
        .reactions
        .iter()
        .filter(|r| r.key == message.key)
    {
        let group = groups.entry(&reaction.emoji).or_default();
        group.0 += 1;
        group.1 |= reaction.reactor.0 == message.key.account.0;
    }
    let mut rows = vec![];
    let mut spans = vec![];
    let mut used = 0;
    for (emoji, (count, mine)) in groups {
        let label = format!(
            "{} {count}{}",
            single(emoji),
            if mine { " · You" } else { "" }
        );
        let size = unicode_width::UnicodeWidthStr::width(label.as_str());
        if used > 0 && used + size + 2 > width {
            rows.push(Line::from(std::mem::take(&mut spans)));
            used = 0;
        }
        if used > 0 {
            spans.push(Span::raw("  "));
            used += 2;
        }
        spans.push(Span::styled(
            label,
            style(
                config,
                view,
                if mine {
                    ThemeRole::Accent
                } else {
                    ThemeRole::Inactive
                },
            ),
        ));
        used += size;
    }
    if !spans.is_empty() {
        rows.push(Line::from(spans));
    }
    rows
}
pub(super) fn popup(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
) {
    let Some(Overlay::Reactions(menu)) = &view.overlay else {
        return;
    };
    let rect = overlays::centered(area, 72, 22);
    frame.render_widget(Clear, rect);
    let border = block(" Reactions ".into(), true, view, config);
    let inner = border.inner(rect);
    frame.render_widget(border, rect);
    hits.popup(frame, rect, view, config);
    let react = search::key(config, Context::Reactions, A::React);
    let remove = search::key(config, Context::Reactions, A::RemoveReaction);
    let rows = view
        .interactions
        .reactions
        .iter()
        .filter(|r| r.key == menu.message.key)
        .collect::<Vec<_>>();
    let mine = rows
        .iter()
        .any(|r| r.reactor.0 == menu.message.key.account.0);
    let button = Rect::new(inner.x, inner.y, inner.width, inner.height.min(1));
    frame.render_widget(
        Paragraph::new(format!("[{react}] React / change reaction")).style(style(
            config,
            view,
            ThemeRole::Accent,
        )),
        button,
    );
    hits.push(button, Target::Action(A::React));
    let remove_row = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1).min(1),
    );
    if mine {
        frame.render_widget(
            Paragraph::new(format!("[{remove}] Remove mine")),
            remove_row,
        );
        hits.push(remove_row, Target::Action(A::RemoveReaction));
    }
    let list = Rect::new(
        inner.x,
        inner.y + 3,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let text = if rows.is_empty() {
        vec![Line::from("No reactions yet")]
    } else {
        rows.into_iter()
            .skip(menu.selected)
            .map(|r| {
                Line::from(format!(
                    "{}  {}",
                    single(&r.emoji),
                    if r.reactor.0 == menu.message.key.account.0 {
                        "You".into()
                    } else {
                        sender(view, &r.reactor.0)
                    }
                ))
            })
            .collect()
    };
    frame.render_widget(Paragraph::new(text), list);
}
