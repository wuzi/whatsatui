mod interactions;
mod merge;
mod notifications;
pub mod paths;
mod records;
mod search;
mod stickers;
mod worker;
use crate::app::model::*;
use diesel::sqlite::SqliteConnection;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Another instance owns this data directory")]
    Locked,
    #[error("Local storage worker stopped")]
    Unavailable,
    #[error("Cannot access local data files")]
    Io(#[from] std::io::Error),
    #[error("Cannot open the local database")]
    Connection(#[from] diesel::ConnectionError),
    #[error("Cannot commit or read local data")]
    Database(#[from] diesel::result::Error),
    #[error("Local data has an unsupported format")]
    Format(#[from] serde_json::Error),
    #[error("{0}")]
    Mutation(&'static str),
    #[error("Local data has an invalid identity or revision")]
    InvalidData,
}
type Job = Box<dyn FnOnce(&mut SqliteConnection) + Send>;
#[derive(Clone)]
pub struct Store {
    tx: mpsc::Sender<Job>,
    data_dir: std::sync::Arc<PathBuf>,
    mute_changes: tokio::sync::watch::Sender<()>,
}
impl Store {
    pub async fn open(path: PathBuf) -> Result<Self, StoreError> {
        let location = path.clone();
        let (tx, mut rx) = mpsc::channel::<Job>(64);
        let (ready, wait) = oneshot::channel();
        std::thread::Builder::new()
            .name("chat-storage".into())
            .spawn(move || match worker::open(&path) {
                Ok(mut connection) => {
                    if ready.send(Ok(())).is_err() {
                        return;
                    }
                    while let Some(job) = rx.blocking_recv() {
                        job(&mut connection);
                    }
                }
                Err(e) => {
                    let _ = ready.send(Err(e));
                }
            })?;
        wait.await.map_err(|_| StoreError::Unavailable)??;
        let location = std::fs::canonicalize(location)?;
        let data_dir =
            std::sync::Arc::new(location.parent().ok_or(StoreError::InvalidData)?.to_owned());
        Ok(Self {
            tx,
            data_dir,
            mute_changes: tokio::sync::watch::channel(()).0,
        })
    }
    pub(crate) fn mute_changes(&self) -> tokio::sync::watch::Receiver<()> {
        self.mute_changes.subscribe()
    }
    pub fn data_dir(&self) -> &std::path::Path {
        &self.data_dir
    }
    pub(crate) async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut SqliteConnection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        let (tx, rx) = oneshot::channel();
        self.tx
            .send(Box::new(move |conn| {
                let _ = tx.send(f(conn));
            }))
            .await
            .map_err(|_| StoreError::Unavailable)?;
        rx.await.map_err(|_| StoreError::Unavailable)?
    }
    pub async fn stage_mutation(
        &self,
        attempt: MutationAttempt,
        now_ms: i64,
    ) -> Result<MutationAttempt, StoreError> {
        self.call(move |c| interactions::stage(c, attempt, now_ms))
            .await
    }
    pub async fn finish_mutation(
        &self,
        account: AccountId,
        id: String,
        state: MutationState,
    ) -> Result<StoreChange, StoreError> {
        self.call(move |c| interactions::finish(c, &account, &id, state))
            .await
    }
    pub async fn apply_batch(&self, batch: MessageBatch) -> Result<StoreChange, StoreError> {
        self.call(move |c| worker::apply(c, batch)).await
    }
    pub(crate) async fn apply_batch_with_incoming(
        &self,
        batch: MessageBatch,
    ) -> Result<(StoreChange, Vec<MessageRecord>), StoreError> {
        self.call(move |c| worker::apply_with_incoming(c, batch))
            .await
    }
    pub async fn save_draft(
        &self,
        account: AccountId,
        chat: ChatId,
        draft: Draft,
    ) -> Result<(), StoreError> {
        self.call(move |c| worker::save_draft(c, &account, &chat, &draft))
            .await
    }
    pub async fn stage_outgoing(&self, message: OutboundText) -> Result<(), StoreError> {
        self.call(move |c| worker::stage(c, message, true)).await
    }
    /// Commit an explicit resend while preserving the conversation's current draft.
    pub async fn stage_resend(&self, message: OutboundText) -> Result<(), StoreError> {
        self.call(move |c| worker::stage(c, message, false)).await
    }
    pub async fn snapshot(
        &self,
        account: AccountId,
        chat: ChatId,
        cursor: Option<PageCursor>,
    ) -> Result<ChatSnapshot, StoreError> {
        self.call(move |c| worker::snapshot(c, &account, &chat, cursor))
            .await
    }
    pub async fn recent_stickers(
        &self,
        account: AccountId,
        now_ms: i64,
    ) -> Result<Vec<MessageRecord>, StoreError> {
        self.call(move |c| stickers::recent(c, &account, now_ms))
            .await
    }
    pub async fn list_chats(&self, account: AccountId) -> Result<Vec<ChatSummary>, StoreError> {
        self.call(move |c| worker::list(c, &account)).await
    }
    pub async fn get_message(&self, key: MessageKey) -> Result<Option<MessageRecord>, StoreError> {
        self.call(move |c| worker::get(c, &key)).await
    }
    pub async fn search_messages(
        &self,
        account: AccountId,
        chat: ChatId,
        query: String,
        now_ms: i64,
    ) -> Result<MessageSearchPage, StoreError> {
        self.call(move |c| search::messages(c, &account, &chat, &query, now_ms))
            .await
    }
    pub async fn recover_sends(&self, account: AccountId) -> Result<(), StoreError> {
        self.call(move |c| worker::recover(c, &account)).await
    }
    pub async fn set_send_state(
        &self,
        key: MessageKey,
        state: SendState,
    ) -> Result<StoreChange, StoreError> {
        self.call(move |c| worker::set_state(c, &key, state)).await
    }
    pub async fn record_receipt(&self, receipt: Receipt) -> Result<StoreChange, StoreError> {
        self.call(move |c| worker::receipt(c, receipt)).await
    }
    pub async fn upsert_chats(
        &self,
        account: AccountId,
        chats: Vec<ChatSummary>,
    ) -> Result<StoreChange, StoreError> {
        let mute_changes = self.mute_changes.clone();
        self.call(move |c| {
            let (change, mute_changed) = worker::upsert_chats(c, &account, chats)?;
            if mute_changed {
                mute_changes.send_replace(());
            }
            Ok(change)
        })
        .await
    }
    pub async fn merge_alias(
        &self,
        account: AccountId,
        alias: ParticipantId,
        canonical: ParticipantId,
    ) -> Result<StoreChange, StoreError> {
        let mute_changes = self.mute_changes.clone();
        self.call(move |c| {
            let change = merge::merge_alias(c, &account, &alias, &canonical)?;
            if !change.chats.is_empty() {
                mute_changes.send_replace(());
            }
            Ok(change)
        })
        .await
    }
    pub async fn mark_read(
        &self,
        account: AccountId,
        chat: ChatId,
        keys: Vec<MessageKey>,
    ) -> Result<StoreChange, StoreError> {
        self.call(move |c| merge::mark_read(c, &account, &chat, keys))
            .await
    }
    pub async fn expire(&self, account: AccountId, now_ms: i64) -> Result<StoreChange, StoreError> {
        self.call(move |c| merge::expire(c, &account, now_ms)).await
    }
    pub async fn stored_outbound(
        &self,
        mut message: OutboundText,
    ) -> Result<OutboundText, StoreError> {
        self.call(move |c| {
            let row = worker::get(c, &message.key)?.ok_or(StoreError::InvalidData)?;
            let (text, attachment) = match row.body {
                MessageBody::Text(text) => (text, None),
                MessageBody::LocalImage { image, caption } => (caption, Some(image)),
                _ => return Err(StoreError::InvalidData),
            };
            message.draft.attachment = attachment;
            message.key = row.key;
            message.draft.text = text;
            message.draft.reply = row.quote;
            message.created_at_ms = row.created_at_ms;
            Ok(message)
        })
        .await
    }
    pub async fn flush(&self) -> Result<(), StoreError> {
        self.call(|c| {
            use diesel::connection::SimpleConnection;
            c.batch_execute("PRAGMA wal_checkpoint(PASSIVE)")?;
            Ok(())
        })
        .await
    }
}
