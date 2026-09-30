use super::*;
use crate::message_actions;

impl App {
    pub(super) fn open_reaction_picker(&mut self) {
        let Some(message) = self
            .action_message()
            .filter(|m| message_actions::can_react(m, chrono::Utc::now().timestamp_millis()))
        else {
            self.view.notice = Some("Select a sent message to react to".into());
            return;
        };
        self.view.overlay = Some(Overlay::Emoji {
            target: Some(Box::new(message)),
            editor: Editor::default(),
            selected: 0,
        });
    }
    pub(super) fn open_reactions(&mut self) {
        if let Some(message) = self.action_message() {
            self.view.overlay = Some(Overlay::Reactions(Box::new(MessageMenu {
                message,
                selected: 0,
            })));
        }
    }
    pub(super) fn choose_reaction(
        &mut self,
        message: MessageRecord,
        mut emoji: String,
        effects: &mut Vec<Effect>,
    ) {
        if self.view.interactions.reactions.iter().any(|r| {
            r.key == message.key && r.reactor.0 == message.key.account.0 && r.emoji == emoji
        }) {
            emoji.clear();
        }
        if self
            .start_mutation(message, MutationKind::Reaction { emoji }, effects)
            .is_some()
        {
            self.view.overlay = None;
        }
    }
    pub(super) fn remove_reaction(&mut self, effects: &mut Vec<Effect>) {
        if let Some(message) = self.action_message() {
            if !self
                .view
                .interactions
                .reactions
                .iter()
                .any(|r| r.key == message.key && r.reactor.0 == message.key.account.0)
            {
                self.view.notice = Some("You have no reaction on this message".into());
                return;
            }
            if self
                .start_mutation(
                    message,
                    MutationKind::Reaction {
                        emoji: String::new(),
                    },
                    effects,
                )
                .is_some()
            {
                self.view.overlay = None;
            }
        }
    }
    fn start_mutation(
        &mut self,
        message: MessageRecord,
        kind: MutationKind,
        effects: &mut Vec<Effect>,
    ) -> Option<RequestId> {
        if self.view.connection != ConnectionState::Connected {
            self.view.notice = Some("Not sent: wait for the connection".into());
            return None;
        }
        if self.view.account.as_ref() != Some(&message.key.account)
            || self.view.chat.as_ref() != Some(&message.key.chat)
            || self.view.messages.iter().all(|m| {
                m.key != message.key
                    || m.body != message.body
                    || m.edited_at_ms != message.edited_at_ms
            })
        {
            self.view.notice = Some("Message changed; reopen its actions".into());
            return None;
        }
        if self
            .mutation_requests
            .values()
            .any(|(m, _)| m.key == message.key)
            || self
                .view
                .interactions
                .mutations
                .iter()
                .any(|a| a.target.key == message.key && a.state == MutationState::Pending)
        {
            self.view.notice = Some("An action on this message is still pending".into());
            return None;
        }
        let now = chrono::Utc::now().timestamp_millis();
        let eligible = match &kind {
            MutationKind::Reaction { .. } => message_actions::can_react(&message, now),
            MutationKind::Edit { text } => {
                message_actions::can_edit(&message, now) && !text.trim().is_empty()
            }
        };
        if !eligible {
            self.view.notice = Some("Message action is no longer available".into());
            return None;
        }
        let request = self.request();
        self.mutation_requests
            .insert(request, (message.clone(), kind.clone()));
        effects.push(Effect::Mutate {
            request,
            message: Box::new(message),
            kind,
        });
        self.view.notice = Some("Sending message action…".into());
        Some(request)
    }
    pub(super) fn start_edit(&mut self, effects: &mut Vec<Effect>) {
        if self.view.editing.is_some() {
            self.view.notice = Some("Finish or cancel the current edit first".into());
            return;
        }
        let Some(message) = self
            .action_message()
            .filter(|m| message_actions::can_edit(m, chrono::Utc::now().timestamp_millis()))
        else {
            self.view.notice = Some("Only your sent text can be edited, within 15 minutes".into());
            return;
        };
        let MessageBody::Text(text) = &message.body else {
            return;
        };
        // A paste requested for the normal draft cannot target this editor.
        self.clipboard_request = None;
        self.view.notice = None;
        self.view.editing = Some(EditingMessage {
            editor: Editor::new(text.clone()),
            message,
            request: None,
            error: None,
        });
        self.view.overlay = None;
        self.focus(Focus::Composer, effects);
    }
    pub(super) fn reconcile_editing(&mut self) {
        if let Some(editing) = &mut self.view.editing {
            if editing.request.is_some() {
                return;
            }
            let current = self
                .view
                .messages
                .iter()
                .find(|m| m.key == editing.message.key);
            if current.is_none_or(|m| {
                m.body != editing.message.body || m.edited_at_ms != editing.message.edited_at_ms
            }) {
                editing.error = Some("Original changed; cancel and reopen the edit".into());
            } else if !message_actions::can_edit(
                &editing.message,
                chrono::Utc::now().timestamp_millis(),
            ) {
                editing.error = Some(
                    "Edit window closed or message expired; cancel to return to your draft".into(),
                );
            }
        }
    }
    pub(super) fn save_edit(&mut self, effects: &mut Vec<Effect>) {
        let Some(editing) = self.view.editing.as_ref() else {
            return;
        };
        if editing.request.is_some() {
            return;
        }
        let message = editing.message.clone();
        let text = editing.editor.text().to_owned();
        if message.body == MessageBody::Text(text.clone()) {
            self.view.editing = None;
            return;
        }
        if let Some(request) = self.start_mutation(message, MutationKind::Edit { text }, effects) {
            if let Some(editing) = &mut self.view.editing {
                editing.request = Some(request);
                editing.error = None;
            }
        } else if let Some(editing) = &mut self.view.editing {
            editing.error = self.view.notice.clone();
        }
    }
    pub(super) fn mutation_outcome(
        &mut self,
        request: RequestId,
        account: AccountId,
        result: Result<MutationState, String>,
    ) {
        let Some((message, kind)) = self.mutation_requests.remove(&request) else {
            return;
        };
        if self.view.account.as_ref() != Some(&account) || message.key.account != account {
            return;
        }
        let sent = matches!(result, Ok(MutationState::Sent));
        let notice = match result {
            Ok(MutationState::Sent) => match kind {
                MutationKind::Edit { .. } => "Message edited",
                MutationKind::Reaction { ref emoji } if emoji.is_empty() => "Reaction removed",
                _ => "Reaction sent",
            }
            .into(),
            Ok(MutationState::Failed) => "WhatsApp rejected the action; original kept".into(),
            Ok(_) => "Action unconfirmed; check WhatsApp before trying again".into(),
            Err(e) => e,
        };
        if self
            .view
            .editing
            .as_ref()
            .is_some_and(|e| e.request == Some(request))
        {
            if sent {
                self.view.editing = None;
            } else if let Some(editing) = &mut self.view.editing {
                editing.request = None;
                editing.error = Some(notice.clone());
            }
        }
        if self.view.chat.as_ref() == Some(&message.key.chat) {
            self.view.notice = Some(notice);
        }
    }
    pub(super) fn mutations_stopped(&mut self) {
        // Keep the visible uncertainty while storage recovers the journal.
        // Some requests have not appeared in a chat snapshot yet.
        self.view.interactions.mutations = self.view().interactions.mutations;
        for attempt in &mut self.view.interactions.mutations {
            if attempt.state == MutationState::Pending {
                attempt.state = MutationState::Unconfirmed;
            }
        }
        let requests: Vec<_> = self
            .mutation_requests
            .iter()
            .map(|(request, (message, _))| (*request, message.key.account.clone()))
            .collect();
        for (request, account) in requests {
            self.mutation_outcome(request, account, Ok(MutationState::Unconfirmed));
        }
    }
}

