use super::*;
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
fn key(config: &Config, context: Context, action: crate::config::bindings::ActionId) -> String {
    config
        .bindings
        .help(context)
        .into_iter()
        .find(|(_, a)| *a == action)
        .map(|(k, _)| k)
        .unwrap_or_else(|| "unbound".into())
}
pub(super) fn render(frame: &mut Frame, area: Rect, view: &ViewModel, config: &Config) {
    match &view.overlay {
        Some(Overlay::MessageSearch(_)) => search::messages(frame, area, view, config),
        Some(Overlay::Search { .. }) => search::chats(frame, area, view, config),
        Some(Overlay::Help) => {
            let r = centered(area, 72, area.height.saturating_sub(2));
            frame.render_widget(Clear, r);
            let context = match view.focus {
                Focus::Chats => Context::Chats,
                Focus::Messages => Context::Messages,
                Focus::Composer => Context::Composer,
            };
            let lines = config
                .bindings
                .help(context)
                .into_iter()
                .map(|(key, action)| Line::from(format!("{key:16} {}", action.label())))
                .collect::<Vec<_>>();
            frame.render_widget(
                Paragraph::new(lines).block(block(
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
        }
        None => {}
    }
}
