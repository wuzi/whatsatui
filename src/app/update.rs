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
        preserve_draft: bool,
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
struct PendingSend {
    account: AccountId,
    chat: ChatId,
    draft: Draft,
    preserve_draft: bool,
    staging: bool,
}
pub struct App {
    pub config: Config,
    view: ViewModel,
    editor: Editor,
    drafts: HashMap<ChatId, LocalDraft>,
    pending: HashMap<RequestId, PendingSend>,
    reading: std::collections::HashSet<ChatId>,
    read_watermarks: HashMap<ChatId, Option<MessageKey>>,
    last_expiry_ms: i64,
    draft_loads: HashMap<(AccountId, ChatId), RequestId>,
    buffered: HashMap<(AccountId, ChatId), Vec<EditAction>>,
    serial: u64,
    list_request: Option<RequestId>,
    chat_request: Option<RequestId>,
    reload_list: bool,
    reload_chat: bool,
    page_cursor: Option<PageCursor>,
    pub(crate) foreground: Option<bool>,
    pub(crate) quitting: bool,
    shutdown_emitted: bool,
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
                has_newer: false,
                loading: false,
                truecolor: true,
            },
            editor: Editor::default(),
            drafts: HashMap::new(),
            pending: HashMap::new(),
            reading: Default::default(),
            read_watermarks: Default::default(),
            last_expiry_ms: 0,
            draft_loads: HashMap::new(),
            buffered: HashMap::new(),
            serial: 0,
            list_request: None,
            chat_request: None,
            reload_list: false,
            reload_chat: false,
            page_cursor: None,
            foreground: None,
            quitting: false,
            shutdown_emitted: false,
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
            self.view.loading = !self.drafts.contains_key(&chat);
            if self.view.loading {
                self.draft_loads
                    .entry((account.clone(), chat.clone()))
                    .or_insert(request);
            }
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
                self.view.at_bottom = next + 1 == self.view.messages.len() && !self.view.has_newer;
                if self.view.at_bottom {
                    self.page_cursor = None;
                } else if self.page_cursor.is_none()
                    && let Some(last) = self.view.messages.last()
                {
                    self.page_cursor = Some(PageCursor {
                        direction: PageDirection::AtOrBefore,
                        created_at_ms: last.created_at_ms,
                        key: last.key.clone(),
                    });
                }
                if delta < 0
                    && i == 0
                    && self.view.has_older
                    && let Some(m) = self.view.messages.first()
                {
                    self.load_chat(
                        Some(PageCursor {
                            direction: PageDirection::Before,
                            created_at_ms: m.created_at_ms,
                            key: m.key.clone(),
                        }),
                        effects,
                    );
                }
            }
            Focus::Composer => {}
        }
    }
    fn action(&mut self, action: ActionId, effects: &mut Vec<Effect>) {
        use ActionId as A;
        match action {
            A::Quit => {
                effects.extend(self.request_shutdown());
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
                let last = self.view.messages.last();
                let on_last =
                    last.is_some_and(|m| Some(&m.key) == self.view.selected_message.as_ref());
                if on_last && self.view.has_newer {
                    if let Some(m) = last {
                        self.load_chat(
                            Some(PageCursor {
                                direction: PageDirection::After,
                                created_at_ms: m.created_at_ms,
                                key: m.key.clone(),
                            }),
                            effects,
                        );
                    }
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
            A::Newline => self.edit_current(EditAction::Newline),
            A::RemoveReply => {
                if self.view.draft.reply.take().is_some() {
                    self.remember();
                }
            }
            A::Reply => {
                if let Some(message) = self.selected().cloned()
                    && let MessageBody::Text(text) = &message.body
                {
                    self.view.draft.reply = Some(Quote {
                        key: message.key,
                        preview: text.graphemes(true).take(160).collect(),
                        availability: QuoteAvailability::Available,
                    });
                    self.remember();
                    self.focus(Focus::Composer, effects);
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
                    self.view.overlay = Some(Overlay::Resend {
                        message: Box::new(message),
                    });
                }
            }
            A::Confirm => {
                if self.view.connection != ConnectionState::Connected {
                    self.view.notice = Some("Not sent: wait for the connection".into());
                    return;
                }
                if let Some(Overlay::Resend { message }) = self.view.overlay.take()
                    && let MessageBody::Text(text) = message.body
                {
                    self.prepare(
                        message.key.chat,
                        Draft {
                            text,
                            reply: message.quote,
                            revision: 0,
                        },
                        true,
                        effects,
                    );
                }
            }
            A::Send => {
                if self.view.draft.text.trim().is_empty() {
                    return;
                }
                if let Some(chat) = self.view.chat.clone() {
                    if self.view.loading {
                        self.view.notice =
                            Some("Loading the saved draft; your typing is kept".into());
                        return;
                    }
                    self.prepare(chat, self.view.draft.clone(), false, effects);
                }
            }
        }
    }
    fn prepare(
        &mut self,
        chat: ChatId,
        draft: Draft,
        preserve_draft: bool,
        effects: &mut Vec<Effect>,
    ) {
        if self.quitting {
            return;
        }
        if self.view.connection != ConnectionState::Connected {
            self.view.notice = Some("Not sent: wait for the connection; your draft is kept".into());
            return;
        }
        let Some(account) = self.view.account.clone() else {
            return;
        };
        if self.pending.values().any(|p| {
            p.account == account
                && p.chat == chat
                && p.draft == draft
                && p.preserve_draft == preserve_draft
        }) {
            return;
        }
        if self.pending.len() >= 8 {
            self.view.notice = Some("Please wait for pending sends; your draft is kept".into());
            return;
        }
        let request = self.request();
        self.pending.insert(
            request,
            PendingSend {
                account: account.clone(),
                chat: chat.clone(),
                draft: draft.clone(),
                preserve_draft,
                staging: false,
            },
        );
        effects.push(Effect::Prepare {
            request,
            account,
            chat,
            draft,
        });
    }
    fn edit_current(&mut self, edit: EditAction) {
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        if !self.drafts.contains_key(&chat) {
            self.buffered
                .entry((account, chat))
                .or_default()
                .push(edit.clone());
            self.editor.apply(edit);
            self.view.draft.text = self.editor.text().into();
        } else if self.editor.apply(edit) {
            self.remember();
        }
    }
    fn terminal(&mut self, event: Event, effects: &mut Vec<Effect>) {
        if self.quitting {
            return;
        }
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
            {
                self.edit_current(edit);
            }
        }
    }
    fn changed(&mut self, change: StoreChange, effects: &mut Vec<Effect>) {
        if self.view.account.as_ref() != Some(&change.account) || change.chats.is_empty() {
            return;
        }
        self.load_list(effects);
        if self
            .view
            .chat
            .as_ref()
            .is_some_and(|chat| change.chats.contains(chat))
        {
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
                    self.reading.clear();
                    self.read_watermarks.clear();
                    self.pending.retain(|_, p| p.staging);
                    self.view.overlay = None;
                    self.view.receipts.clear();
                    self.view.selected_message = None;
                    self.list_request = None;
                    self.chat_request = None;
                    effects.push(Effect::RecoverAccount(account));
                    self.load_list(effects);
                }
            }
            BackendEvent::ConnectionChanged { state, reason } => {
                let disconnected = self.view.connection == ConnectionState::Connected
                    && state != ConnectionState::Connected;
                self.view.connection = state;
                self.view.reason = reason;
                if disconnected && let Some(account) = self.view.account.clone() {
                    effects.push(Effect::RecoverAccount(account));
                }
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
                if let Some(p) = self.pending.get_mut(&request) {
                    if !p.staging
                        && !self.quitting
                        && self.view.account.as_ref() == Some(&message.key.account)
                        && p.account == message.key.account
                        && p.chat == message.key.chat
                        && p.draft == message.draft
                        && message.key.from_me
                    {
                        p.staging = true;
                        effects.push(Effect::Stage {
                            request,
                            message,
                            preserve_draft: p.preserve_draft,
                        });
                    } else if !p.staging {
                        self.pending.remove(&request);
                    }
                }
            }
            BackendEvent::PreparationFailed { request, reason } => {
                if self.pending.remove(&request).is_some() {
                    self.view.notice = Some(reason);
                }
            }
            BackendEvent::SendOutcome { key, state } => {
                effects.push(Effect::PersistOutcome { key, state })
            }
            BackendEvent::LocalError(notice) => self.view.notice = notice,
            BackendEvent::Stopped => {
                if let Some(account) = self.view.account.clone() {
                    effects.push(Effect::RecoverAccount(account));
                }
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
                        if self.view.chat.is_none()
                            && let Some(first) = self.view.chats.first()
                        {
                            self.select_chat(first.chat.clone(), effects);
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
                cursor,
                result,
            } => {
                let pair = (account.clone(), chat.clone());
                let active_account = self.view.account.as_ref() == Some(&account);
                if self.draft_loads.get(&pair) == Some(&request) {
                    self.draft_loads.remove(&pair);
                    if let Ok(snapshot) = &result {
                        let mut data = snapshot.draft.clone();
                        let mut editor = Editor::new(data.text.clone());
                        let mut changed = false;
                        for edit in self.buffered.remove(&pair).unwrap_or_default() {
                            if editor.apply(edit) {
                                data.revision += 1;
                                changed = true;
                            }
                        }
                        data.text = editor.text().into();
                        if active_account {
                            if self.view.chat.as_ref() == Some(&chat) {
                                self.view.draft = data.clone();
                                self.editor = editor;
                                self.view.loading = false;
                            }
                            self.drafts.insert(
                                chat.clone(),
                                LocalDraft {
                                    data,
                                    dirty: changed,
                                    edited_at: self.view.now,
                                    saving: None,
                                },
                            );
                            if self.quitting || self.view.chat.as_ref() != Some(&chat) {
                                effects.extend(self.flush_drafts());
                            }
                        } else if changed {
                            let request = self.request();
                            effects.push(Effect::SaveDraft {
                                request,
                                account: account.clone(),
                                chat: chat.clone(),
                                draft: data,
                            });
                        }
                    }
                }
                if !active_account
                    || self.view.chat.as_ref() != Some(&chat)
                    || self.chat_request != Some(request)
                {
                    return;
                }
                self.chat_request = None;
                match result {
                    Ok(snapshot) => {
                        self.view.loading = false;
                        let canonical = snapshot.summary.chat.clone();
                        if canonical != chat {
                            if let Some(mut local) = self.drafts.remove(&chat) {
                                if local.dirty {
                                    local.data.revision = local
                                        .data
                                        .revision
                                        .max(snapshot.draft.revision)
                                        .saturating_add(1);
                                    if let Some(q) = &mut local.data.reply
                                        && q.key.chat == chat
                                    {
                                        q.key.chat = canonical.clone();
                                    }
                                }
                                self.drafts.insert(canonical.clone(), local);
                            }
                            self.view.chat = Some(canonical.clone());
                        }
                        let chat = canonical;
                        let old = self.view.selected_message.clone();
                        self.view.messages = snapshot.messages;
                        self.view.receipts = snapshot.receipts;
                        self.view.has_older = snapshot.has_older;
                        self.view.has_newer = snapshot.has_newer;
                        if !self.view.at_bottom {
                            self.view.new_messages = u32::from(snapshot.has_newer);
                        }
                        self.view.selected_message = if self.view.at_bottom {
                            self.view.messages.last().map(|m| m.key.clone())
                        } else {
                            old.filter(|k| self.view.messages.iter().any(|m| &m.key == k))
                                .or_else(|| {
                                    if cursor
                                        .as_ref()
                                        .is_some_and(|c| c.direction == PageDirection::After)
                                    {
                                        self.view.messages.first()
                                    } else {
                                        self.view.messages.last()
                                    }
                                    .map(|m| m.key.clone())
                                })
                        };
                        self.view.at_bottom = !self.view.has_newer
                            && self.view.selected_message.as_ref()
                                == self.view.messages.last().map(|m| &m.key);
                        self.page_cursor = if self.view.at_bottom {
                            None
                        } else {
                            self.view.messages.last().map(|m| PageCursor {
                                direction: PageDirection::AtOrBefore,
                                key: m.key.clone(),
                                created_at_ms: m.created_at_ms,
                            })
                        };
                        if let Some(local) = self.drafts.get_mut(&chat) {
                            if let Some(q) = &mut local.data.reply {
                                if let Some(stored) = snapshot.draft.reply.as_ref().filter(|r| {
                                    r.key == q.key
                                        && matches!(
                                            r.availability,
                                            QuoteAvailability::Deleted | QuoteAvailability::Expired
                                        )
                                }) {
                                    *q = stored.clone();
                                }
                                if let Some(original) =
                                    self.view.messages.iter().find(|m| m.key == q.key)
                                    && matches!(
                                        original.body,
                                        MessageBody::Deleted | MessageBody::Expired
                                    )
                                {
                                    q.preview.clear();
                                    q.availability =
                                        if matches!(original.body, MessageBody::Expired) {
                                            QuoteAvailability::Expired
                                        } else {
                                            QuoteAvailability::Deleted
                                        };
                                }
                            }
                            self.view.draft.reply = local.data.reply.clone();
                            self.view.draft.revision = local.data.revision;
                        }
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
                    self.view.notice = Some(if self.quitting {
                        self.quitting = false;
                        format!("Quit canceled: {e}; draft kept. Fix storage and try again.")
                    } else {
                        e
                    });
                }
            }
            StoreCompletion::Changed { account: _, result } => match result {
                Ok(change) => self.changed(change, effects),
                Err(e) => self.view.notice = Some(e),
            },
            StoreCompletion::Staged {
                request,
                message,
                result,
            } => {
                let Some(pending) = self.pending.remove(&request) else {
                    return;
                };
                match result {
                    Ok(()) => {
                        if self.view.account.as_ref() == Some(&message.key.account) {
                            if !pending.preserve_draft
                                && let Some(local) = self.drafts.get_mut(&message.key.chat)
                                && local.data.revision == message.draft.revision
                                && local.data == message.draft
                            {
                                local.data = Draft {
                                    revision: message.draft.revision + 1,
                                    ..Default::default()
                                };
                                local.dirty = false;
                                local.saving = None;
                                if self.view.chat.as_ref() == Some(&message.key.chat) {
                                    self.view.draft = local.data.clone();
                                    self.editor = Editor::default();
                                }
                            }
                            if !self.quitting && self.view.connection == ConnectionState::Connected
                            {
                                effects.push(Effect::Transmit(message.clone()));
                            } else {
                                effects.push(Effect::PersistOutcome {
                                    key: message.key.clone(),
                                    state: SendState::Unconfirmed,
                                });
                            }
                            self.changed(
                                StoreChange {
                                    account: message.key.account,
                                    chats: vec![message.key.chat],
                                },
                                effects,
                            );
                        } else {
                            effects.push(Effect::PersistOutcome {
                                key: message.key,
                                state: SendState::Unconfirmed,
                            });
                        }
                    }
                    Err(e) => self.view.notice = Some(e),
                }
            }
            StoreCompletion::Read {
                account,
                chat,
                keys,
                result,
            } => {
                if self.view.account.as_ref() == Some(&account) {
                    self.reading.remove(&chat);
                    match result {
                        Ok(()) => {
                            self.read_watermarks
                                .insert(chat.clone(), keys.last().cloned());
                            if let Some(c) = self.view.chats.iter_mut().find(|c| c.chat == chat) {
                                c.unread = 0;
                            }
                        }
                        Err(e) => self.view.notice = Some(e),
                    }
                }
            }
        }
    }
    fn maybe_read(&mut self, effects: &mut Vec<Effect>) {
        if self.quitting
            || self.view.overlay.is_some()
            || self.view.focus == Focus::Chats
            || !self.view.at_bottom
            || self.view.loading
            || self.foreground == Some(false)
        {
            return;
        }
        let (Some(account), Some(chat)) = (self.view.account.clone(), self.view.chat.clone())
        else {
            return;
        };
        let keys = self
            .view
            .messages
            .iter()
            .filter(|m| {
                !m.key.from_me && !matches!(m.body, MessageBody::Expired | MessageBody::Deleted)
            })
            .map(|m| m.key.clone())
            .collect::<Vec<_>>();
        let watermark = keys.last().cloned();
        if self.reading.contains(&chat) || self.read_watermarks.get(&chat) == Some(&watermark) {
            return;
        }
        self.reading.insert(chat.clone());
        effects.push(Effect::MarkRead {
            account,
            chat,
            keys,
        });
    }
    pub fn request_shutdown(&mut self) -> Vec<Effect> {
        self.quitting = true;
        let mut effects = self.flush_drafts();
        self.finish_shutdown(&mut effects);
        effects
    }
    fn finish_shutdown(&mut self, effects: &mut Vec<Effect>) {
        if self.quitting
            && !self.shutdown_emitted
            && !self.drafts.values().any(|d| d.dirty)
            && self.buffered.values().all(Vec::is_empty)
        {
            self.shutdown_emitted = true;
            effects.push(Effect::Shutdown);
        }
    }
    pub fn update(&mut self, input: Input, now: Instant) -> Vec<Effect> {
        self.view.now = now;
        let mut effects = vec![];
        match input {
            Input::Terminal(event) => self.terminal(event, &mut effects),
            Input::Backend(event) => self.backend(event, &mut effects),
            Input::Store(event) => self.completion(event, &mut effects),
            Input::Tick(epoch_ms) => {
                if epoch_ms.saturating_sub(self.last_expiry_ms) >= 1000 {
                    self.last_expiry_ms = epoch_ms;
                    if let Some(account) = self.view.account.clone() {
                        effects.push(Effect::Expire {
                            account,
                            now_ms: epoch_ms,
                        });
                    }
                }
                if self.drafts.values().any(|d| {
                    d.dirty
                        && d.saving != Some(d.data.revision)
                        && now.saturating_duration_since(d.edited_at)
                            >= std::time::Duration::from_millis(250)
                }) {
                    effects.extend(self.flush_drafts());
                }
            }
        }
        self.maybe_read(&mut effects);
        self.finish_shutdown(&mut effects);
        effects
    }
}
