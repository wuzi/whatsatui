use super::update::StoreCompletion;
use crate::whatsapp::BackendEvent;
#[derive(Debug)]
pub enum Input {
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
