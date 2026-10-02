mod mpv;
mod worker;
use crate::{
    app::model::{MessageRecord, RequestId},
    media::Downloader,
    storage::Store,
};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Speed {
    #[default]
    Normal,
    OneHalf,
    Double,
    /// A speed chosen using the native player's controls, in thousandths.
    Native(u32),
}
impl Speed {
    pub fn next(self) -> Self {
        match self {
            Self::Normal => Self::OneHalf,
            Self::OneHalf => Self::Double,
            Self::Double => Self::Normal,
            Self::Native(n) if n < 1500 => Self::OneHalf,
            Self::Native(n) if n < 2000 => Self::Double,
            Self::Native(_) => Self::Normal,
        }
    }
    pub fn value(self) -> f64 {
        match self {
            Self::Normal => 1.0,
            Self::OneHalf => 1.5,
            Self::Double => 2.0,
            Self::Native(n) => f64::from(n) / 1000.0,
        }
    }
    pub fn label(self) -> String {
        format!("{}x", self.value())
    }
    pub(crate) fn from_milli(value: u32) -> Self {
        match value {
            1000 => Self::Normal,
            1500 => Self::OneHalf,
            2000 => Self::Double,
            value => Self::Native(value.clamp(10, 100_000)),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub id: RequestId,
    /// Distinguishes repeated control values from an acknowledged command.
    pub revision: u64,
    pub message: MessageRecord,
    pub paused: bool,
    pub speed: Speed,
}
impl Request {
    pub(crate) fn is_video(&self) -> bool {
        matches!(&self.message.body, crate::app::model::MessageBody::Media(a) if a.kind == crate::media::AttachmentKind::Video || a.is_gif())
    }
    pub(crate) fn same_source(&self, other: &Self) -> bool {
        self.id == other.id && self.message == other.message
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    Playing,
    Paused,
    Finished,
    Failed,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Playback {
    pub request: Request,
    pub phase: Phase,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub speed_milli: u32,
    pub error: Option<String>,
}
impl Playback {
    pub fn loading(request: Request) -> Self {
        let duration_ms = match &request.message.body {
            crate::app::model::MessageBody::Media(a) => a
                .audio
                .as_ref()
                .and_then(|m| m.seconds)
                .map(|s| u64::from(s) * 1000),
            _ => None,
        };
        Self {
            speed_milli: (request.speed.value() * 1000.0).round() as u32,
            request,
            phase: Phase::Loading,
            position_ms: 0,
            duration_ms,
            error: None,
        }
    }
    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Loading | Phase::Playing | Phase::Paused)
    }
    pub fn speed_label(&self) -> String {
        Speed::from_milli(self.speed_milli).label()
    }
}
pub struct Player {
    desired: watch::Sender<Option<Request>>,
    pub events: watch::Receiver<Option<Playback>>,
    task: tokio::task::JoinHandle<()>,
}
impl Player {
    pub fn start(store: Store, downloader: Arc<dyn Downloader>, executable: PathBuf) -> Self {
        let (desired, requests) = watch::channel(None);
        let (events, updates) = watch::channel(None);
        let task = tokio::spawn(worker::run(store, downloader, executable, requests, events));
        Self {
            desired,
            events: updates,
            task,
        }
    }
    pub fn set(&self, request: Option<Request>) {
        self.desired.send_replace(request);
    }
}
impl Drop for Player {
    fn drop(&mut self) {
        self.task.abort();
    }
}
