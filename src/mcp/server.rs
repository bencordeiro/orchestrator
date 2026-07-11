//! Streamable HTTP MCP server with bearer-token protection.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::{
    extract::Request,
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Router,
};
use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler,
};
use rmcp::transport::{
    streamable_http_server::{session::local::LocalSessionManager, tower::StreamableHttpService},
    StreamableHttpServerConfig,
};
use serde::Deserialize;
use serde_json::json;

use crate::core::{DelegateRequest, Orchestrator};
use crate::secrets::SecretStore;

pub const DEFAULT_LISTEN: &str = "127.0.0.1:7420";

/// Shared state for auth middleware + handler factory.
#[derive(Clone)]
pub struct McpState {
    pub orchestrator: Orchestrator,
    pub bearer_token: Arc<String>,
}

/// MCP service exposing exactly two tools: `delegate` and `list_slots`.
#[derive(Clone)]
pub struct OrchestratorMcp {
    orchestrator: Orchestrator,
    // Populated by `#[tool_router]` / `#[tool_handler]`; read via generated code.
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct DelegateArgs {
    /// The task for the worker model to perform.
    pub task: String,
    /// Slot name. Defaults to `"worker"`.
    #[serde(default)]
    pub slot: Option<String>,
    /// Prior conversation id for continuity. Omit for a fresh stateless job.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Optional extra context string.
    #[serde(default)]
    pub context: Option<String>,
    /// Optional list of file paths/names for the worker to consider.
    #[serde(default)]
    pub files: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListSlotsArgs {}

#[tool_router]
impl OrchestratorMcp {
    pub fn new(orchestrator: Orchestrator) -> Self {
        Self {
            orchestrator,
            tool_router: Self::tool_router(),
        }
    }

    /// Delegate a task to a named worker slot. Slot is resolved at call time.
    #[tool(
        description = "Delegate a task to a worker model behind a named slot (default: worker). IMPORTANT: the worker cannot see your conversation, files, or tools — it receives ONLY what you put in this call. Write `task` as a complete, self-contained brief: state the goal, include all necessary code/data/context inline (use `context` for bulk material), specify the expected output format, and give concrete acceptance criteria. Be concise but leave nothing to be inferred. Returns the worker response and a conversation_id. Omit conversation_id for a fresh stateless job (preferred for independent tasks); pass a prior conversation_id only for genuinely multi-step work — the full thread history is re-sent to the worker on every continued call. On backend/auth/quota errors returns a clear 'worker unavailable' message — no automatic slot switching; report the failure and let the user swap the slot. Large tasks may legitimately take several minutes to return — a long wait is normal generation time, NOT a stall; do not cancel and retry, as that aborts the in-flight job. Concurrent delegations are supported."
    )]
    async fn delegate(
        &self,
        Parameters(args): Parameters<DelegateArgs>,
    ) -> Result<CallToolResult, McpError> {
        let req = DelegateRequest {
            task: args.task,
            slot: args.slot,
            conversation_id: args.conversation_id,
            context: args.context,
            files: args.files,
        };

        match self.orchestrator.delegate(req).await {
            Ok(result) => {
                // Do not expose backend_id over MCP (opacity requirement).
                let payload = json!({
                    "conversation_id": result.conversation_id,
                    "response": result.response,
                });
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    payload.to_string(),
                )]))
            }
            Err(e) => {
                let msg = e.to_caller_message();
                Ok(CallToolResult::error(vec![ContentBlock::text(msg)]))
            }
        }
    }

    /// List available slots with capability descriptions only.
    #[tool(
        description = "List available worker slots and their capability descriptions. Never reveals which vendor or model is behind a slot."
    )]
    async fn list_slots(
        &self,
        Parameters(_args): Parameters<ListSlotsArgs>,
    ) -> Result<CallToolResult, McpError> {
        match self.orchestrator.list_slots() {
            Ok(slots) => {
                let payload = json!({ "slots": slots });
                Ok(CallToolResult::success(vec![ContentBlock::text(
                    payload.to_string(),
                )]))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(
                e.to_caller_message(),
            )])),
        }
    }
}

#[tool_handler]
impl ServerHandler for OrchestratorMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(
                "orchestrator",
                env!("CARGO_PKG_VERSION"),
            ))
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Slot-based model delegation. Use `list_slots` to see workers, then `delegate` to send tasks. Workers are capable but context-blind: they see only what you send, so invest in a precise, self-contained brief — that is the main driver of result quality. Prefer fresh stateless jobs; use conversation_id only for multi-step threads. Slot backends can be hot-swapped server-side without restarting this MCP session."
                    .to_string(),
            )
    }
}

fn extract_bearer(headers: &HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer ").map(|s| s.to_string()))
}

async fn auth_middleware(
    axum::extract::State(expected): axum::extract::State<Arc<String>>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match extract_bearer(&headers) {
        Some(token) if token == *expected => Ok(next.run(request).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

async fn health() -> &'static str {
    "ok"
}

/// Build the axum router: public `/health`, protected `/mcp`.
pub fn build_router(state: McpState) -> Router {
    let orch = state.orchestrator.clone();
    let mcp_service = StreamableHttpService::new(
        move || Ok(OrchestratorMcp::new(orch.clone())),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );

    let protected = Router::new()
        .nest_service("/mcp", mcp_service)
        .layer(middleware::from_fn_with_state(
            state.bearer_token.clone(),
            auth_middleware,
        ));

    Router::new()
        .route("/health", get(health))
        .merge(protected)
}

/// Bind and serve until Ctrl-C.
pub async fn serve(state: McpState, listen: &str) -> anyhow::Result<()> {
    let addr: SocketAddr = listen.parse()?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("orchestrator MCP listening on http://{addr}/mcp");
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            tokio::signal::ctrl_c().await.ok();
            tracing::info!("shutdown signal received");
        })
        .await?;
    Ok(())
}

/// Bind and serve forever (used by the Tauri host process).
pub async fn serve_forever(state: McpState, listen: &str) -> anyhow::Result<()> {
    let addr: SocketAddr = listen.parse()?;
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("orchestrator MCP listening on http://{addr}/mcp");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Load the MCP bearer token from the secret store (required).
pub fn load_bearer_token(
    secrets: &dyn SecretStore,
    token_ref: &str,
) -> crate::error::Result<String> {
    match secrets.get(token_ref)? {
        Some(t) if !t.is_empty() => Ok(t),
        _ => Err(crate::error::OrchestratorError::Secret(format!(
            "MCP bearer token ref '{token_ref}' is missing — set it with `orchestrator secrets set {token_ref} <token>`"
        ))),
    }
}
