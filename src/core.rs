//! Core orchestration: `delegate` and `list_slots`.

use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;

use crate::backends::{invoke_slot, ChatMessage, ChatResponse};
use crate::config::{BackendProfile, SlotConfig};
use crate::conversation::ConversationStore;
use crate::error::{OrchestratorError, Result};
use crate::registry::{PublicSlot, SlotRegistry};
use crate::secrets::SecretStore;
use crate::status::{SlotStatus, StatusBoard};
use crate::usage::{UsageEvent, UsageLog};

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

/// Full slot card for the GUI (includes backend details — never sent over MCP).
#[derive(Debug, Clone, Serialize)]
pub struct SlotBoardItem {
    pub name: String,
    pub description: String,
    pub backend_kind: String,
    pub base_url: String,
    pub model: String,
    pub auth_ref: Option<String>,
    pub profile_id: Option<String>,
    pub profile_label: Option<String>,
    pub enable_fallback: bool,
    pub fallback: Vec<String>,
    pub last_call_at: Option<String>,
    pub last_latency_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_success: Option<bool>,
}

/// Optional hook when a worker becomes unavailable (tray notification, etc.).
pub type WorkerUnavailableHook = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Ready-to-paste MCP setup commands for supported clients.
#[derive(Debug, Clone, Serialize)]
pub struct McpSetupCommands {
    pub claude: String,
    pub codex: String,
}

