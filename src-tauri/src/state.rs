//! Shared Tauri application state hosting the M1 orchestrator + MCP server.

use std::path::PathBuf;
use std::sync::Arc;

use orchestrator::core::Orchestrator;
use orchestrator::mcp::server::{load_bearer_token, serve_forever, McpState};
use orchestrator::{
    write_example_if_missing, ConversationStore, KeyringSecretStore, LoadedConfig, SecretStore,
    SlotRegistry,
};
use tokio::sync::Mutex;

/// Process-wide app state shared with Tauri commands.
pub struct AppState {
    pub orchestrator: Orchestrator,
    pub bearer_token: Arc<String>,
    pub listen: String,
    pub config_path: PathBuf,
    /// MCP server join handle (kept so the task is not dropped).
    pub _mcp_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

impl AppState {
    pub async fn bootstrap(config_path: PathBuf) -> anyhow::Result<Self> {
        if !config_path.exists() {
            write_example_if_missing(&config_path)?;
            tracing::info!("created example config at {}", config_path.display());
        }

        let loaded = LoadedConfig::load(&config_path)?;
        let listen = loaded.file.listen.clone();
        let bearer_ref = loaded.file.bearer_token_ref.clone();
        let conv_dir = loaded.conversations_path();

        let secrets: Arc<dyn SecretStore> = Arc::new(KeyringSecretStore);
        let bearer = match load_bearer_token(secrets.as_ref(), &bearer_ref) {
            Ok(t) => t,
            Err(_) => {
                if let Ok(t) = std::env::var("ORCHESTRATOR_BEARER_TOKEN") {
                    tracing::warn!("using ORCHESTRATOR_BEARER_TOKEN env override");
                    t
                } else {
                    // First-run convenience: generate and store a token.
                    let generated = uuid_like_token();
                    secrets.set(&bearer_ref, &generated)?;
                    tracing::info!("generated and stored MCP bearer token in keychain");
                    generated
                }
            }
        };

        if let Ok(key) = std::env::var("ORCHESTRATOR_WORKER_API_KEY") {
            if secrets.get("worker_api_key")?.is_none() {
                secrets.set("worker_api_key", &key)?;
            }
        }

        let registry = Arc::new(SlotRegistry::open(&config_path)?);
        let conversations = Arc::new(ConversationStore::new(conv_dir)?);
        let orchestrator = Orchestrator::new(registry, conversations, secrets);

        let mcp_state = McpState {
            orchestrator: orchestrator.clone(),
            bearer_token: Arc::new(bearer.clone()),
        };

        let listen_for_server = listen.clone();
        let handle = tokio::spawn(async move {
            if let Err(e) = serve_forever(mcp_state, &listen_for_server).await {
                tracing::error!("MCP server exited: {e:#}");
            }
        });

        Ok(Self {
            orchestrator,
            bearer_token: Arc::new(bearer),
            listen,
            config_path,
            _mcp_handle: Mutex::new(Some(handle)),
        })
    }
}

fn uuid_like_token() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("orch-{nanos:x}")
}

/// Resolve default slots.json path (next to exe, or cwd, or config dir).
pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("ORCHESTRATOR_SLOTS") {
        return PathBuf::from(p);
    }
    // Prefer cwd for dev; fall back to config dir.
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let local = cwd.join("slots.json");
    if local.exists() {
        return local;
    }
    if let Some(cfg) = dirs::config_dir() {
        let p = cfg.join("orchestrator").join("slots.json");
        if p.exists() {
            return p;
        }
        // Ensure parent exists for first write.
        let _ = std::fs::create_dir_all(p.parent().unwrap());
        return p;
    }
    local
}
