use super::*;
use crate::audio::{Phase, Playback, Request};

impl App {
    pub(super) fn play_audio(&mut self, effects: &mut Vec<Effect>) {
        let Some(message) = self
            .action_message()
            .filter(|m| crate::message_actions::can_play(m, chrono::Utc::now().timestamp_millis()))
        else {
            self.view.notice = Some("Select a voice message, audio file or video to play".into());
            return;
        };
        if self
            .audio_request
            .as_ref()
            .is_some_and(|r| r.message.key == message.key)
            && self.view.playback.as_ref().is_some_and(Playback::active)
        {
            self.toggle_audio(effects);
            self.view.overlay = None;
            return;
        }
        let request = Request {
            id: self.request(),
            revision: 0,
            message,
            paused: false,
            speed: self.audio_speed,
        };
        self.view.playback = Some(Playback::loading(request.clone()));
        self.audio_request = Some(request.clone());
        self.view.overlay = None;
        self.view.notice = None;
        effects.push(Effect::Audio(Some(request)));
    }
    pub(super) fn toggle_audio(&mut self, effects: &mut Vec<Effect>) {
        if !self.view.playback.as_ref().is_some_and(Playback::active) {
            return;
        }
        self.sync_window_controls();
        if let Some(request) = &mut self.audio_request {
            request.paused = !request.paused;
            request.revision = request.revision.wrapping_add(1);
            effects.push(Effect::Audio(Some(request.clone())));
        }
    }
    pub(super) fn change_audio_speed(&mut self, effects: &mut Vec<Effect>) {
        if !self.view.playback.as_ref().is_some_and(Playback::active) {
            return;
        }
        self.sync_window_controls();
        self.audio_speed = self.audio_speed.next();
        if let Some(request) = &mut self.audio_request {
            request.speed = self.audio_speed;
            request.revision = request.revision.wrapping_add(1);
            effects.push(Effect::Audio(Some(request.clone())));
        }
    }
    fn sync_window_controls(&mut self) {
        if let (Some(request), Some(playback)) = (&mut self.audio_request, &self.view.playback)
            && playback.request == *request
            && matches!(playback.phase, Phase::Playing | Phase::Paused)
        {
            // Adopt native controls only after our latest desired state was
            // observed. Older observations cannot undo rapid local keypresses.
            request.paused = playback.phase == Phase::Paused;
            request.speed = crate::audio::Speed::from_milli(playback.speed_milli);
            self.audio_speed = request.speed;
        }
    }
    pub(super) fn stop_audio(&mut self, effects: &mut Vec<Effect>) {
        if self.audio_request.take().is_some() {
            effects.push(Effect::Audio(None));
        }
        self.view.playback = None;
    }
    pub(super) fn audio_observed(&mut self, playback: Playback) {
        if self.quitting
            || self.view.account.as_ref() != Some(&playback.request.message.key.account)
            || self
                .audio_request
                .as_ref()
                .is_none_or(|r| !r.same_source(&playback.request))
        {
            return;
        }
        if playback.phase == Phase::Failed {
            self.view.notice = playback.error.clone();
        }
        self.view.playback = Some(playback);
    }
    pub(super) fn reconcile_audio(&mut self, effects: &mut Vec<Effect>) {
        let Some(request) = &self.audio_request else {
            return;
        };
        if self.view.account.as_ref() != Some(&request.message.key.account)
            || request
                .message
                .expires_at_ms
                .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis())
            || self
                .view
                .messages
                .iter()
                .find(|m| m.key == request.message.key)
                .is_some_and(|m| m.body != request.message.body)
        {
            self.stop_audio(effects);
        }
    }
}
