use super::*;

impl App {
    fn notification_reading_chat(&self) -> Option<ChatId> {
        (self.foreground == Some(true) && self.reading_conversation())
            .then(|| self.view.chat.clone())
            .flatten()
    }
    pub(super) fn incoming_notifications(&mut self, mut messages: Vec<MessageRecord>) {
        if self.quitting || !self.config.notifications.enabled {
            return;
        }
        let reading = self.notification_reading_chat();
        messages.retain(|m| {
            self.view
                .account
                .as_ref()
                .is_none_or(|a| a == &m.key.account)
                && reading.as_ref() != Some(&m.key.chat)
        });
        self.notifications.push(messages, self.view.now);
    }
    pub(super) fn reconcile_notifications(&mut self, effects: &mut Vec<Effect>) {
        let enabled = !(self.quitting
            || !self.config.notifications.enabled
            || self.view.connection == ConnectionState::PairingRequired);
        self.notifications
            .update_context(crate::notifications::Context {
                account: self.view.account.clone(),
                reading: self.notification_reading_chat(),
                enabled,
                previews: self.config.notifications.previews,
            });
        if !enabled {
            self.notifications.clear();
            return;
        }
        self.notifications.retain(
            self.view.account.as_ref(),
            self.notification_reading_chat().as_ref(),
        );
        if self.view.account.is_some()
            && let Some(request) = self
                .notifications
                .take(self.view.now, self.config.notifications.previews)
        {
            effects.push(Effect::Notify(request, self.notifications.context()));
        }
    }
    pub(super) fn notification_result(&mut self, result: Result<(), String>) {
        if self.notifications.completed(&result, self.view.now) && !self.quitting {
            self.view.notice = result.err();
        }
    }
}
