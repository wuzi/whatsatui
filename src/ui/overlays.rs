use super::*;
use crate::app::model::SendState;
use ratatui::widgets::Clear;
pub(super) fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}
pub(super) fn pairing(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
    let Some((content, expires)) = &view.qr else {
        return;
    };
    if view.now >= *expires {
        let r = centered(area, 54, 5);
        frame.render_widget(Clear, r);
        frame.render_widget(
            Paragraph::new("Pairing code expired.\nWaiting for a fresh QR…").block(block(
                " Link WhatsApp ".into(),
                true,
                view,
                config,
            )),
            r,
        );
        return;
    }
    let Ok(qr) = qrcode::QrCode::new(content) else {
        return;
    };
    let size = qr.width() + 8;
    let height = size.div_ceil(2) + 5;
    if size + 2 > area.width as usize || height > area.height as usize {
        let r = centered(area, area.width.saturating_sub(2), 5);
        frame.render_widget(Clear, r);
        frame.render_widget(
            Paragraph::new(format!(
                "Resize to {} × {} to scan the full QR.\nWhatsApp → Linked devices",
                size + 2,
                height
            ))
            .wrap(Wrap { trim: false })
            .block(block(" Link WhatsApp ".into(), true, view, config)),
            r,
        );
        return;
    }
    let r = centered(area, (size + 2) as u16, height as u16);
    frame.render_widget(Clear, r);
    let border = block(" Link WhatsApp ".into(), true, view, config);
    let inner = border.inner(r);
    frame.render_widget(border, r);
    frame.render_widget(
        Paragraph::new("WhatsApp → Linked devices\nScan this code to link your terminal."),
        Rect::new(inner.x, inner.y, inner.width, 2),
    );
    let dark = |x: usize, y: usize| {
        x >= 4
            && y >= 4
            && x < size - 4
            && y < size - 4
            && qr[(x - 4, y - 4)] == qrcode::Color::Dark
    };
    let lines = (0..size)
        .step_by(2)
        .map(|y| {
            let row = (0..size)
                .map(|x| match (dark(x, y), dark(x, y + 1)) {
                    (true, true) => '█',
                    (true, false) => '▀',
                    (false, true) => '▄',
                    _ => ' ',
                })
                .collect::<String>();
            Line::from(row)
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(Color::Black).bg(Color::White)),
        Rect::new(inner.x, inner.y + 2, size as u16, size.div_ceil(2) as u16),
    );
}
pub(super) fn key(
    config: &Config,
    context: Context,
    action: crate::config::bindings::ActionId,
) -> String {
    config
        .bindings
        .help(context)
        .into_iter()
        .find(|(_, a)| *a == action)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "unbound".into())
}
pub(super) fn render(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    images: &mut Images,
    hits: &mut InteractionMap,
) {
    if view.overlay.is_some() {
        hits.clear();
    }
    match &view.overlay {
        Some(Overlay::Stickers(picker)) => {
            super::stickers::render(frame, area, view, config, picker, images, hits)
        }
        Some(Overlay::Emoji {
            editor, selected, ..
        }) => super::emoji::render(frame, area, view, config, editor, *selected, hits),
        Some(Overlay::Attachment { .. }) => {
            super::attachments::dialog(frame, area, view, config, hits)
        }
        Some(Overlay::MessageActions(_)) => actions::menu(frame, area, view, config, hits),
        Some(Overlay::Reactions(_)) => reactions::popup(frame, area, view, config, hits),
        Some(Overlay::MessageLinks(_)) => actions::links(frame, area, view, config, hits),
        Some(Overlay::MessageSearch(_)) => search::messages(frame, area, view, config, hits),
        Some(Overlay::Search { .. }) => search::chats(frame, area, view, config, hits),
        Some(Overlay::Help) => {
            let r = centered(area, 72, area.height.saturating_sub(2));
            frame.render_widget(Clear, r);
            let context = match view.focus {
                Focus::Chats => Context::Chats,
                Focus::Messages => Context::Messages,
                Focus::Composer => Context::Composer,
            };
            let mut keys =
                std::collections::BTreeMap::<crate::config::bindings::ActionId, Vec<String>>::new();
            for (key, action) in config.bindings.help(context) {
                keys.entry(action).or_default().push(key);
            }
            let mut lines = keys
                .into_iter()
                .map(|(action, keys)| {
                    Line::from(format!("{:20} {}", keys.join(", "), action.label()))
                })
                .collect::<Vec<_>>();
            lines.extend([
                Line::from(""),
                Line::from("Mouse: click selects · double-click opens"),
                Line::from("Right-click: message actions · wheel: scroll"),
                Line::from("↑/↓ or Page Up/Down scroll this help"),
                Line::from(""),
                Line::from("Message status"),
            ]);
            lines.extend(
                [
                    SendState::Sending,
                    SendState::Sent,
                    SendState::Delivered,
                    SendState::Read,
                    SendState::Failed,
                    SendState::Unconfirmed,
                ]
                .into_iter()
                .map(|state| {
                    Line::from(vec![
                        message_body::status_mark(state, view, config),
                        Span::raw(format!("  {state:?}")),
                    ])
                }),
            );
            hits.help_max_scroll = lines
                .len()
                .saturating_sub(r.height.saturating_sub(2) as usize);
            frame.render_widget(
                Paragraph::new(lines)
                    .scroll((
                        view.help_scroll
                            .min(hits.help_max_scroll)
                            .min(u16::MAX as usize) as u16,
                        0,
                    ))
                    .block(block(
                        format!(
                            " Help · {context:?} · {} to close ",
                            key(
                                config,
                                Context::Help,
                                crate::config::bindings::ActionId::Back
                            )
                        ),
                        true,
                        view,
                        config,
                    )),
                r,
            );
            hits.popup(frame, r, view, config);
        }
        Some(Overlay::Resend { message }) => {
            let r = centered(area, 64, 7);
            frame.render_widget(Clear, r);
            let text = if message.send_state == Some(crate::app::model::SendState::Unconfirmed) {
                "The original may already have arrived.\nSend a new copy with a new message ID?"
            } else {
                "Send this failed message as a new attempt?"
            };
            frame.render_widget(
                Paragraph::new(format!(
                    "{text}\n\n{} confirms · {} cancels",
                    key(
                        config,
                        Context::Resend,
                        crate::config::bindings::ActionId::Confirm
                    ),
                    key(
                        config,
                        Context::Resend,
                        crate::config::bindings::ActionId::Back
                    )
                ))
                .wrap(Wrap { trim: false })
                .block(block(" Confirm resend ".into(), true, view, config)),
                r,
            );
            hits.popup(frame, r, view, config);
            let buttons = Rect::new(
                r.x + 1,
                r.bottom().saturating_sub(2),
                r.width.saturating_sub(2),
                1,
            );
            frame.render_widget(
                Paragraph::new("[ Send again ]   [ Cancel ]").style(style(
                    config,
                    view,
                    ThemeRole::Accent,
                )),
                buttons,
            );
            hits.push(
                Rect::new(buttons.x, buttons.y, 14.min(buttons.width), 1),
                Target::Action(crate::config::bindings::ActionId::Confirm),
            );
            hits.push(
                Rect::new(
                    buttons.x + 16,
                    buttons.y,
                    10.min(buttons.width.saturating_sub(16)),
                    1,
                ),
                Target::Action(crate::config::bindings::ActionId::Back),
            );
        }
        None => {}
    }
}
