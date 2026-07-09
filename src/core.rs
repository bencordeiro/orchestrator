//! Core orchestration: `delegate` and `list_slots`.

use std::sync::Arc;

use serde::Serialize;

use crate::backends::{invoke_slot, ChatMessage, ChatResponse};
use crate::conversation::ConversationStore;
use crate::error::{OrchestratorError, Result};
use crate::registry::{PublicSlot, SlotRegistry};
use crate::secrets::SecretStore;

/// Arguments for a `delegate` call.
#[derive(Debug, Clone, Default)]
pub struct DelegateRequest {
    pub task: String,
    pub slot: Option<String>,
    pub conversation_id: Option<String>,
    pub context: Option<String>,
    pub files: Option<Vec<String>>,
}

/// Successful `delegate` result returned to the MCP caller.
#[derive(Debug, Clone, Serialize)]
pub struct DelegateResult {
    pub conversation_id: String,
    pub response: String,
    /// Present only in internal/test builds — stripped from MCP tool output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend_id: Option<String>,
}

/// Headless orchestrator core shared by the binary and (later) Tauri.
#[derive(Clone)]
pub struct Orchestrator {
    registry: Arc<SlotRegistry>,
    conversations: Arc<ConversationStore>,
    secrets: Arc<dyn SecretStore>,
    http: reqwest::Client,
    /// When true, include `backend_id` in results (tests / debug).
    pub expose_backend_id: bool,
}

impl Orchestrator {
    pub fn new(
        registry: Arc<SlotRegistry>,
        conversations: Arc<ConversationStore>,
        secrets: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            registry,
            conversations,
            secrets,
            http: reqwest::Client::new(),
            expose_backend_id: false,
        }
    }

    pub fn registry(&self) -> &SlotRegistry {
        &self.registry
    }

    pub fn conversations(&self) -> &ConversationStore {
        &self.conversations
    }

    /// List slot names + capability descriptions only.
    pub fn list_slots(&self) -> Result<Vec<PublicSlot>> {
        self.registry.list_public()
    }

    /// Delegate a task to the resolved slot backend.
    ///
    /// Slot is resolved **at call time** every time — never cached from startup.
    pub async fn delegate(&self, req: DelegateRequest) -> Result<DelegateResult> {
        let slot_name = req
            .slot
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("worker")
            .to_string();

        // Critical: resolve on every call.
        let slot = match self.registry.resolve(&slot_name) {
            Ok(s) => s,
            Err(e) => {
                return Err(OrchestratorError::WorkerUnavailable(format!(
                    "{}",
                    e.to_caller_message().trim_start_matches("worker unavailable: ")
                )));
            }
        };

        // Conversation continuity.
        let (conversation_id, mut history) = if let Some(ref id) = req.conversation_id {
            let conv = self.conversations.get(id)?;
            (conv.id, conv.messages)
        } else {
            let conv = self.conversations.create()?;
            (conv.id, Vec::new())
        };

        // Build the new user turn (task + optional context/files).
        let mut user_parts = vec![req.task.clone()];
        if let Some(ctx) = req.context.as_ref().filter(|c| !c.is_empty()) {
            user_parts.push(format!("\n\n## Additional context\n{ctx}"));
        }
        if let Some(files) = req.files.as_ref().filter(|f| !f.is_empty()) {
            user_parts.push(format!(
                "\n\n## Referenced files\n{}",
                files
                    .iter()
                    .map(|f| format!("- {f}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        let user_message = ChatMessage::user(user_parts.join(""));
        history.push(user_message.clone());

        // Invoke backend. Fallback chains exist in schema but are off by default.
        let chat_result = invoke_slot(&slot, &history, self.secrets.as_ref(), &self.http).await;

        // Optional explicit fallback only when enable_fallback is true.
        let chat_result = match chat_result {
            Ok(r) => Ok(r),
            Err(e) if slot.enable_fallback => {
                if let Some(chain) = slot.fallback.as_ref() {
                    let mut last_err = e;
                    let mut succeeded: Option<ChatResponse> = None;
                    for fb in chain {
                        match self.registry.resolve(fb) {
                            Ok(fb_slot) => {
                                match invoke_slot(
                                    &fb_slot,
                                    &history,
                                    self.secrets.as_ref(),
                                    &self.http,
                                )
                                .await
                                {
                                    Ok(r) => {
                                        succeeded = Some(r);
                                        break;
                                    }
                                    Err(err) => last_err = err,
                                }
                            }
                            Err(err) => last_err = err,
                        }
                    }
                    match succeeded {
                        Some(r) => Ok(r),
                        None => Err(last_err),
                    }
                } else {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        };

        let response = match chat_result {
            Ok(r) => r,
            Err(e) => {
                // Persist the user message even on failure so retries can continue
                // the same thread if desired? Spec says return clear error — we
                // do NOT append failed turns, so the conversation stays clean.
                let _ = self.conversations.get(&conversation_id); // ensure id remains valid
                return Err(match e {
                    OrchestratorError::WorkerUnavailable(r) => {
                        OrchestratorError::WorkerUnavailable(r)
                    }
                    other => OrchestratorError::WorkerUnavailable(
                        other
                            .to_caller_message()
                            .trim_start_matches("worker unavailable: ")
                            .to_string(),
                    ),
                });
            }
        };

        // Persist user + assistant turns.
        self.conversations.append(
            &conversation_id,
            &[user_message, ChatMessage::assistant(&response.content)],
            Some(&slot_name),
        )?;

        Ok(DelegateResult {
            conversation_id,
            response: response.content,
            backend_id: if self.expose_backend_id {
                Some(response.backend_id)
            } else {
                None
            },
        })
    }
}
