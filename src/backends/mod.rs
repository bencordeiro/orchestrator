//! Backend adapters that talk to worker model providers.

mod anthropic;
mod openai;

pub use anthropic::AnthropicBackend;
pub use openai::OpenAiCompatibleBackend;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::config::{BackendKind, SlotConfig};
use crate::error::{OrchestratorError, Result};
use crate::secrets::{resolve_auth, SecretStore};

/// A single chat message in orchestrator-normalized form.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// Result of a successful backend call.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    /// Opaque backend identity for tests/logs only — never exposed via list_slots.
    pub backend_id: String,
}

/// Trait implemented by every provider adapter.
#[async_trait]
pub trait Backend: Send + Sync {
    async fn chat(
        &self,
        model: &str,
        messages: &[ChatMessage],
        api_key: Option<&str>,
    ) -> Result<ChatResponse>;
}

/// Build the right adapter for a slot and invoke it.
///
/// Slot resolution (config + secrets) is the caller's responsibility so that
/// every `delegate` re-reads config at call time.
pub async fn invoke_slot(
    slot: &SlotConfig,
    messages: &[ChatMessage],
    secrets: &dyn SecretStore,
    http: &reqwest::Client,
) -> Result<ChatResponse> {
    let api_key = resolve_auth(secrets, slot.auth_ref.as_deref())?;

    match slot.backend {
        BackendKind::OpenaiCompatible => {
            let backend = OpenAiCompatibleBackend::new(http.clone(), &slot.base_url);
            backend
                .chat(&slot.model, messages, api_key.as_deref())
                .await
        }
        BackendKind::Anthropic => {
            let backend = AnthropicBackend::new(http.clone(), &slot.base_url);
            backend
                .chat(&slot.model, messages, api_key.as_deref())
                .await
        }
    }
}

/// Map HTTP status / transport failures into `worker unavailable` style errors.
pub(crate) fn map_http_error(context: &str, status: reqwest::StatusCode, body: &str) -> OrchestratorError {
    let reason = if status.as_u16() == 401 || status.as_u16() == 403 {
        format!("auth failed ({status}): {body}")
    } else if status.as_u16() == 429 {
        format!("quota exceeded ({status}): {body}")
    } else if status.is_server_error() {
        format!("backend error ({status}): {body}")
    } else {
        format!("{context} failed ({status}): {body}")
    };
    OrchestratorError::WorkerUnavailable(reason)
}
