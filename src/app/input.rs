use super::update::StoreCompletion;
use crate::whatsapp::BackendEvent;
#[derive(Debug)]
pub enum Input {
    Rendered(crate::ui::InteractionMap),
    StickerImported {
        request: super::model::RequestId,
        account: super::model::AccountId,
        chat: super::model::ChatId,
        result: Result<crate::media::outgoing::LocalImage, String>,
    },
    ClipboardRead {
        request: super::model::RequestId,
        account: super::model::AccountId,
        chat: super::model::ChatId,
        result: Result<crate::desktop::clipboard::Paste, String>,
    },
    ImageImported {
        request: super::model::RequestId,
        account: super::model::AccountId,
        chat: super::model::ChatId,
        result: Result<crate::media::outgoing::LocalImage, String>,
    },
    DesktopAction {
        request: super::model::RequestId,
        account: super::model::AccountId,
        result: Result<String, String>,
    },
    Terminal(crossterm::event::Event),
    Backend(BackendEvent),
    Store(StoreCompletion),
    Tick(i64),
    TimelineViewport(super::TimelineViewport),
}
