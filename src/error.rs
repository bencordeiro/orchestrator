//! Shared error types for the orchestrator core.

use thiserror::Error;

/// Errors that can surface from core orchestration logic.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("worker unavailable: {0}")]
    WorkerUnavailable(String),

    #[error("unknown slot: {0}")]
    UnknownSlot(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("secret error: {0}")]
    Secret(String),

    #[error("conversation error: {0}")]
    Conversation(String),

    #[error("backend error: {0}")]
    Backend(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl OrchestratorError {
    /// Format suitable for returning to MCP callers.
    pub fn to_caller_message(&self) -> String {
        match self {
            OrchestratorError::WorkerUnavailable(reason) => {
                format!("worker unavailable: {reason}")
            }
            OrchestratorError::UnknownSlot(name) => {
                format!("worker unavailable: unknown slot '{name}'")
            }
            OrchestratorError::Backend(reason) => format!("worker unavailable: {reason}"),
            other => format!("worker unavailable: {other}"),
        }
    }
}

pub type Result<T> = std::result::Result<T, OrchestratorError>;
