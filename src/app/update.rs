use super::{editor::*, input::Input, model::*, view_model::*};
use crate::{config::Config, whatsapp::BackendEvent};
use tokio::time::Instant;
#[derive(Clone, Debug)]
pub enum Effect {
    LoadChats {
        request: RequestId,
        account: AccountId,
    },
    LoadChat {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        cursor: Option<PageCursor>,
    },
    SaveDraft {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        draft: Draft,
    },
    Prepare {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        draft: Draft,
    },
    Stage {
        request: RequestId,
        message: OutboundText,
    },
    Transmit(OutboundText),
    PersistOutcome {
        key: MessageKey,
        state: SendState,
    },
    MarkRead {
        account: AccountId,
        chat: ChatId,
        keys: Vec<MessageKey>,
    },
    RecoverAccount(AccountId),
    Expire {
        account: AccountId,
        now_ms: i64,
    },
    Shutdown,
}
#[derive(Debug)]
pub enum StoreCompletion {
    Chats {
        request: RequestId,
        account: AccountId,
        result: Result<Vec<ChatSummary>, String>,
    },
    Chat {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        cursor: Option<PageCursor>,
        result: Result<Box<ChatSnapshot>, String>,
    },
    DraftSaved {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        revision: u64,
        result: Result<(), String>,
    },
    Staged {
        request: RequestId,
        message: OutboundText,
        result: Result<(), String>,
    },
    Changed {
        account: AccountId,
        result: Result<StoreChange, String>,
    },
    Read {
        account: AccountId,
        chat: ChatId,
        keys: Vec<MessageKey>,
        result: Result<(), String>,
    },
}
use crate::config::bindings::{ActionId, Context};
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;
#[derive(Clone)]
struct LocalDraft {
    data: Draft,
    dirty: bool,
    edited_at: Instant,
    saving: Option<u64>,
}
pub struct App {
    pub config: Config,
    view: ViewModel,
    editor: Editor,
    drafts: HashMap<ChatId, LocalDraft>,
    serial: u64,
    list_request: Option<RequestId>,
    chat_request: Option<RequestId>,
    reload_list: bool,
    reload_chat: bool,
    page_cursor: Option<PageCursor>,
    pub(crate) foreground: Option<bool>,
    pub(crate) quitting: bool,
}
impl App {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            view: ViewModel {
                focus: Focus::Chats,
                account: None,
                chats: vec![],
                chat: None,
                messages: vec![],
                selected_message: None,
                receipts: vec![],
                draft: Draft::default(),
                cursor: 0,
                overlay: None,
                search_results: vec![],
                connection: ConnectionState::Connecting,
                reason: None,
                progress: None,
                syncing: false,
                qr: None,
                now: Instant::now(),
                notice: None,
                at_bottom: true,
                new_messages: 0,
                has_older: false,
                loading: false,
                truecolor: true,
            },
            editor: Editor::default(),
            drafts: HashMap::new(),
            serial: 0,
            list_request: None,
            chat_request: None,
            reload_list: false,
            reload_chat: false,
            page_cursor: None,
            foreground: None,
            quitting: false,
        }
    }
    pub fn view(&self) -> ViewModel {
        let mut view = self.view.clone();
        view.cursor = self.editor.cursor();
        view.search_results = self.search_results();
        view
    }
    pub fn set_truecolor(&mut self, value: bool) {
        self.view.truecolor = value;
    }
    fn request(&mut self) -> RequestId {
        self.serial += 1;
        RequestId(self.serial)
    }
    fn context(&self) -> Context {
        match &self.view.overlay {
            Some(Overlay::Search { .. }) => Context::Search,
            Some(Overlay::Help) => Context::Help,
            Some(Overlay::Resend { .. }) => Context::Resend,
            None => match self.view.focus {
                Focus::Chats => Context::Chats,
                Focus::Messages => Context::Messages,
                Focus::Composer => Context::Composer,
            },
        }
    }
    fn load_list(&mut self, effects: &mut Vec<Effect>) {
        if self.list_request.is_some() {
            self.reload_list = true;
            return;
        }
        if let Some(account) = self.view.account.clone() {
            let request = self.request();
            self.list_request = Some(request);
            effects.push(Effect::LoadChats { request, account });
        }
    }
    fn load_chat(&mut self, cursor: Option<PageCursor>, effects: &mut Vec<Effect>) {
        if let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone()) {
            let request = self.request();
            self.chat_request = Some(request);
            self.view.loading = true;
            self.page_cursor = cursor.clone();
            effects.push(Effect::LoadChat {
                request,
                account,
                chat,
                cursor,
            });
        }
    }
    fn remember(&mut self) {
        if let Some(chat) = self.view.chat.clone() {
            self.view.draft.text = self.editor.text().into();
            self.view.draft.revision = self.view.draft.revision.saturating_add(1);
            let saving = self.drafts.get(&chat).and_then(|d| d.saving);
            self.drafts.insert(
                chat,
                LocalDraft {
                    data: self.view.draft.clone(),
                    dirty: true,
                    edited_at: self.view.now,
                    saving,
                },
            );
        }
    }
    pub fn flush_drafts(&mut self) -> Vec<Effect> {
        let mut effects = vec![];
        if let Some(account) = self.view.account.clone() {
            let chats: Vec<_> = self.drafts.keys().cloned().collect();
            for chat in chats {
                let d = self.drafts.get(&chat).unwrap();
                if d.dirty && d.saving != Some(d.data.revision) {
                    let draft = d.data.clone();
                    let request = self.request();
                    self.drafts.get_mut(&chat).unwrap().saving = Some(draft.revision);
                    effects.push(Effect::SaveDraft {
                        request,
                        account: account.clone(),
                        chat,
                        draft,
                    });
                }
            }
        }
        effects
    }
    fn focus(&mut self, focus: Focus, effects: &mut Vec<Effect>) {
        if self.view.focus == Focus::Composer && focus != Focus::Composer {
            effects.extend(self.flush_drafts());
        }
        self.view.focus = focus;
    }
    fn select_chat(&mut self, chat: ChatId, effects: &mut Vec<Effect>) {
        if self.view.chat.as_ref() == Some(&chat) {
            return;
        }
        effects.extend(self.flush_drafts());
        self.view.chat = Some(chat.clone());
        self.view.messages.clear();
        self.view.receipts.clear();
        self.view.selected_message = None;
        self.view.at_bottom = true;
        self.view.new_messages = 0;
        self.view.draft = self
            .drafts
            .get(&chat)
            .map(|d| d.data.clone())
            .unwrap_or_default();
        self.editor = Editor::new(self.view.draft.text.clone());
        self.load_chat(None, effects);
    }
    fn search_results(&self) -> Vec<ChatSummary> {
        let query = if let Some(Overlay::Search { editor, .. }) = &self.view.overlay {
            editor.text().to_lowercase()
        } else {
            return vec![];
        };
        self.view
            .chats
            .iter()
            .filter(|c| {
                c.name.to_lowercase().contains(&query)
                    || c.phone.as_ref().is_some_and(|p| p.contains(&query))
                    || c.chat.0.to_lowercase().contains(&query)
            })
            .cloned()
            .collect()
    }
    fn selected(&self) -> Option<&MessageRecord> {
        self.view
            .selected_message
            .as_ref()
            .and_then(|k| self.view.messages.iter().find(|m| &m.key == k))
    }
    fn move_selection(&mut self, delta: isize, effects: &mut Vec<Effect>) {
        if matches!(self.view.overlay, Some(Overlay::Search { .. })) {
            let len = self.search_results().len();
            if let Some(Overlay::Search { selected, .. }) = &mut self.view.overlay {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(len.saturating_sub(1));
            }
            return;
        }
        match self.view.focus {
            Focus::Chats => {
                let i = self
                    .view
                    .chats
                    .iter()
                    .position(|c| Some(&c.chat) == self.view.chat.as_ref())
                    .unwrap_or(0);
                let i = i
                    .saturating_add_signed(delta)
                    .min(self.view.chats.len().saturating_sub(1));
                if let Some(c) = self.view.chats.get(i) {
                    self.select_chat(c.chat.clone(), effects);
                }
            }
            Focus::Messages => {
                let i = self
                    .view
                    .messages
                    .iter()
                    .position(|m| Some(&m.key) == self.view.selected_message.as_ref())
                    .unwrap_or(self.view.messages.len().saturating_sub(1));
                let next = i
                    .saturating_add_signed(delta)
                    .min(self.view.messages.len().saturating_sub(1));
                self.view.selected_message = self.view.messages.get(next).map(|m| m.key.clone());
                self.view.at_bottom =
                    next + 1 == self.view.messages.len() && self.page_cursor.is_none();
                if delta < 0 && i == 0 && self.view.has_older {
                    if let Some(m) = self.view.messages.first() {
                        self.load_chat(
                            Some(PageCursor {
                                created_at_ms: m.created_at_ms,
                                key: m.key.clone(),
                            }),
                            effects,
                        );
                    }
                }
            }
            Focus::Composer => {}
        }
    }
    fn action(&mut self, action: ActionId, effects: &mut Vec<Effect>) {
        use ActionId as A;
        match action {
            A::Quit => {
                self.quitting = true;
                effects.extend(self.flush_drafts());
                effects.push(Effect::Shutdown);
            }
            A::FocusNext | A::FocusPrevious => {
                let next = match (self.view.focus, action == A::FocusNext) {
                    (Focus::Chats, true) | (Focus::Composer, false) => Focus::Messages,
                    (Focus::Messages, true) | (Focus::Chats, false) => Focus::Composer,
                    _ => Focus::Chats,
                };
                self.focus(next, effects);
            }
            A::Back => {
                if self.view.overlay.take().is_none() {
                    self.focus(
                        if self.view.focus == Focus::Composer {
                            Focus::Messages
                        } else {
                            Focus::Chats
                        },
                        effects,
                    );
                }
            }
            A::Search => {
                self.view.overlay = Some(Overlay::Search {
                    editor: Editor::default(),
                    selected: 0,
                });
            }
            A::Help => {
                self.view.overlay = Some(Overlay::Help);
            }
            A::Next => self.move_selection(1, effects),
            A::Previous => self.move_selection(-1, effects),
            A::PageUp => self.move_selection(-20, effects),
            A::PageDown => {
                if self.page_cursor.is_some() {
                    self.view.at_bottom = true;
                    self.load_chat(None, effects);
                } else {
                    self.move_selection(20, effects);
                }
            }
            A::Bottom => {
                self.view.at_bottom = true;
                self.view.new_messages = 0;
                self.view.selected_message = self.view.messages.last().map(|m| m.key.clone());
                if self.page_cursor.is_some() {
                    self.load_chat(None, effects);
                }
            }
            A::Open => {
                if let Some(Overlay::Search { selected, .. }) = &self.view.overlay {
                    if let Some(chat) = self.search_results().get(*selected) {
                        let id = chat.chat.clone();
                        self.view.overlay = None;
                        self.select_chat(id, effects);
                    } else {
                        return;
                    }
                }
                self.focus(Focus::Composer, effects);
            }
            A::Newline => {
                if self.view.chat.is_some() {
                    self.editor.apply(EditAction::Newline);
                    self.remember();
                }
            }
            A::RemoveReply => {
                if self.view.draft.reply.take().is_some() {
                    self.remember();
                }
            }
            A::Reply => {
                if let Some(message) = self.selected().cloned() {
                    if let MessageBody::Text(text) = &message.body {
                        self.view.draft.reply = Some(Quote {
                            key: message.key,
                            preview: text.graphemes(true).take(160).collect(),
                            availability: QuoteAvailability::Available,
                        });
                        self.remember();
                        self.focus(Focus::Composer, effects);
                    }
                }
            }
            A::Resend => {
                if let Some(message) = self
                    .selected()
                    .filter(|m| {
                        m.key.from_me
                            && matches!(
                                m.send_state,
                                Some(SendState::Failed | SendState::Unconfirmed)
                            )
                    })
                    .cloned()
                {
                    self.view.overlay = Some(Overlay::Resend { message });
                }
            }
            A::Confirm => {
                if let Some(Overlay::Resend { message }) = self.view.overlay.take() {
                    if let MessageBody::Text(text) = message.body {
                        if let Some(account) = self.view.account.clone() {
                            let request = self.request();
                            effects.push(Effect::Prepare {
                                request,
                                account,
                                chat: message.key.chat,
                                draft: Draft {
                                    text,
                                    reply: message.quote,
                                    revision: 0,
                                },
                            });
                        }
                    }
                }
            }
            A::Send => {
                if self.view.draft.text.trim().is_empty() || self.view.loading {
                    return;
                }
                if self.view.connection != ConnectionState::Connected {
                    self.view.notice =
                        Some("Not sent: wait for the connection; your draft is kept".into());
                    return;
                }
                if let (Some(account), Some(chat)) =
                    (self.view.account.clone(), self.view.chat.clone())
                {
                    let request = self.request();
                    effects.push(Effect::Prepare {
                        request,
                        account,
                        chat,
                        draft: self.view.draft.clone(),
                    });
                }
            }
        }
    }
    fn terminal(&mut self, event: Event, effects: &mut Vec<Effect>) {
        let edit = match event {
            Event::FocusLost => {
                self.foreground = Some(false);
                return;
            }
            Event::FocusGained => {
                self.foreground = Some(true);
                return;
            }
            Event::Paste(text) => Some(EditAction::Insert(text)),
            Event::Key(key) => {
                if key.kind == KeyEventKind::Release {
                    return;
                }
                if let Some(action) = self.config.bindings.lookup(self.context(), key) {
                    if key.kind == KeyEventKind::Repeat
                        && matches!(
                            action,
                            ActionId::Send | ActionId::Open | ActionId::Confirm | ActionId::Quit
                        )
                    {
                        return;
                    }
                    self.action(action, effects);
                    return;
                }
                match key.code {
                    KeyCode::Char(c)
                        if !key
                            .modifiers
                            .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL) =>
                    {
                        Some(EditAction::Insert(c.to_string()))
                    }
                    KeyCode::Backspace => Some(EditAction::Backspace),
                    KeyCode::Delete => Some(EditAction::Delete),
                    KeyCode::Left => Some(EditAction::Left),
                    KeyCode::Right => Some(EditAction::Right),
                    KeyCode::Up => Some(EditAction::Up),
                    KeyCode::Down => Some(EditAction::Down),
                    KeyCode::Home => Some(EditAction::Home),
                    KeyCode::End => Some(EditAction::End),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(edit) = edit {
            if let Some(Overlay::Search { editor, selected }) = &mut self.view.overlay {
                if editor.apply(edit) {
                    *selected = 0;
                }
            } else if self.view.overlay.is_none()
                && self.view.focus == Focus::Composer
                && self.view.chat.is_some()
                && !self.view.loading
            {
                if self.editor.apply(edit) {
                    self.remember();
                }
            }
        }
    }
    fn changed(&mut self, change: StoreChange, effects: &mut Vec<Effect>) {
        if self.view.account.as_ref() != Some(&change.account) {
            return;
        }
        self.load_list(effects);
        if self
            .view
            .chat
            .as_ref()
            .is_some_and(|chat| change.chats.contains(chat))
        {
            if !self.view.at_bottom {
                self.view.new_messages = self.view.new_messages.saturating_add(1);
            }
            if self.chat_request.is_some() {
                self.reload_chat = true;
            } else {
                self.load_chat(self.page_cursor.clone(), effects);
            }
        }
    }
    fn backend(&mut self, event: BackendEvent, effects: &mut Vec<Effect>) {
        match event {
            BackendEvent::AccountKnown(account) => {
                if self.view.account.as_ref() != Some(&account) {
                    effects.extend(self.flush_drafts());
                    self.view.account = Some(account.clone());
                    self.view.chat = None;
                    self.view.chats.clear();
                    self.view.messages.clear();
                    self.view.draft = Draft::default();
                    self.editor = Editor::default();
                    self.drafts.clear();
                    self.list_request = None;
                    self.chat_request = None;
                    effects.push(Effect::RecoverAccount(account));
                    self.load_list(effects);
                }
            }
            BackendEvent::ConnectionChanged { state, reason } => {
                self.view.connection = state;
                self.view.reason = reason;
                if state == ConnectionState::Connected {
                    self.view.qr = None;
                }
            }
            BackendEvent::PairingQr {
                content,
                expires_at,
            } => {
                self.view.connection = ConnectionState::PairingRequired;
                self.view.qr = Some((content, expires_at));
            }
            BackendEvent::HistoryProgress(progress) => {
                self.view.progress = progress;
                self.view.syncing = progress != Some(100);
            }
            BackendEvent::StoreChanged(change) => self.changed(change, effects),
            BackendEvent::Prepared { request, message } => {
                if self.view.account.as_ref() == Some(&message.key.account) {
                    effects.push(Effect::Stage { request, message });
                }
            }
            BackendEvent::PreparationFailed { reason, .. } => self.view.notice = Some(reason),
            BackendEvent::SendOutcome { key, state } => {
                effects.push(Effect::PersistOutcome { key, state })
            }
            BackendEvent::LocalError(notice) => self.view.notice = notice,
            BackendEvent::Stopped => {
                self.view.connection = ConnectionState::Disconnected;
                self.view.notice = Some("WhatsApp service stopped; local drafts are kept".into());
            }
        }
    }
    fn completion(&mut self, event: StoreCompletion, effects: &mut Vec<Effect>) {
        match event {
            StoreCompletion::Chats {
                request,
                account,
                result,
            } => {
                if self.view.account.as_ref() != Some(&account)
                    || self.list_request != Some(request)
                {
                    return;
                }
                self.list_request = None;
                match result {
                    Ok(chats) => {
                        self.view.chats = chats;
                        if self.view.chat.is_none() {
                            if let Some(first) = self.view.chats.first() {
                                self.select_chat(first.chat.clone(), effects);
                            }
                        }
                    }
                    Err(e) => self.view.notice = Some(e),
                }
                if std::mem::take(&mut self.reload_list) {
                    self.load_list(effects);
                }
            }
            StoreCompletion::Chat {
                request,
                account,
                chat,
                cursor: _,
                result,
            } => {
                if self.view.account.as_ref() != Some(&account)
                    || self.view.chat.as_ref() != Some(&chat)
                    || self.chat_request != Some(request)
                {
                    return;
                }
                self.chat_request = None;
                self.view.loading = false;
                match result {
                    Ok(snapshot) => {
                        let old = self.view.selected_message.clone();
                        self.view.messages = snapshot.messages;
                        self.view.receipts = snapshot.receipts;
                        self.view.has_older = snapshot.has_older;
                        self.view.selected_message = if self.view.at_bottom {
                            self.view.messages.last().map(|m| m.key.clone())
                        } else {
                            old.filter(|k| self.view.messages.iter().any(|m| &m.key == k))
                                .or_else(|| self.view.messages.last().map(|m| m.key.clone()))
                        };
                        if self
                            .drafts
                            .get(&chat)
                            .is_none_or(|d| !d.dirty && d.data.revision < snapshot.draft.revision)
                        {
                            self.view.draft = snapshot.draft;
                            self.editor = Editor::new(self.view.draft.text.clone());
                            self.drafts.insert(
                                chat,
                                LocalDraft {
                                    data: self.view.draft.clone(),
                                    dirty: false,
                                    edited_at: self.view.now,
                                    saving: None,
                                },
                            );
                        }
                    }
                    Err(e) => self.view.notice = Some(e),
                }
                if std::mem::take(&mut self.reload_chat) {
                    self.load_chat(self.page_cursor.clone(), effects);
                }
            }
            StoreCompletion::DraftSaved {
                account,
                chat,
                revision,
                result,
                ..
            } => {
                if self.view.account.as_ref() != Some(&account) {
                    return;
                }
                if let Some(d) = self.drafts.get_mut(&chat) {
                    if d.saving == Some(revision) {
                        d.saving = None;
                    }
                    if result.is_ok() && d.data.revision == revision {
                        d.dirty = false;
                    }
                }
                if let Err(e) = result {
                    self.view.notice = Some(e);
                }
            }
            StoreCompletion::Changed { account: _, result } => match result {
                Ok(change) => self.changed(change, effects),
                Err(e) => self.view.notice = Some(e),
            },
            StoreCompletion::Staged { .. } | StoreCompletion::Read { .. } => {}
        }
    }
    pub fn update(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        self.view.now = now;
        let mut effects = vec![];
        match input {
            Input::Terminal(event) => self.terminal(event, &mut effects),
            Input::Backend(event) => self.backend(event, &mut effects),
            Input::Store(event) => self.completion(event, &mut effects),
            Input::Tick(_) => {}
        }
        effects
    }
}