/// Headless orchestrator core shared by the binary and the Tauri GUI.
#[derive(Clone)]
pub struct Orchestrator {
    registry: Arc<SlotRegistry>,
    conversations: Arc<ConversationStore>,
    secrets: Arc<dyn SecretStore>,
    status: Arc<StatusBoard>,
    usage: Option<Arc<UsageLog>>,
    on_unavailable: Option<WorkerUnavailableHook>,
    jobs: Arc<crate::jobs::JobStore>,
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
            status: Arc::new(StatusBoard::new()),
            usage: None,
            on_unavailable: None,
            jobs: Arc::new(crate::jobs::JobStore::new()),
            // Connect fast-fails on dead backends; NO overall request timeout —
            // long generations (minutes) are legitimate and must not be killed.
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(15))
                .pool_idle_timeout(std::time::Duration::from_secs(90))
                .build()
                .expect("http client"),
            expose_backend_id: false,
        }
    }

    pub fn with_status_board(mut self, status: Arc<StatusBoard>) -> Self {
        self.status = status;
        self
    }

    pub fn with_usage_log(mut self, usage: Arc<UsageLog>) -> Self {
        self.usage = Some(usage);
        self
    }

    pub fn with_unavailable_hook(mut self, hook: WorkerUnavailableHook) -> Self {
        self.on_unavailable = Some(hook);
        self
    }

    pub fn registry(&self) -> &SlotRegistry {
        &self.registry
    }

    pub fn conversations(&self) -> &ConversationStore {
        &self.conversations
    }

    pub fn status_board(&self) -> &StatusBoard {
        &self.status
    }

    pub fn usage_log(&self) -> Option<&UsageLog> {
        self.usage.as_deref()
    }

    pub fn secrets(&self) -> &dyn SecretStore {
        self.secrets.as_ref()
    }

    pub fn jobs(&self) -> &crate::jobs::JobStore {
        &self.jobs
    }

    /// Start a delegation as a background job; returns the job id immediately.
    ///
    /// The generation runs in a spawned task; no connection stays open, so no
    /// client-side tool timeout can kill it. Poll with `jobs().get(id)`.
    pub fn delegate_background(&self, req: DelegateRequest) -> String {
        let slot_name = req
            .slot
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or("worker")
            .to_string();
        let job_id = self.jobs.start(&slot_name);
        let this = self.clone();
        let id = job_id.clone();
        tokio::spawn(async move {
            match this.delegate(req).await {
                Ok(result) => this.jobs.complete(&id, &result),
                Err(e) => this.jobs.fail(&id, e.to_caller_message()),
            }
        });
        job_id
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// List slot names + capability descriptions only.
    pub fn list_slots(&self) -> Result<Vec<PublicSlot>> {
        self.registry.list_public()
    }

    /// GUI slot board: slots + assigned backend + live status.
    pub fn slot_board(&self) -> Result<Vec<SlotBoardItem>> {
        let cfg = self.registry.current()?;
        let mut items: Vec<SlotBoardItem> = cfg
            .file
            .slots
            .iter()
            .map(|(name, slot)| {
                let (profile_id, profile_label) =
                    find_matching_profile(&cfg.file.backend_profiles, slot);
                let st = self.status.get(name);
                SlotBoardItem {
                    name: name.clone(),
                    description: slot.description.clone(),
                    backend_kind: backend_kind_str(slot.backend).to_string(),
                    base_url: slot.base_url.clone(),
                    model: slot.model.clone(),
                    auth_ref: slot.auth_ref.clone(),
                    profile_id,
                    profile_label,
                    enable_fallback: slot.enable_fallback,
                    fallback: slot.fallback.clone().unwrap_or_default(),
                    last_call_at: st.last_call_at.map(|t| t.to_rfc3339()),
                    last_latency_ms: st.last_latency_ms,
                    last_error: st.last_error,
                    last_success: st.last_success,
                }
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(items)
    }

    pub fn backend_profiles(&self) -> Result<Vec<(String, BackendProfile)>> {
        let cfg = self.registry.current()?;
        let mut out: Vec<_> = cfg.file.backend_profiles.into_iter().collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    pub fn slot_status(&self, slot: &str) -> SlotStatus {
        self.status.get(slot)
    }

    /// Build the Claude Code MCP setup command (includes bearer token).
    pub fn mcp_setup_command(&self, bearer_token: &str) -> Result<String> {
        Ok(self.mcp_setup_commands(bearer_token)?.claude)
    }

    /// Setup commands for Claude Code and Codex CLI (same MCP endpoint + bearer).
    pub fn mcp_setup_commands(&self, bearer_token: &str) -> Result<McpSetupCommands> {
        let cfg = self.registry.current()?;
        let listen = cfg.file.listen.clone();
        let host = if listen.starts_with("127.0.0.1:") {
            listen.replacen("127.0.0.1", "localhost", 1)
        } else {
            listen
        };
        let url = format!("http://{host}/mcp");
        let claude = format!(
            "claude mcp add --transport http orchestrator {url} --header \"Authorization: Bearer {bearer_token}\""
        );
        // Codex HTTP MCP via mcp-remote stdio shim (header for bearer).
        let codex = format!(
            "codex mcp add orchestrator -- npx -y mcp-remote {url} --header \"Authorization: Bearer {bearer_token}\""
        );
        Ok(McpSetupCommands { claude, codex })
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

        let started = Instant::now();

        let slot = match self.registry.resolve(&slot_name) {
            Ok(s) => s,
            Err(e) => {
                let msg = e
                    .to_caller_message()
                    .trim_start_matches("worker unavailable: ")
                    .to_string();
                let latency_ms = started.elapsed().as_millis() as u64;
                self.status.record_error(&slot_name, latency_ms, &msg);
                self.record_usage(
                    &slot_name,
                    None,
                    "",
                    "",
                    latency_ms,
                    false,
                    Some(msg.clone()),
                );
                self.fire_unavailable(&slot_name, &msg);
                return Err(OrchestratorError::WorkerUnavailable(msg));
            }
        };

        let (profile_id, _) =
            find_matching_profile(&self.registry.current()?.file.backend_profiles, &slot);

        let (conversation_id, mut history, is_fresh) = if let Some(ref id) = req.conversation_id {
            let conv = self.conversations.get(id).map_err(|e| {
                let latency_ms = started.elapsed().as_millis() as u64;
                let msg = e.to_string();
                self.status.record_error(&slot_name, latency_ms, &msg);
                self.record_usage(
                    &slot_name,
                    profile_id.as_deref(),
                    &slot.base_url,
                    &slot.model,
                    latency_ms,
                    false,
                    Some(msg.clone()),
                );
                self.fire_unavailable(&slot_name, &msg);
                e
            })?;
            (conv.id, conv.messages, false)
        } else {
            (ConversationStore::allocate_id(), Vec::new(), true)
        };

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

        // Track which backend actually answered (primary vs fallback).
        let mut used_base = slot.base_url.clone();
        let mut used_model = slot.model.clone();
        let mut used_profile = profile_id.clone();

        let chat_result = invoke_slot(&slot, &history, self.secrets.as_ref(), &self.http).await;

        // Fallback chains exist in schema but are **off by default**.
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
                                        used_base = fb_slot.base_url.clone();
                                        used_model = fb_slot.model.clone();
                                        let (pid, _) = find_matching_profile(
                                            &self.registry.current()?.file.backend_profiles,
                                            &fb_slot,
                                        );
                                        used_profile = pid;
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

        let latency_ms = started.elapsed().as_millis() as u64;

        let response = match chat_result {
            Ok(r) => r,
            Err(e) => {
                let msg = match e {
                    OrchestratorError::WorkerUnavailable(r) => r,
                    other => other
                        .to_caller_message()
                        .trim_start_matches("worker unavailable: ")
                        .to_string(),
                };
                self.status.record_error(&slot_name, latency_ms, &msg);
                self.record_usage(
                    &slot_name,
                    used_profile.as_deref(),
                    &used_base,
                    &used_model,
                    latency_ms,
                    false,
                    Some(msg.clone()),
                );
                self.fire_unavailable(&slot_name, &msg);
                return Err(OrchestratorError::WorkerUnavailable(msg));
            }
        };

        if is_fresh {
            self.conversations.create_with_messages(
                &conversation_id,
                &[user_message, ChatMessage::assistant(&response.content)],
                Some(&slot_name),
            )?;
        } else {
            self.conversations.append(
                &conversation_id,
                &[user_message, ChatMessage::assistant(&response.content)],
                Some(&slot_name),
            )?;
        }

        self.status.record_success(&slot_name, latency_ms);
        self.record_usage(
            &slot_name,
            used_profile.as_deref(),
            &used_base,
            &used_model,
            latency_ms,
            true,
            None,
        );

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

    fn record_usage(
        &self,
        slot: &str,
        profile_id: Option<&str>,
        base_url: &str,
        model: &str,
        latency_ms: u64,
        success: bool,
        reason: Option<String>,
    ) {
        if let Some(log) = &self.usage {
            let ev = UsageEvent {
                ts: chrono::Utc::now(),
                slot: slot.to_string(),
                profile_id: profile_id.map(|s| s.to_string()),
                base_url: base_url.to_string(),
                model: model.to_string(),
                latency_ms,
                success,
                reason,
            };
            if let Err(e) = log.record(&ev) {
                tracing::warn!("usage log write failed: {e}");
            }
        }
    }

    fn fire_unavailable(&self, slot: &str, reason: &str) {
        if let Some(hook) = &self.on_unavailable {
            hook(slot, reason);
        }
    }
}

fn backend_kind_str(k: crate::config::BackendKind) -> &'static str {
    match k {
        crate::config::BackendKind::OpenaiCompatible => "openai_compatible",
        crate::config::BackendKind::Anthropic => "anthropic",
    }
}

fn find_matching_profile(
    profiles: &std::collections::HashMap<String, BackendProfile>,
    slot: &SlotConfig,
) -> (Option<String>, Option<String>) {
    for (id, p) in profiles {
        if p.matches_slot(slot) {
            return (Some(id.clone()), Some(p.label.clone()));
        }
    }
    (None, None)
}
