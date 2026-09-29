use super::update::StoreCompletion;
use crate::whatsapp::BackendEvent;
#[derive(Debug)]
pub enum Input {
    Terminal(crossterm::event::Event),
    Backend(BackendEvent),
    Store(StoreCompletion),
    Tick(i64),
    TimelineViewport(super::TimelineViewport),
}
