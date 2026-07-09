//! Shared Tauri application state hosting the M1 orchestrator + MCP server + sidecar.

use std::path::PathBuf;
use std::sync::Arc;

use orchestrator::core::Orchestrator;
use orchestrator::mcp::server::{load_bearer_token, serve_forever, McpState};
use orchestrator::{
    write_example_if_missing, ConversationStore, KeyringSecretStore, LoadedConfig, SecretStore,
    SlotRegistry, UsageLog, USAGE_DEFAULT_MAX_BYTES,
};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::notify_bridge::NotifyBridge;
use crate::sidecar::{CliproxySettings, SidecarManager, SidecarPaths};

/// Process-wide app state shared with Tauri commands.
pub struct AppState {
    pub orchestrator: Orchestrator,
    pub bearer_token: Arc<String>,
    pub listen: String,
    pub config_path: PathBuf,
    /// MCP server join handle (kept so the task is not dropped).
    pub _mcp_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    pub sidecar: Arc<SidecarManager>,
    pub notify: Arc<NotifyBridge>,
    /// Extra Ollama hosts (beyond localhost:11434), persisted as JSON.
    pub ollama_hosts_path: PathBuf,
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
                    let generated = Uuid::new_v4().to_string();
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

        // Usage log under app config dir.
        let usage_path = usage_log_path(&config_path);
        let usage = Arc::new(UsageLog::open(&usage_path, USAGE_DEFAULT_MAX_BYTES)?);

        let notify = Arc::new(NotifyBridge::new());
        let notify_for_hook = notify.clone();
        let hook: orchestrator::WorkerUnavailableHook = Arc::new(move |slot, reason| {
            notify_for_hook.on_worker_unavailable(slot, reason);
        });

        let orchestrator = Orchestrator::new(registry.clone(), conversations, secrets)
            .with_usage_log(usage)
            .with_unavailable_hook(hook);

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

        let paths = SidecarPaths::resolve(&config_path)?;
        paths.ensure_dirs()?;
        let settings = paths.load_or_init_settings().unwrap_or_else(|_| {
            let s = CliproxySettings::generate_fresh();
            let _ = paths.save_settings(&s);
            s
        });
        let _ = paths.write_proxy_config(&settings);
        let sidecar = Arc::new(SidecarManager::new(paths, settings, registry));
        if let Err(e) = sidecar.maybe_autostart().await {
            tracing::warn!("CLIProxyAPI autostart skipped: {e:#}");
        }
        sidecar.spawn_supervisor_loop();
        if let Err(e) = sidecar.sync_profiles().await {
            tracing::debug!("subscription profile sync: {e:#}");
        }

        let ollama_hosts_path = ollama_hosts_path(&config_path);

        Ok(Self {
            orchestrator,
            bearer_token: Arc::new(bearer),
            listen,
            config_path,
            _mcp_handle: Mutex::new(Some(handle)),
            sidecar,
            notify,
            ollama_hosts_path,
        })
    }
}

fn app_data_root(slots_config: &std::path::Path) -> PathBuf {
    if let Some(cfg) = dirs::config_dir() {
        cfg.join("orchestrator")
    } else if let Some(p) = slots_config.parent() {
        p.to_path_buf()
    } else {
        PathBuf::from(".")
    }
}

fn usage_log_path(slots_config: &std::path::Path) -> PathBuf {
    app_data_root(slots_config).join("usage.jsonl")
}

fn ollama_hosts_path(slots_config: &std::path::Path) -> PathBuf {
    app_data_root(slots_config).join("ollama_hosts.json")
}

/// Resolve default slots.json path.
///
/// Installed apps should **not** write next to Program Files. Prefer:
/// 1. `ORCHESTRATOR_SLOTS` env
/// 2. Existing `./slots.json` (dev)
/// 3. `%AppData%/orchestrator/slots.json` (first-launch auto-create)
pub fn default_config_path() -> PathBuf {
    if let Ok(p) = std::env::var("ORCHESTRATOR_SLOTS") {
        return PathBuf::from(p);
    }
    // Dev convenience: use cwd slots.json when present.
    if let Ok(cwd) = std::env::current_dir() {
        let local = cwd.join("slots.json");
        if local.exists() {
            return local;
        }
    }
    // Production / clean machine: always under the user config dir.
    if let Some(cfg) = dirs::config_dir() {
        let dir = cfg.join("orchestrator");
        let _ = std::fs::create_dir_all(&dir);
        return dir.join("slots.json");
    }
    PathBuf::from("slots.json")
}
