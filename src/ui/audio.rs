use super::*;
use crate::{
    app::model::{MessageBody, MessageRecord},
    audio::Phase,
    config::bindings::ActionId as A,
};

fn time(ms: u64) -> String {
    format!("{}:{:02}", ms / 60_000, ms / 1000 % 60)
}
pub(super) fn label(message: &MessageRecord, view: &ViewModel) -> String {
    if let Some(p) = &view.playback
        && p.request.message.key == message.key
    {
        let control = match p.phase {
            Phase::Loading => "Loading…",
            Phase::Playing => "Pause",
            Phase::Paused => "Resume",
            Phase::Finished => "Replay",
            Phase::Failed => "Retry",
        };
        return format!(
            "[{control}] {} / {} · {}",
            time(p.position_ms),
            p.duration_ms.map(time).unwrap_or_else(|| "--:--".into()),
            p.speed_label()
        );
    }
    let MessageBody::Media(a) = &message.body else {
        return String::new();
    };
    let voice = a.audio.as_ref().is_some_and(|a| a.voice);
    if a.kind == crate::media::AttachmentKind::Video {
        return "[Play] Video · open in mpv".into();
    }
    format!(
        "[Play] {} · {}",
        if voice { "Voice message" } else { "Audio" },
        a.audio
            .as_ref()
            .and_then(|a| a.seconds)
            .map(|s| time(u64::from(s) * 1000))
            .unwrap_or_else(|| "--:--".into())
    )
}
pub(super) fn header(
    frame: &mut Frame,
    area: Rect,
    view: &ViewModel,
    config: &Config,
    hits: &mut InteractionMap,
) {
    let Some(p) = view.playback.as_ref().filter(|p| p.active()) else {
        return;
    };
    frame.render_widget(Paragraph::new(" ".repeat(area.width as usize)), area);
    let mut x = area.x;
    let control = match p.phase {
        Phase::Loading => "[Wait]",
        Phase::Paused => "[Play]",
        _ => "[Pause]",
    };
    let progress = format!(
        " {} / {} ",
        time(p.position_ms),
        p.duration_ms.map(time).unwrap_or_else(|| "--:--".into())
    );
    for (text, action) in [
        (control.to_owned(), Some(A::PlayAudio)),
        (progress, None),
        (format!("[{}]", p.speed_label()), Some(A::AudioSpeed)),
        (" [Stop]".into(), Some(A::AudioStop)),
    ] {
        let width = text.width() as u16;
        let rect = Rect::new(
            x,
            area.y,
            width.min(area.right().saturating_sub(x)),
            area.height,
        );
        frame.render_widget(
            Paragraph::new(text).style(style(config, view, ThemeRole::Accent)),
            rect,
        );
        // Incomplete controls are visible text but are never clickable.
        if rect.width == width
            && let Some(action) = action
        {
            hits.push(rect, Target::Playback(p.request.id, action));
        }
        x = x.saturating_add(width);
    }
}
