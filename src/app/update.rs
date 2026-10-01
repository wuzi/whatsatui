use super::{editor::*, input::Input, model::*, view_model::*};
use crate::{config::Config, whatsapp::BackendEvent};
use tokio::time::Instant;
mod actions;
mod attachments;
mod audio;
mod interactions;
mod mouse;
mod navigation;
mod notifications;
mod search;
mod stickers;
#[derive(Clone, Debug)]
pub enum Effect {
    Notify(crate::notifications::Request),
    Audio(Option<crate::audio::Request>),
    LoadOriginal {
        request: RequestId,
        key: MessageKey,
    },
    Mutate {
        request: RequestId,
        message: Box<MessageRecord>,
        kind: MutationKind,
    },
    LoadStickers {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
    },
    ImportSticker {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        message: Box<MessageRecord>,
    },
    PasteClipboard {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        sticker: bool,
    },
    ImportImage {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        path: String,
    },
    MediaAction {
        request: RequestId,
        message: Box<MessageRecord>,
        action: crate::media::MediaAction,
    },
    DesktopAction {
        request: RequestId,
        message: Box<MessageRecord>,
        action: crate::message_actions::DesktopAction,
    },
    SearchMessages {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        query: String,
    },
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
    Original {
        request: RequestId,
        key: MessageKey,
        result: Result<Option<Box<MessageRecord>>, String>,
    },
    Stickers {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        result: Result<Vec<MessageRecord>, String>,
    },
    MessageSearch {
        request: RequestId,
        account: AccountId,
        chat: ChatId,
        query: String,
        result: Result<MessageSearchPage, String>,
    },
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
        message: Box<OutboundText>,
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
#[derive(Clone)]
struct LocalDraft {
    data: Draft,
    /// Last observed durable revision, separate from unsaved keystroke revisions.
    stored_revision: u64,
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
    notifications: crate::notifications::Inbox,
    pub config: Config,
    view: ViewModel,
    rendered: Option<crate::ui::InteractionMap>,
    original_request: Option<(RequestId, MessageKey, MessageKey)>,
    last_click: Option<(crate::ui::interaction::Target, Context, Instant)>,
    help_max_scroll: usize,
    editor: Editor,
    visible_messages: Vec<MessageKey>,
    timeline_tail_rows: usize,
    drafts: HashMap<ChatId, LocalDraft>,
    pending: HashMap<RequestId, PendingSend>,
    mutation_requests: HashMap<RequestId, (MessageRecord, MutationKind)>,
    audio_request: Option<crate::audio::Request>,
    audio_speed: crate::audio::Speed,
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
    desktop_request: Option<(RequestId, AccountId)>,
    clipboard_request: Option<(RequestId, AccountId, ChatId, bool)>,
}
impl App {
    pub fn new(config: Config) -> Self {
        Self {
            notifications: crate::notifications::Inbox::new(chrono::Utc::now().timestamp_millis()),
            config,
            rendered: None,
            original_request: None,
            last_click: None,
            help_max_scroll: 0,
            view: ViewModel {
                playback: None,
                interactions: Default::default(),
                editing: None,
                list_offsets: Default::default(),
                help_scroll: 0,
                focus: Focus::Chats,
                account: None,
                chats: vec![],
                chat: None,
                messages: vec![],
                selected_message: None,
                timeline_anchor: None,
                message_scroll: 0,
                message_scroll_max: 0,
                message_page_rows: 1,
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
            visible_messages: vec![],
            timeline_tail_rows: 0,
            drafts: HashMap::new(),
            pending: HashMap::new(),
            mutation_requests: HashMap::new(),
            audio_request: None,
            audio_speed: Default::default(),
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
            desktop_request: None,
            clipboard_request: None,
        }
    }
    pub fn view(&self) -> ViewModel {
        let mut view = self.view.clone();
        view.cursor = view
            .editing
            .as_ref()
            .map_or(self.editor.cursor(), |e| e.editor.cursor());
        for (request, (message, kind)) in &self.mutation_requests {
            if view.account.as_ref() == Some(&message.key.account)
                && view.chat.as_ref() == Some(&message.key.chat)
            {
                view.interactions
                    .mutations
                    .retain(|a| a.target.key != message.key);
                view.interactions.mutations.push(MutationAttempt {
                    id: format!("ui-{}", request.0),
                    target: message.clone(),
                    kind: kind.clone(),
                    created_at_ms: 0,
                    state: MutationState::Pending,
                });
            }
        }
        view.chats = self.chat_summaries();
        view.search_results = self.search_results();
        view
    }
    pub fn set_truecolor(&mut self, value: bool) {
        self.view.truecolor = value;
    }
    /// Under sustained backpressure keep quit, text editing, and terminal state
    /// available; defer navigation that would enqueue more storage work.
    pub fn input_under_pressure(&mut self, event: Event, now: Instant) -> Vec<Effect> {
        let allowed = match &event {
            Event::Key(key) => match self.config.bindings.lookup(self.context(), *key) {
                Some(ActionId::Quit | ActionId::ClearText | ActionId::Newline) => true,
                Some(_) => false,
                None => matches!(
                    self.context(),
                    Context::Composer
                        | Context::Search
                        | Context::MessageSearch
                        | Context::Attachment
                        | Context::Emoji
                ),
            },
            Event::Paste(_) | Event::Resize(..) | Event::FocusGained | Event::FocusLost => true,
            _ => false,
        };
        if allowed {
            self.update(Input::Terminal(event), now)
        } else {
            self.view.notice = Some(
                "Busy syncing; navigation will resume shortly. You can still type or quit.".into(),
            );
            vec![]
        }
    }
    fn request(&mut self) -> RequestId {
        self.serial += 1;
        RequestId(self.serial)
    }
    fn context(&self) -> Context {
        match &self.view.overlay {
            Some(Overlay::Stickers(_)) => Context::Stickers,
            Some(Overlay::Emoji { .. }) => Context::Emoji,
            Some(Overlay::Attachment { .. }) => Context::Attachment,
            Some(Overlay::MessageActions(_)) => Context::MessageActions,
            Some(Overlay::Reactions(_)) => Context::Reactions,
            Some(Overlay::MessageLinks(_)) => Context::MessageLinks,
            Some(Overlay::MessageSearch(_)) => Context::MessageSearch,
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
            let stored_revision = self.drafts.get(&chat).map_or(0, |d| d.stored_revision);
            self.drafts.insert(
                chat,
                LocalDraft {
                    data: self.view.draft.clone(),
                    stored_revision,
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
        self.clipboard_request = None;
        self.view.editing = None;
        self.original_request = None;
        self.view.interactions = Default::default();
        self.view.chat = Some(chat.clone());
        self.view.messages.clear();
        self.view.receipts.clear();
        self.view.selected_message = None;
        self.view.timeline_anchor = None;
        self.visible_messages.clear();
        self.timeline_tail_rows = 0;
        self.view.message_scroll = 0;
        self.view.message_scroll_max = 0;
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
    fn chat_summaries(&self) -> Vec<ChatSummary> {
        self.view
            .chats
            .iter()
            .cloned()
            .map(|mut chat| {
                if let Some(draft) = self.drafts.get(&chat.chat) {
                    chat.has_draft = draft.data.has_content();
                }
                chat
            })
            .collect()
    }
    fn search_results(&self) -> Vec<ChatSummary> {
        if let Some(Overlay::Search {
            editor,
            unread_only,
            ..
        }) = &self.view.overlay
        {
            super::search::rank_chats(&self.chat_summaries(), editor.text(), *unread_only)
        } else {
            vec![]
        }
    }
    fn selected(&self) -> Option<&MessageRecord> {
        self.view
            .selected_message
            .as_ref()
            .and_then(|k| self.view.messages.iter().find(|m| &m.key == k))
    }
    fn move_selection(&mut self, delta: isize, effects: &mut Vec<Effect>) {
        if let Some(Overlay::Reactions(menu)) = &mut self.view.overlay {
            let len = self
                .view
                .interactions
                .reactions
                .iter()
                .filter(|r| r.key == menu.message.key)
                .count();
            menu.selected = menu
                .selected
                .saturating_add_signed(delta)
                .min(len.saturating_sub(1));
            return;
        }
        if matches!(self.view.overlay, Some(Overlay::Help)) {
            self.view.help_scroll = self
                .view
                .help_scroll
                .saturating_add_signed(delta)
                .min(self.help_max_scroll);
            return;
        }
        if let Some(Overlay::Stickers(picker)) = &mut self.view.overlay {
            if picker.sending.is_none() {
                picker.selected = picker
                    .selected
                    .saturating_add_signed(delta)
                    .min(picker.items.len().saturating_sub(1));
            }
            return;
        }
        if let Some(Overlay::Emoji {
            editor, selected, ..
        }) = &mut self.view.overlay
        {
            *selected = selected
                .saturating_add_signed(delta)
                .min(super::emoji::search(editor.text()).len().saturating_sub(1));
            return;
        }
        if let Some(Overlay::Attachment {
            selected,
            importing,
            ..
        }) = &mut self.view.overlay
        {
            if importing.is_none() {
                *selected = selected
                    .saturating_add_signed(delta)
                    .min(self.view.draft.recovered.len().saturating_sub(1));
            }
            return;
        }
        if self.move_action_selection(delta) {
            return;
        }
        if let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay {
            search.selected = search
                .selected
                .saturating_add_signed(delta)
                .min(search.page.hits.len().saturating_sub(1));
            return;
        }
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
                if self.view.at_bottom && delta < 0 {
                    self.view.timeline_anchor = self.view.messages.last().map(|m| m.key.clone());
                }
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
                if next != i
                    && self
                        .view
                        .selected_message
                        .as_ref()
                        .is_none_or(|k| !self.visible_messages.contains(k))
                {
                    self.view.timeline_anchor = self.view.selected_message.clone();
                    self.timeline_tail_rows = 0;
                    self.view.message_scroll = 0;
                    self.view.message_scroll_max = 0;
                }
                self.reading_position();
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
                } else if delta > 0
                    && i + 1 == self.view.messages.len()
                    && self.view.has_newer
                    && let Some(m) = self.view.messages.last()
                {
                    self.load_chat(
                        Some(PageCursor {
                            direction: PageDirection::After,
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
    fn reading_position(&mut self) {
        self.view.at_bottom = self.view.message_scroll == 0
            && !self.view.has_newer
            && self
                .view
                .timeline_anchor
                .as_ref()
                .is_none_or(|k| self.view.messages.last().is_some_and(|m| &m.key == k))
            && self
                .view
                .messages
                .last()
                .is_some_and(|m| Some(&m.key) == self.view.selected_message.as_ref());
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
    }
    fn action(&mut self, action: ActionId, effects: &mut Vec<Effect>) {
        use ActionId as A;
        if self.view.editing.is_some()
            && matches!(
                action,
                A::AttachImage
                    | A::Stickers
                    | A::PasteClipboard
                    | A::RemoveAttachment
                    | A::RemoveReply
                    | A::Reply
                    | A::Resend
            )
        {
            self.view.notice =
                Some("Finish or cancel the edit before attaching or replying".into());
            return;
        }
        if matches!(action, A::Reply | A::Resend)
            && matches!(self.view.overlay, Some(Overlay::MessageActions(_)))
            && !self.activate_menu_target(action)
        {
            return;
        }
        match action {
            A::FocusComposer => self.focus(Focus::Composer, effects),
            A::ClearText => {
                if self.clipboard_request.take().is_some() {
                    self.view.notice = None;
                }
                self.edit_current(EditAction::Clear);
            }
            A::PlayAudio => self.play_audio(effects),
            A::AudioSpeed => self.change_audio_speed(effects),
            A::AudioStop => self.stop_audio(effects),
            A::JumpToQuote => self.jump_to_quote(effects),
            A::React => self.open_reaction_picker(),
            A::Reactions => self.open_reactions(),
            A::RemoveReaction => self.remove_reaction(effects),
            A::EditMessage => self.start_edit(effects),
            A::Stickers => self.open_stickers(effects),
            A::Emoji => {
                if !self.view.loading && self.view.chat.is_some() {
                    self.view.overlay = Some(Overlay::Emoji {
                        target: None,
                        editor: Editor::default(),
                        selected: 0,
                    });
                }
            }
            A::PasteClipboard => self.paste_clipboard(effects),
            A::AttachImage => self.open_attachment(),
            A::RemoveAttachment => {
                self.clipboard_request = None;
                if self.view.draft.attachment.take().is_some() {
                    self.remember();
                }
            }
            A::MessageActions => self.open_message_actions(),
            A::CopyText => self.copy_message_or_link(effects),
            A::OpenLinks => self.open_message_links(),
            A::DownloadMedia => {
                self.start_media_action(crate::media::MediaAction::Download, effects)
            }
            A::OpenMedia => self.start_media_action(crate::media::MediaAction::Open, effects),
            A::MessageSearch => self.open_message_search(),
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
                if self.view.overlay.is_none() && self.view.editing.is_some() {
                    if self
                        .view
                        .editing
                        .as_ref()
                        .is_some_and(|e| e.request.is_some())
                    {
                        self.view.notice = Some("Saving edit; waiting for WhatsApp".into());
                        return;
                    }
                    self.original_request = None;
                    self.view.editing = None;
                    self.focus(Focus::Composer, effects);
                    return;
                }
                if let Some(Overlay::Stickers(picker)) = &self.view.overlay
                    && let Some(request) = picker.sending
                    && let Some(pending) = self.pending.get(&request)
                {
                    // A storage transaction already in flight must finish before
                    // we can report whether there is a durable send attempt.
                    if pending.staging {
                        self.view.notice = Some("Saving sticker; waiting for storage".into());
                        return;
                    }
                    self.pending.remove(&request);
                }
                self.clipboard_request = None;
                if self.back_from_links() {
                    return;
                }
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
            A::Search | A::Unread => {
                self.view.overlay = Some(Overlay::Search {
                    editor: Editor::default(),
                    selected: 0,
                    unread_only: action == A::Unread,
                });
            }
            A::ToggleUnread => {
                if let Some(Overlay::Search {
                    unread_only,
                    selected,
                    ..
                }) = &mut self.view.overlay
                {
                    *unread_only = !*unread_only;
                    *selected = 0;
                }
            }
            A::Help => {
                self.view.help_scroll = 0;
                self.view.overlay = Some(Overlay::Help);
            }
            A::Next => self.move_selection(1, effects),
            A::Previous => self.move_selection(-1, effects),
            A::ScrollUp => self.scroll_timeline(-1, effects),
            A::ScrollDown => self.scroll_timeline(1, effects),
            A::PageUp if matches!(self.view.overlay, Some(Overlay::Help)) => {
                self.move_selection(-8, effects)
            }
            A::PageDown if matches!(self.view.overlay, Some(Overlay::Help)) => {
                self.move_selection(8, effects)
            }
            A::PageUp => self.scroll_timeline(-(self.view.message_page_rows as isize), effects),
            A::PageDown => self.scroll_timeline(self.view.message_page_rows as isize, effects),
            A::Bottom => {
                self.view.timeline_anchor = None;
                self.timeline_tail_rows = 0;
                self.view.message_scroll = 0;
                self.view.at_bottom = true;
                self.view.new_messages = 0;
                self.view.selected_message = self.view.messages.last().map(|m| m.key.clone());
                if self.page_cursor.is_some() {
                    self.load_chat(None, effects);
                }
            }
            A::Open => {
                if matches!(self.view.overlay, Some(Overlay::Stickers(_))) {
                    self.send_sticker(effects);
                    return;
                }
                if matches!(self.view.overlay, Some(Overlay::Reactions(_))) {
                    self.open_reaction_picker();
                    return;
                }
                if let Some(Overlay::Emoji {
                    editor,
                    selected,
                    target,
                }) = &self.view.overlay
                {
                    let emoji = super::emoji::search(editor.text())
                        .get(*selected)
                        .map(|e| e.as_str().to_owned());
                    if let Some(emoji) = emoji {
                        if let Some(target) = target.clone() {
                            self.choose_reaction(*target, emoji, effects);
                        } else {
                            self.view.overlay = None;
                            self.edit_current(EditAction::Insert(emoji));
                        }
                    }
                    return;
                }
                if matches!(self.view.overlay, Some(Overlay::Attachment { .. })) {
                    self.import_attachment(effects);
                    return;
                }
                if matches!(
                    self.view.overlay,
                    Some(Overlay::MessageActions(_) | Overlay::MessageLinks(_))
                ) {
                    self.open_action_selection(effects);
                    return;
                }
                if matches!(self.view.overlay, Some(Overlay::MessageSearch(_))) {
                    self.submit_or_open_message(effects);
                    return;
                }
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
                if let Some(quote) = self.selected().and_then(|m| {
                    crate::message_actions::quote(m, chrono::Utc::now().timestamp_millis())
                }) {
                    self.view.draft.reply = Some(quote);
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
                if let Some(Overlay::Resend { message }) = self.view.overlay.take() {
                    let (text, attachment) = match message.body {
                        MessageBody::Text(text) => (text, None),
                        MessageBody::LocalImage { image, caption } => (caption, Some(image)),
                        _ => {
                            self.view.notice = Some("This message cannot be resent".into());
                            return;
                        }
                    };
                    self.prepare(
                        message.key.chat,
                        Draft {
                            text,
                            attachment,
                            reply: message.quote,
                            revision: 0,
                            ..Default::default()
                        },
                        true,
                        effects,
                    );
                }
            }
            A::Send => {
                if self.view.editing.is_some() {
                    self.save_edit(effects);
                    return;
                }
                if self.clipboard_request.is_some() {
                    self.view.notice = Some("Wait for the clipboard image before sending".into());
                    return;
                }
                if self.view.draft.text.trim().is_empty() && self.view.draft.attachment.is_none() {
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
        if let Some(editing) = &mut self.view.editing {
            if editing.request.is_none() {
                editing.editor.apply(edit);
            }
            return;
        }
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
        if matches!(&event, Event::Key(_) | Event::Paste(_) | Event::Mouse(_)) {
            self.original_request = None;
            // Input establishes initial focus only. A buffered key must not
            // override an explicit FocusLost before the matching FocusGained.
            self.foreground.get_or_insert(true);
        }

        if self.quitting {
            return;
        }
        let edit = match event {
            Event::Mouse(event) => {
                self.mouse(event, effects);
                None
            }
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
                            ActionId::React
                                | ActionId::EditMessage
                                | ActionId::RemoveReaction
                                | ActionId::PasteClipboard
                                | ActionId::Stickers
                                | ActionId::Send
                                | ActionId::Open
                                | ActionId::Confirm
                                | ActionId::Quit
                                | ActionId::CopyText
                                | ActionId::OpenLinks
                                | ActionId::DownloadMedia
                                | ActionId::OpenMedia
                                | ActionId::MessageActions
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
            if let Some(Overlay::Emoji {
                editor, selected, ..
            }) = &mut self.view.overlay
            {
                if super::search::edit_query(editor, edit) {
                    *selected = 0;
                }
            } else if let Some(Overlay::Attachment {
                editor,
                importing,
                error,
                ..
            }) = &mut self.view.overlay
            {
                if importing.is_none() {
                    super::search::edit_query(editor, edit);
                    *error = None;
                }
            } else if let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay {
                if super::search::edit_query(&mut search.editor, edit) {
                    search.invalidate(None);
                }
            } else if let Some(Overlay::Search {
                editor, selected, ..
            }) = &mut self.view.overlay
            {
                if super::search::edit_query(editor, edit) {
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
        self.refresh_stickers(effects);
        if let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay
            && search.account == change.account
            && change.chats.contains(&search.chat)
        {
            search.invalidate(Some("Conversation changed; search again"));
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
            BackendEvent::IncomingMessages(messages) => self.incoming_notifications(messages),
            BackendEvent::MutationOutcome {
                request,
                account,
                result,
            } => self.mutation_outcome(request, account, result),
            BackendEvent::AccountKnown(account) => {
                if self.view.account.as_ref() != Some(&account) {
                    effects.extend(self.flush_drafts());
                    self.clipboard_request = None;
                    self.view.editing = None;
                    self.view.interactions = Default::default();
                    self.mutation_requests.clear();
                    self.original_request = None;
                    self.view.account = Some(account.clone());
                    self.view.list_offsets.clear();
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
                    self.view.timeline_anchor = None;
                    self.visible_messages.clear();
                    self.timeline_tail_rows = 0;
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
                let message = *message;
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
                        self.sticker_staged(
                            request,
                            &Err("Sticker preparation changed; try again".into()),
                        );
                    }
                }
            }
            BackendEvent::PreparationFailed { request, reason } => {
                if self.pending.remove(&request).is_some() {
                    self.sticker_staged(request, &Err(reason.clone()));
                    self.view.notice = Some(reason);
                }
            }
            BackendEvent::SendOutcome { key, state } => {
                effects.push(Effect::PersistOutcome { key, state })
            }
            BackendEvent::LocalError(notice) => self.view.notice = notice,
            BackendEvent::Stopped => {
                self.mutations_stopped();
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
            StoreCompletion::Original {
                request,
                key,
                result,
            } => self.original_loaded(request, key, result, effects),
            StoreCompletion::Stickers {
                request,
                account,
                chat,
                result,
            } => self.stickers_loaded(request, account, chat, result, effects),
            StoreCompletion::MessageSearch {
                request,
                account,
                chat,
                query,
                result,
            } => {
                self.complete_message_search(request, account, chat, query, result);
            }
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
                if result.is_err() && self.quitting && self.buffered.contains_key(&pair) {
                    self.quitting = false;
                    self.view.notice=Some("Quit canceled: saved draft could not be loaded; typed text kept. Retry after fixing storage.".into());
                }
                // Initialization belongs to the conversation, not to whichever
                // overlapping request happened to start first. Only success
                // consumes the edits typed while its saved draft was loading.
                if (self.draft_loads.contains_key(&pair) || self.buffered.contains_key(&pair))
                    && result.is_ok()
                    && (!active_account || !self.drafts.contains_key(&chat))
                {
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
                                    stored_revision: snapshot.draft.revision,
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
                            if let Some(Overlay::MessageSearch(search)) = &mut self.view.overlay {
                                search.request = None;
                                search.chat = canonical.clone();
                                search.invalidate(Some("Contact updated; search again"));
                            }
                            self.clipboard_request = None;
                            // An import belongs to the identity selected when it began.
                            if matches!(
                                self.view.overlay,
                                Some(Overlay::Attachment { .. } | Overlay::Stickers(_))
                            ) {
                                self.view.overlay = None;
                                self.view.notice = Some(
                                    "Contact updated; reopen the image picker to attach".into(),
                                );
                            }
                            let alias_draft = self.drafts.remove(&chat);
                            let canonical_draft = self.drafts.remove(&canonical);
                            let mut merged = snapshot.draft.clone();
                            merged.origin.get_or_insert_with(|| canonical.clone());
                            let mut dirty = false;
                            for (source, local) in [
                                (&canonical, canonical_draft.as_ref()),
                                (&chat, alias_draft.as_ref()),
                            ] {
                                if let Some(local) = local {
                                    merged.revision = merged.revision.max(local.data.revision);
                                    if local.dirty || local.data.revision > snapshot.draft.revision
                                    {
                                        dirty = true;
                                        merged.merge_from(&local.data, source, true);
                                    }
                                }
                            }
                            for q in merged.quotes_mut() {
                                if q.key.chat == chat {
                                    q.key.chat = canonical.clone();
                                }
                                if q.key.sender.0 == chat.0 {
                                    q.key.sender = canonical.0.clone().into();
                                }
                            }
                            if dirty {
                                merged.revision = merged.revision.saturating_add(1);
                            }
                            self.view.draft = merged.clone();
                            self.editor = Editor::new(merged.text.clone());
                            self.drafts.insert(
                                canonical.clone(),
                                LocalDraft {
                                    data: merged,
                                    stored_revision: snapshot.draft.revision,
                                    dirty,
                                    edited_at: self.view.now,
                                    saving: None,
                                },
                            );
                            if !self.view.draft.recovered.is_empty() {
                                self.view.notice = Some("Contact identities merged; saved drafts are in the image picker".into());
                            } else if dirty {
                                self.view.notice=Some("Contact identities merged; review the combined draft before sending".into());
                            }
                            self.view.chat = Some(canonical.clone());
                        }
                        let chat = canonical;
                        let old = self.view.selected_message.clone();
                        self.view.messages = snapshot.messages;
                        self.view.receipts = snapshot.receipts;
                        self.view.interactions = snapshot.interactions;
                        self.view.has_older = snapshot.has_older;
                        self.view.has_newer = snapshot.has_newer;
                        if !self.view.at_bottom {
                            self.view.new_messages = u32::from(snapshot.has_newer);
                        }
                        self.view.selected_message = if self.view.at_bottom {
                            self.view.messages.last().map(|m| m.key.clone())
                        } else {
                            old.clone()
                                .filter(|k| self.view.messages.iter().any(|m| &m.key == k))
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
                        if self.view.selected_message != old
                            || self
                                .view
                                .timeline_anchor
                                .as_ref()
                                .is_some_and(|k| !self.view.messages.iter().any(|m| &m.key == k))
                        {
                            self.view.timeline_anchor = if self.view.at_bottom {
                                None
                            } else {
                                self.view.selected_message.clone()
                            };
                            self.visible_messages.clear();
                            self.timeline_tail_rows = 0;
                            self.view.message_scroll = 0;
                            self.view.message_scroll_max = 0;
                        }
                        self.reading_position();
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
                            if local.dirty && snapshot.draft.revision > local.stored_revision {
                                let before = local.data.clone();
                                local.data.recover_from(&snapshot.draft, &chat);
                                local.stored_revision = snapshot.draft.revision;
                                if local.data != before {
                                    local.data.revision = local
                                        .data
                                        .revision
                                        .max(snapshot.draft.revision)
                                        .saturating_add(1);
                                    self.view.draft = local.data.clone();
                                    self.editor = Editor::new(local.data.text.clone());
                                    if !local.data.recovered.is_empty() {
                                        self.view.notice = Some("Contact identities merged; saved drafts are in the image picker".into());
                                    }
                                }
                            }
                            for q in local.data.quotes_mut() {
                                if let Some(stored) = snapshot
                                    .draft
                                    .reply
                                    .iter()
                                    .chain(
                                        snapshot
                                            .draft
                                            .recovered
                                            .iter()
                                            .filter_map(|d| d.reply.as_ref()),
                                    )
                                    .find(|r| {
                                        r.key == q.key
                                            && matches!(
                                                r.availability,
                                                QuoteAvailability::Deleted
                                                    | QuoteAvailability::Expired
                                            )
                                    })
                                {
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
                            self.view.draft.attachment = local.data.attachment.clone();
                            self.view.draft.origin = local.data.origin.clone();
                            self.view.draft.recovered = local.data.recovered.clone();
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
                                    stored_revision: self.view.draft.revision,
                                    dirty: false,
                                    edited_at: self.view.now,
                                    saving: None,
                                },
                            );
                        }
                    }
                    Err(e) => {
                        if self.quitting {
                            self.quitting = false;
                        }
                        self.view.notice = Some(format!(
                            "{e}; typed text kept. Reopen the chat or retry quit to load its saved draft."
                        ));
                    }
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
                    if result.is_ok() {
                        d.stored_revision = d.stored_revision.max(revision);
                        if d.data.revision == revision {
                            d.dirty = false;
                        }
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
                let message = *message;
                let Some(pending) = self.pending.remove(&request) else {
                    return;
                };
                self.sticker_staged(request, &result);
                match result {
                    Ok(()) => {
                        if self.view.account.as_ref() == Some(&message.key.account) {
                            if !pending.preserve_draft
                                && let Some(local) = self.drafts.get_mut(&message.key.chat)
                                && local.data.revision == message.draft.revision
                                && local.data == message.draft
                            {
                                local.data = Draft {
                                    origin: local.data.origin.clone(),
                                    recovered: local.data.recovered.clone(),
                                    revision: message.draft.revision + 1,
                                    ..Default::default()
                                };
                                local.dirty = false;
                                local.stored_revision = local.data.revision;
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
    fn reading_conversation(&self) -> bool {
        !(self.quitting
            || self.view.overlay.is_some()
            || self.view.focus == Focus::Chats
            || !self.view.at_bottom
            || self.view.loading
            || self.foreground == Some(false))
    }
    fn maybe_read(&mut self, effects: &mut Vec<Effect>) {
        if !self.reading_conversation() {
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
        self.stop_audio(&mut effects);
        for (account, chat) in self.buffered.keys().cloned().collect::<Vec<_>>() {
            let request = self.request();
            self.draft_loads
                .insert((account.clone(), chat.clone()), request);
            if self.view.account.as_ref() == Some(&account)
                && self.view.chat.as_ref() == Some(&chat)
            {
                self.chat_request = Some(request);
            }
            effects.push(Effect::LoadChat {
                request,
                account,
                chat,
                cursor: None,
            });
        }
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
        if matches!(
            &input,
            Input::Backend(_)
                | Input::Playback(_)
                | Input::Store(_)
                | Input::Terminal(Event::Key(_) | Event::Paste(_) | Event::Resize(..))
        ) {
            self.rendered = None;
            if !matches!(&input, Input::Playback(_)) {
                self.last_click = None;
            }
        }
        let mut effects = vec![];
        self.reconcile_message_actions();
        self.reconcile_editing();
        match input {
            Input::NotificationResult(result) => self.notification_result(result),
            Input::Playback(playback) => self.audio_observed(playback),
            Input::Rendered(map) => {
                if map.matches(&self.view) {
                    self.help_max_scroll = map.help_max_scroll;
                    self.view.list_offsets.extend(map.list_offsets.clone());
                    self.rendered = Some(map);
                }
            }
            Input::StickerImported {
                request,
                account,
                chat,
                result,
            } => self.sticker_imported(request, account, chat, result, &mut effects),
            Input::ClipboardRead {
                request,
                account,
                chat,
                result,
            } => self.clipboard_read(request, account, chat, result),
            Input::ImageImported {
                request,
                account,
                chat,
                result,
            } => self.image_imported(request, account, chat, result),
            Input::DesktopAction {
                request,
                account,
                result,
            } => {
                if self.desktop_request.as_ref() == Some(&(request, account.clone())) {
                    self.desktop_request = None;
                    if self.view.account.as_ref() == Some(&account) {
                        self.view.notice = Some(result.unwrap_or_else(|e| e));
                    }
                }
            }
            Input::TimelineViewport(metrics) => {
                if metrics.selected == self.view.selected_message
                    && metrics.anchor == self.view.timeline_anchor
                {
                    self.visible_messages = metrics.fully_visible;
                    self.timeline_tail_rows = metrics.tail_rows;
                    self.view.message_scroll_max = metrics.max_scroll;
                    self.view.message_page_rows = metrics.page_rows;
                    self.view.message_scroll = self.view.message_scroll.min(metrics.max_scroll);
                    self.reading_position();
                }
            }
            Input::Terminal(event) => self.terminal(event, &mut effects),
            Input::Backend(event) => self.backend(event, &mut effects),
            Input::Store(event) => {
                let highlighted = self.highlighted_chat();
                self.completion(event, &mut effects);
                self.restore_highlighted_chat(highlighted);
            }
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
        self.reconcile_message_actions();
        self.reconcile_editing();
        self.reconcile_audio(&mut effects);
        self.maybe_read(&mut effects);
        self.reconcile_notifications(&mut effects);
        self.finish_shutdown(&mut effects);
        effects
    }
}
