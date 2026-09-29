pub mod encode;

#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    #[error("Invalid conversation or reply identity")]
    InvalidIdentity,
    #[error("WhatsApp service failed; retry by restarting the application")]
    Service(#[source] anyhow::Error),
    #[error("WhatsApp service stopped")]
    Stopped,
}
