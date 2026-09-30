use serde::{Deserialize, Serialize};

macro_rules! id {
    ($name:ident) => {
        #[derive(
            Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        pub struct $name(pub String);
        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }
        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.into())
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}
id!(AccountId);
id!(ChatId);
id!(ParticipantId);
id!(MessageId);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageKey {
    pub account: AccountId,
    pub chat: ChatId,
    pub sender: ParticipantId,
    pub id: MessageId,
    pub from_me: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuoteAvailability {
    Available,
    Missing,
    Unsupported,
    Deleted,
    Expired,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Quote {
    pub key: MessageKey,
    pub preview: String,
    pub availability: QuoteAvailability,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub text: String,
    #[serde(default)]
    pub attachment: Option<Box<crate::media::outgoing::LocalImage>>,
    pub reply: Option<Quote>,
    pub revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundText {
    pub key: MessageKey,
    pub draft: Draft,
    pub created_at_ms: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendState {
    Sending,
    Sent,
    Delivered,
    Read,
    Failed,
    Unconfirmed,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConnectionState {
    #[default]
    Connecting,
    PairingRequired,
    Connected,
    Reconnecting,
    Disconnected,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBody {
    Text(String),
    LocalImage {
        image: Box<crate::media::outgoing::LocalImage>,
        caption: String,
    },
    Media(Box<crate::media::Attachment>),
    Unsupported {
        kind: String,
        caption: Option<String>,
    },
    Deleted,
    Expired,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageRecord {
    pub key: MessageKey,
    pub body: MessageBody,
    pub quote: Option<Quote>,
    pub created_at_ms: i64,
    pub edited_at_ms: Option<i64>,
    pub expires_at_ms: Option<i64>,
    pub send_state: Option<SendState>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageChange {
    Upsert(MessageRecord),
    Edit {
        key: MessageKey,
        /// Replacement message text, or the caption of an existing media body.
        text: String,
        edited_at_ms: i64,
    },
    Delete {
        key: MessageKey,
    },
    Expire {
        key: MessageKey,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageSource {
    Live,
    History,
}
#[derive(Clone, Debug)]
pub struct MessageBatch {
    pub account: AccountId,
    pub source: MessageSource,
    pub changes: Vec<MessageChange>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ReceiptState {
    Delivered,
    Read,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub key: MessageKey,
    pub recipient: ParticipantId,
    pub state: ReceiptState,
    pub at_ms: i64,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSummary {
    pub account: AccountId,
    pub chat: ChatId,
    pub name: String,
    /// Fallback=0, push name=1, history name=2, saved contact/group subject=3.
    #[serde(default)]
    pub name_priority: u8,
    pub phone: Option<String>,
    pub is_group: bool,
    pub preview: String,
    pub latest_at_ms: i64,
    pub unread: u32,
    pub has_draft: bool,
}
#[derive(Clone, Debug)]
pub struct ChatSnapshot {
    pub summary: ChatSummary,
    pub messages: Vec<MessageRecord>,
    pub draft: Draft,
    pub receipts: Vec<Receipt>,
    pub has_older: bool,
    pub has_newer: bool,
}
#[derive(Clone, Debug)]
pub struct MessageSearchHit {
    pub key: MessageKey,
    pub created_at_ms: i64,
    pub preview: String,
    /// First matching grapheme in the preview, before display sanitization.
    pub match_grapheme: usize,
}
#[derive(Clone, Debug, Default)]
pub struct MessageSearchPage {
    pub hits: Vec<MessageSearchHit>,
    pub has_more: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PageDirection {
    Before,
    AtOrBefore,
    After,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageCursor {
    pub direction: PageDirection,
    pub created_at_ms: i64,
    pub key: MessageKey,
}
#[derive(Clone, Debug)]
pub struct StoreChange {
    pub account: AccountId,
    pub chats: Vec<ChatId>,
}
