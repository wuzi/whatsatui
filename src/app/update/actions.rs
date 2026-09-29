use super::*;
use crate::message_actions::{self, DesktopAction};

impl App {
    fn action_message(&self) -> Option<MessageRecord> {
        let message = match &self.view.overlay {
            Some(Overlay::MessageActions(menu)) => &menu.message,
            Some(Overlay::MessageLinks(links)) => &links.message,
            _ => self.selected()?,
        };
        self.view
            .messages
            .iter()
            .find(|m| m.key == message.key && m.body == message.body)
            .filter(|m| {
                !message_actions::available(m, chrono::Utc::now().timestamp_millis()).is_empty()
            })
            .cloned()
    }
    pub(super) fn reconcile_message_actions(&mut self) {
        if matches!(
            self.view.overlay,
            Some(Overlay::MessageActions(_) | Overlay::MessageLinks(_))
        ) && self.action_message().is_none()
        {
            self.view.overlay = None;
            self.view.notice = Some("Message changed or expired; reopen its actions".into());
        }
    }
    pub(super) fn open_message_actions(&mut self) {
        if let Some(message) = self.action_message() {
            self.view.overlay = Some(Overlay::MessageActions(Box::new(MessageMenu {
                message,
                selected: 0,
            })));
        } else {
            self.view.notice = Some("Select an available message first".into());
        }
    }
    pub(super) fn open_message_links(&mut self) {
        let Some(message) = self.action_message() else {
            return;
        };
        let links = message_actions::web_links(
            message_actions::text(&message, chrono::Utc::now().timestamp_millis())
                .unwrap_or_default(),
        );
        if links.is_empty() {
            self.view.notice = Some("No HTTP/HTTPS links in this message".into());
            return;
        }
        let return_to_menu = match &self.view.overlay {
            Some(Overlay::MessageActions(menu)) => Some(menu.selected),
            _ => None,
        };
        self.view.overlay = Some(Overlay::MessageLinks(Box::new(MessageLinks {
            message,
            links,
            selected: 0,
            return_to_menu,
        })));
    }
    pub(super) fn move_action_selection(&mut self, delta: isize) -> bool {
        let (selected, len) = match &mut self.view.overlay {
            Some(Overlay::MessageActions(menu)) => (
                &mut menu.selected,
                message_actions::available(&menu.message, chrono::Utc::now().timestamp_millis())
                    .len(),
            ),
            Some(Overlay::MessageLinks(links)) => (&mut links.selected, links.links.len()),
            _ => return false,
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
        true
    }
    pub(super) fn back_from_links(&mut self) -> bool {
        if !matches!(self.view.overlay, Some(Overlay::MessageLinks(_))) {
            return false;
        }
        if let Some(Overlay::MessageLinks(links)) = self.view.overlay.take()
            && let Some(selected) = links.return_to_menu
        {
            self.view.overlay = Some(Overlay::MessageActions(Box::new(MessageMenu {
                message: links.message,
                selected,
            })));
        }
        true
    }
    pub(super) fn activate_menu_target(&mut self, action: ActionId) -> bool {
        let Some(message) = self.action_message() else {
            return false;
        };
        if !message_actions::available(&message, chrono::Utc::now().timestamp_millis())
            .contains(&action)
        {
            self.view.overlay = None;
            self.view.notice = Some("That message action is no longer available".into());
            return false;
        }
        self.view.selected_message = Some(message.key);
        self.view.overlay = None;
        true
    }
    pub(super) fn open_action_selection(&mut self, effects: &mut Vec<Effect>) {
        match self.view.overlay.clone() {
            Some(Overlay::MessageActions(menu)) => {
                if let Some(action) =
                    message_actions::available(&menu.message, chrono::Utc::now().timestamp_millis())
                        .get(menu.selected)
                {
                    self.action(*action, effects);
                }
            }
            Some(Overlay::MessageLinks(links)) => {
                if let Some(url) = links.links.get(links.selected) {
                    self.start_desktop_action(
                        links.message,
                        DesktopAction::OpenLink(url.clone()),
                        effects,
                    );
                }
            }
            _ => {}
        }
    }
    pub(super) fn copy_message_or_link(&mut self, effects: &mut Vec<Effect>) {
        let Some(message) = self.action_message() else {
            self.view.notice = Some("Message text is not available to copy".into());
            return;
        };
        let action = if let Some(Overlay::MessageLinks(links)) = &self.view.overlay {
            let Some(url) = links.links.get(links.selected) else {
                return;
            };
            DesktopAction::CopyLink(url.clone())
        } else {
            DesktopAction::CopyText
        };
        self.start_desktop_action(message, action, effects);
    }
    pub(super) fn start_media_action(
        &mut self,
        action: crate::media::MediaAction,
        effects: &mut Vec<Effect>,
    ) {
        let Some(message) = self.action_message() else {
            self.view.notice = Some("Select an available attachment first".into());
            return;
        };
        if !matches!(message.body, MessageBody::Media(_)) {
            self.view.notice = Some("No downloadable attachment in this message".into());
            return;
        }
        if self.desktop_request.is_some() {
            self.view.notice = Some("An attachment or desktop action is still running".into());
            return;
        }
        let request = self.request();
        self.desktop_request = Some((request, message.key.account.clone()));
        self.view.overlay = None;
        self.view.notice = Some(
            match action {
                crate::media::MediaAction::Download => "Downloading attachment…",
                crate::media::MediaAction::Open => "Opening downloaded file…",
            }
            .into(),
        );
        effects.push(Effect::MediaAction {
            request,
            message: Box::new(message),
            action,
        });
    }
    fn start_desktop_action(
        &mut self,
        message: MessageRecord,
        action: DesktopAction,
        effects: &mut Vec<Effect>,
    ) {
        if self.desktop_request.is_some() {
            self.view.notice = Some("A desktop action is still running".into());
            return;
        }
        let request = self.request();
        self.desktop_request = Some((request, message.key.account.clone()));
        self.view.overlay = None;
        self.view.notice = Some(
            if matches!(action, DesktopAction::OpenLink(_)) {
                "Opening link…"
            } else {
                "Copying…"
            }
            .into(),
        );
        effects.push(Effect::DesktopAction {
            request,
            message: Box::new(message),
            action,
        });
    }
}
