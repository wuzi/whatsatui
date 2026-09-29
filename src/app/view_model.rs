use super::{editor::Editor, model::*};
use tokio::time::Instant;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Focus {
    #[default]
    Chats,
    Messages,
    Composer,
}
#[derive(Clone, Debug)]
pub enum Overlay {
    Search { editor: Editor, selected: usize },
    Help,
    Resend { message: Box<MessageRecord> },
}
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub focus: Focus,
    pub account: Option<AccountId>,
    pub chats: Vec<ChatSummary>,
    pub chat: Option<ChatId>,
    pub messages: Vec<MessageRecord>,
    pub selected_message: Option<MessageKey>,
    pub receipts: Vec<Receipt>,
    pub draft: Draft,
    pub cursor: usize,
    pub overlay: Option<Overlay>,
    pub search_results: Vec<ChatSummary>,
    pub connection: ConnectionState,
    pub reason: Option<String>,
    pub progress: Option<u32>,
    pub syncing: bool,
    pub qr: Option<(String, Instant)>,
    pub now: Instant,
    pub notice: Option<String>,
    pub at_bottom: bool,
    pub new_messages: u32,
    pub has_older: bool,
    pub has_newer: bool,
    pub loading: bool,
    pub truecolor: bool,
}