impl App {
    pub(super) fn jump_to_quote(&mut self, effects: &mut Vec<Effect>) {
        let Some(message) = self.action_message() else {
            return;
        };
        let Some(quote) = message.quote else {
            self.view.notice = Some("This message has no quoted original".into());
            return;
        };
        if quote.key.account != message.key.account || quote.key.chat != message.key.chat {
            self.view.notice = Some("The quoted original is from another conversation".into());
            return;
        }
        if matches!(
            quote.availability,
            QuoteAvailability::Deleted | QuoteAvailability::Expired
        ) {
            self.view.notice = Some("The quoted original was deleted or expired".into());
            return;
        }
        let request = self.request();
        self.original_request = Some((request, message.key, quote.key.clone()));
        effects.push(Effect::LoadOriginal {
            request,
            key: quote.key,
        });
    }
    pub(super) fn original_loaded(
        &mut self,
        request: RequestId,
        key: MessageKey,
        result: Result<Option<Box<MessageRecord>>, String>,
        effects: &mut Vec<Effect>,
    ) {
        let Some((expected, source, target)) = &self.original_request else {
            return;
        };
        if *expected != request || target != &key {
            return;
        }
        let valid = self.view.account.as_ref() == Some(&source.account)
            && self.view.chat.as_ref() == Some(&source.chat);
        self.original_request = None;
        if !valid {
            return;
        }
        let original = match result {
            Ok(Some(m))
                if self.view.account.as_ref() == Some(&m.key.account)
                    && self.view.chat.as_ref() == Some(&m.key.chat)
                    && !matches!(m.body, MessageBody::Deleted | MessageBody::Expired)
                    && !m
                        .expires_at_ms
                        .is_some_and(|at| at <= chrono::Utc::now().timestamp_millis()) =>
            {
                m
            }
            Err(e) => {
                self.view.notice = Some(e);
                return;
            }
            _ => {
                self.view.notice =
                    Some("Original is missing from local history, deleted, or expired".into());
                return;
            }
        };
        self.view.overlay = None;
        self.focus(Focus::Messages, effects);
        self.view.at_bottom = false;
        self.view.message_scroll = 0;
        self.view.message_scroll_max = 0;
        self.view.selected_message = Some(original.key.clone());
        self.view.timeline_anchor = Some(original.key.clone());
        self.visible_messages.clear();
        self.timeline_tail_rows = 0;
        self.load_chat(
            Some(PageCursor {
                direction: PageDirection::AtOrBefore,
                created_at_ms: original.created_at_ms,
                key: original.key,
            }),
            effects,
        );
    }
}
