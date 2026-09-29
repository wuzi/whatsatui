mod bridge;
pub mod demo;
mod durability;
pub mod encode;
mod media;
mod normalize;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Invalid conversation or reply identity")]
    InvalidIdentity,
    #[error("WhatsApp service failed; retry by restarting the application")]
    Service(#[source] anyhow::Error),
    #[error("WhatsApp service stopped")]
    Stopped,
}

mod native;
use crate::{app::model::*, storage::Store};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

#[derive(Clone, Debug)]
pub enum BackendCommand {
    PrepareText {
        request: RequestId,
        chat: ChatId,
        draft: Draft,
    },
    Transmit(OutboundText),
    MarkRead(Vec<MessageKey>),
}
#[derive(Clone, Debug)]
pub enum BackendEvent {
    AccountKnown(AccountId),
    ConnectionChanged {
        state: ConnectionState,
        reason: Option<String>,
    },
    PairingQr {
        content: String,
        expires_at: Instant,
    },
    HistoryProgress(Option<u32>),
    StoreChanged(StoreChange),
    Prepared {
        request: RequestId,
        message: OutboundText,
    },
    PreparationFailed {
        request: RequestId,
        reason: String,
    },
    SendOutcome {
        key: MessageKey,
        state: SendState,
    },
    LocalError(Option<String>),
    Stopped,
}
pub struct BackendHandle {
    pub commands: mpsc::Sender<BackendCommand>,
    pub events: mpsc::Receiver<BackendEvent>,
    pub control: BackendControl,
    pub media: std::sync::Arc<dyn crate::media::Downloader>,
}
pub struct BackendControl {
    stop: Option<oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<Result<(), BackendError>>>,
}
impl BackendControl {
    pub fn new(
        stop: oneshot::Sender<()>,
        task: tokio::task::JoinHandle<Result<(), BackendError>>,
    ) -> Self {
        Self {
            stop: Some(stop),
            task: Some(task),
        }
    }
    pub async fn shutdown(mut self) -> Result<(), BackendError> {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Some(task) = self.task.take() {
            task.await.map_err(|_| BackendError::Stopped)??;
        }
        Ok(())
    }
}
impl Drop for BackendControl {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
    }
}
pub async fn start(session_path: PathBuf, store: Store) -> Result<BackendHandle, BackendError> {
    native::start(session_path, store).await
}
