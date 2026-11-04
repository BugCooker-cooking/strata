//! Portable AI protocol and orchestration. Secrets, transport, file pickers and
//! context grants belong to a trusted host; model JSON can never create them.
pub mod catalog;
pub mod provider;
pub mod session;
pub mod tools;

pub use provider::{Message, ModelReply, ProviderConfig, ProviderKind, ToolCall, Usage};
pub use session::{ContextGrant, ExecutionMode, Session, SessionOptions, Step, ToolHost};

#[derive(Debug, thiserror::Error)]
pub enum AiError {
    #[error("invalid AI request: {0}")]
    Invalid(String),
    #[error("model protocol error: {0}")]
    Protocol(String),
    #[error("AI turn stopped or superseded")]
    Stopped,
    #[error("AI turn time or step limit reached")]
    Limit,
    #[error("context authorization does not match this document and destination")]
    Permission,
    #[error("host operation failed: {0}")]
    Host(String),
}

pub(crate) fn parse<T: serde::de::DeserializeOwned>(
    value: serde_json::Value,
) -> Result<T, AiError> {
    serde_json::from_value(value).map_err(|e| AiError::Invalid(e.to_string()))
}
