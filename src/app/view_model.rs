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
    Stickers(Box<StickerPicker>),
    Emoji {
        target: Option<Box<MessageRecord>>,
        editor: Editor,
        selected: usize,
    },
    Attachment {
        editor: Editor,
        selected: usize,
        importing: Option<RequestId>,
        error: Option<String>,
    },
    MessageActions(Box<MessageMenu>),
    Reactions(Box<MessageMenu>),
    MessageLinks(Box<MessageLinks>),
    MessageSearch(Box<super::search::MessageSearch>),
    Search {
        editor: Editor,
        selected: usize,
        unread_only: bool,
    },
    Help,
    Resend {
        message: Box<MessageRecord>,
    },
}
#[derive(Clone, Debug)]
pub struct MessageMenu {
    pub message: MessageRecord,
    pub selected: usize,
}
#[derive(Clone, Debug)]
pub struct MessageLinks {
    pub message: MessageRecord,
    pub links: Vec<String>,
    pub selected: usize,
    pub return_to_menu: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct TimelineViewport {
    pub selected: Option<MessageKey>,
    pub anchor: Option<MessageKey>,
    pub tail_rows: usize,
    pub fully_visible: Vec<MessageKey>,
    pub max_scroll: usize,
    pub page_rows: usize,
}
#[derive(Clone, Debug)]
pub struct ViewModel {
    pub playback: Option<crate::audio::Playback>,
    pub interactions: MessageInteractions,
    pub editing: Option<EditingMessage>,
    pub list_offsets: std::collections::BTreeMap<crate::config::bindings::Context, usize>,
    pub help_scroll: usize,
    pub focus: Focus,
    pub account: Option<AccountId>,
    pub chats: Vec<ChatSummary>,
    pub chat: Option<ChatId>,
    pub messages: Vec<MessageRecord>,
    pub selected_message: Option<MessageKey>,
    /// Bottom message anchor, independent of the action selection. None follows latest.
    pub timeline_anchor: Option<MessageKey>,
    /// Wrapped rows above the anchor's end.
    pub message_scroll: usize,
    pub message_scroll_max: usize,
    pub message_page_rows: usize,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StickerChoice {
    Recent(Box<MessageRecord>),
    Local(Box<crate::media::outgoing::LocalImage>),
}
impl StickerChoice {
    /// Different chats/messages may contain the same sticker.
    pub fn content_id(&self) -> Option<String> {
        match self {
            Self::Local(image) => Some(image.id.clone()),
            Self::Recent(message) => match &message.body {
                MessageBody::Media(a) => {
                    Some(a.sha256.iter().map(|b| format!("{b:02x}")).collect())
                }
                MessageBody::LocalImage { image, .. } => Some(image.id.clone()),
                _ => None,
            },
        }
    }
}
#[derive(Clone, Debug)]
pub struct StickerPicker {
    pub items: Vec<StickerChoice>,
    pub selected: usize,
    pub loading: Option<RequestId>,
    pub reload: bool,
    pub sending: Option<RequestId>,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct EditingMessage {
    pub message: MessageRecord,
    pub editor: Editor,
    pub request: Option<RequestId>,
    pub error: Option<String>,
}
