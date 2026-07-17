//! Tauri commands for the slot board GUI.

use orchestrator::config::{BackendKind, BackendProfile, SlotConfig};
use orchestrator::core::SlotBoardItem;
use serde::{Deserialize, Serialize};
use tauri::{Manager, State};

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct ServerInfo {
    pub listen: String,
    pub mcp_url: String,
    pub health_url: String,
    pub config_path: String,
}

#[derive(Debug, Serialize)]
pub struct BackendProfileView {
    pub id: String,
    pub label: String,
    pub backend: String,
    pub base_url: String,
    pub model: String,
    pub auth_ref: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertSlotArgs {
    pub name: String,
    pub description: String,
    /// Backend profile id to assign (required for new slots).
    pub profile_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpsertProfileArgs {
    pub id: String,
    pub label: String,
    pub backend: String,
    pub base_url: String,
    pub model: String,
    pub auth_ref: Option<String>,
}

fn map_err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_server_info(state: State<'_, AppState>) -> Result<ServerInfo, String> {
    let listen = state.listen.clone();
    Ok(ServerInfo {
        mcp_url: format!("http://{listen}/mcp"),
        health_url: format!("http://{listen}/health"),
        listen,
        config_path: state.config_path.display().to_string(),
    })
}

#[tauri::command]
pub fn get_slot_board(state: State<'_, AppState>) -> Result<Vec<SlotBoardItem>, String> {
    state.orchestrator.slot_board().map_err(map_err)
}

/// Open the rotating-log directory in the OS file manager. Returns its path.
#[tauri::command]
pub fn open_log_dir(state: State<'_, AppState>) -> Result<String, String> {
    let dir = crate::state::log_dir(&state.config_path);
    std::fs::create_dir_all(&dir).map_err(map_err)?;

    #[cfg(target_os = "windows")]
    let spawned = std::process::Command::new("explorer").arg(&dir).spawn();
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(&dir).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let spawned = std::process::Command::new("xdg-open").arg(&dir).spawn();

    // explorer.exe returns a non-zero exit code even on success, so only the
    // spawn failing (file manager missing) is a real error worth surfacing.
    spawned.map_err(map_err)?;
    Ok(dir.display().to_string())
}

#[tauri::command]
pub fn get_backend_profiles(state: State<'_, AppState>) -> Result<Vec<BackendProfileView>, String> {
    let profiles = state.orchestrator.backend_profiles().map_err(map_err)?;
    Ok(profiles
        .into_iter()
        .map(|(id, p)| BackendProfileView {
            id,
            label: p.label,
            backend: match p.backend {
                BackendKind::OpenaiCompatible => "openai_compatible".into(),
                BackendKind::Anthropic => "anthropic".into(),
            },
            base_url: p.base_url,
            model: p.model,
            auth_ref: p.auth_ref,
        })
        .collect())
}

#[tauri::command]
pub fn swap_slot_backend(
    state: State<'_, AppState>,
    slot_name: String,
    profile_id: String,
) -> Result<(), String> {
    // Writes config then force_reload — takes effect on next delegate.
    state
        .orchestrator
        .registry()
        .assign_backend(&slot_name, &profile_id)
        .map_err(map_err)
}

#[tauri::command]
pub fn add_slot(state: State<'_, AppState>, args: UpsertSlotArgs) -> Result<(), String> {
    let cfg = state.orchestrator.registry().current().map_err(map_err)?;
    let profile = cfg
        .file
        .backend_profiles
        .get(&args.profile_id)
        .cloned()
        .ok_or_else(|| format!("unknown backend profile '{}'", args.profile_id))?;

    let mut slot = SlotConfig {
        description: args.description,
        backend: profile.backend,
        base_url: profile.base_url,
        model: profile.model,
        auth_ref: profile.auth_ref,
        fallback: None,
        enable_fallback: false,
    };
    // apply_to_slot is already reflected above; keep description as provided.
    let _ = &mut slot;

    state
        .orchestrator
        .registry()
        .upsert_slot(&args.name, slot)
        .map_err(map_err)
}

#[tauri::command]
pub fn remove_slot(state: State<'_, AppState>, name: String) -> Result<(), String> {
    state
        .orchestrator
        .registry()
        .remove_slot(&name)
        .map_err(map_err)
}

#[tauri::command]
pub fn update_slot_description(
    state: State<'_, AppState>,
    name: String,
    description: String,
) -> Result<(), String> {
    state
        .orchestrator
        .registry()
        .mutate(|file| {
            let slot = file
                .slots
                .get_mut(&name)
                .ok_or_else(|| orchestrator::OrchestratorError::UnknownSlot(name.clone()))?;
            slot.description = description;
            Ok(())
        })
        .map_err(map_err)
}

#[tauri::command]
pub fn upsert_backend_profile(
    state: State<'_, AppState>,
    args: UpsertProfileArgs,
) -> Result<(), String> {
    let backend = match args.backend.as_str() {
        "openai_compatible" => BackendKind::OpenaiCompatible,
        "anthropic" => BackendKind::Anthropic,
        other => return Err(format!("unknown backend kind '{other}'")),
    };
    state
        .orchestrator
        .registry()
        .upsert_backend_profile(
            &args.id,
            BackendProfile {
                label: args.label,
                backend,
                base_url: args.base_url,
                model: args.model,
                auth_ref: args.auth_ref.filter(|s| !s.is_empty()),
            },
        )
        .map_err(map_err)
}

#[tauri::command]
pub fn remove_backend_profile(state: State<'_, AppState>, id: String) -> Result<(), String> {
    state
        .orchestrator
        .registry()
        .remove_backend_profile(&id)
        .map_err(map_err)
}

#[tauri::command]
pub fn get_mcp_setup_command(state: State<'_, AppState>) -> Result<String, String> {
    // Backward-compatible: Claude only.
    state
        .orchestrator
        .mcp_setup_command(state.bearer_token.as_str())
        .map_err(map_err)
}

/// Dual setup commands for Claude Code and Codex CLI.
#[tauri::command]
pub fn get_mcp_setup_commands(
    state: State<'_, AppState>,
) -> Result<orchestrator::McpSetupCommands, String> {
    state
        .orchestrator
        .mcp_setup_commands(state.bearer_token.as_str())
        .map_err(map_err)
}

#[tauri::command]
pub fn set_secret(name: String, value: String) -> Result<(), String> {
    use orchestrator::{KeyringSecretStore, SecretStore};
    KeyringSecretStore.set(&name, &value).map_err(map_err)
}

// ── CLIProxyAPI / subscription accounts (M3) ─────────────────────────────

#[tauri::command]
pub async fn get_sidecar_status(
    state: State<'_, AppState>,
) -> Result<crate::sidecar::SidecarStatus, String> {
    Ok(state.sidecar.status_snapshot().await)
}

#[tauri::command]
pub async fn set_sidecar_enabled(state: State<'_, AppState>, enabled: bool) -> Result<(), String> {
    state.sidecar.set_enabled(enabled).await.map_err(map_err)
}

/// Structured accounts response — never errors for disabled/not-installed.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountsListResult {
    /// `ok` | `disabled` | `not_installed` | `stopped` | `unhealthy`
    pub state: String,
    pub message: String,
    pub accounts: Vec<crate::sidecar::AuthAccount>,
}

#[tauri::command]
pub async fn list_subscription_accounts(
    state: State<'_, AppState>,
) -> Result<AccountsListResult, String> {
    use crate::sidecar::SidecarPresence;
    let st = state.sidecar.status_snapshot().await;
    match st.presence {
        SidecarPresence::NotInstalled => {
            return Ok(AccountsListResult {
                state: "not_installed".into(),
                message: "CLIProxyAPI sidecar binary is not installed. Run scripts/download-cliproxy.ps1 or rebuild the release package.".into(),
                accounts: vec![],
            });
        }
        SidecarPresence::Disabled => {
            return Ok(AccountsListResult {
                state: "disabled".into(),
                message: "Subscriptions are off. Enable the sidecar to connect provider accounts.".into(),
                accounts: vec![],
            });
        }
        _ => {}
    }
    match state.sidecar.client().await.list_auth_files().await {
        Ok(a) => Ok(AccountsListResult {
            state: "ok".into(),
            message: if a.is_empty() {
                "No accounts connected yet.".into()
            } else {
                format!("{} account(s)", a.len())
            },
            accounts: a,
        }),
        Err(e) => {
            let msg = e.to_string();
            // Unreachable / not running — calm structured state, not a red error.
            if msg.contains("not running")
                || msg.contains("connection")
                || msg.contains("timed out")
                || msg.contains("error sending")
                || msg.contains("Connect")
            {
                let state_s = if st.presence == SidecarPresence::Unhealthy {
                    "unhealthy"
                } else {
                    "stopped"
                };
                Ok(AccountsListResult {
                    state: state_s.into(),
                    message: "Subscription sidecar is not reachable. It will start when enabled or when credentials are present.".into(),
                    accounts: vec![],
                })
            } else {
                // Unexpected: still return structured payload rather than hard Err.
                Ok(AccountsListResult {
                    state: "unhealthy".into(),
                    message: format!("Could not list accounts: {msg}"),
                    accounts: vec![],
                })
            }
        }
    }
}

#[tauri::command]
pub async fn list_proxy_models(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.sidecar.list_proxy_models().await.map_err(map_err)
}

#[tauri::command]
pub async fn set_account_model_override(
    state: State<'_, AppState>,
    account_id: String,
    model: String,
) -> Result<Vec<String>, String> {
    state
        .sidecar
        .set_account_model_override(&account_id, &model)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn clear_account_model_override(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<String>, String> {
    state
        .sidecar
        .clear_account_model_override(&account_id)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn start_subscription_oauth(
    state: State<'_, AppState>,
    provider: String,
) -> Result<(), String> {
    state
        .sidecar
        .start_oauth(&provider)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn disconnect_subscription_account(
    state: State<'_, AppState>,
    name: String,
) -> Result<(), String> {
    state
        .sidecar
        .disconnect_account(&name)
        .await
        .map_err(map_err)
}

#[tauri::command]
pub async fn sync_subscription_profiles(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.sidecar.sync_profiles().await.map_err(map_err)
}

#[tauri::command]
pub fn list_oauth_providers() -> Vec<String> {
    crate::sidecar::OAUTH_PROVIDERS
        .iter()
        .map(|(n, _)| (*n).to_string())
        .collect()
}

// ── M4: Ollama, fallback config, usage ───────────────────────────────────

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetFallbackArgs {
    pub name: String,
    pub enable_fallback: bool,
    pub fallback: Vec<String>,
}

#[tauri::command]
pub fn set_slot_fallback(state: State<'_, AppState>, args: SetFallbackArgs) -> Result<(), String> {
    state
        .orchestrator
        .registry()
        .mutate(|file| {
            let slot = file.slots.get_mut(&args.name).ok_or_else(|| {
                orchestrator::OrchestratorError::UnknownSlot(args.name.clone())
            })?;
            slot.enable_fallback = args.enable_fallback;
            slot.fallback = if args.fallback.is_empty() {
                None
            } else {
                Some(args.fallback)
            };
            Ok(())
        })
        .map_err(map_err)
}

#[tauri::command]
pub async fn discover_ollama_models(
    state: State<'_, AppState>,
) -> Result<Vec<orchestrator::OllamaModel>, String> {
    let extra = load_extra_hosts(&state.ollama_hosts_path);
    let models = orchestrator::discover_models(state.orchestrator.http(), &extra).await;
    Ok(models)
}

#[tauri::command]
pub fn get_ollama_extra_hosts(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    Ok(load_extra_hosts(&state.ollama_hosts_path))
}

#[tauri::command]
pub fn set_ollama_extra_hosts(
    state: State<'_, AppState>,
    hosts: Vec<String>,
) -> Result<(), String> {
    save_extra_hosts(&state.ollama_hosts_path, &hosts).map_err(map_err)
}

#[tauri::command]
pub fn create_ollama_profile(
    state: State<'_, AppState>,
    host: String,
    model: String,
) -> Result<String, String> {
    use orchestrator::config::{BackendKind, BackendProfile};
    use orchestrator::ollama::{normalize_host, profile_id_for_model};

    let host = normalize_host(&host);
    let id = profile_id_for_model(&host, &model);
    let label = format!("Ollama · {model} @ {host}");
    let profile = BackendProfile {
        label,
        backend: BackendKind::OpenaiCompatible,
        base_url: format!("{host}/v1"),
        model,
        auth_ref: None,
    };
    state
        .orchestrator
        .registry()
        .upsert_backend_profile(&id, profile)
        .map_err(map_err)?;
    Ok(id)
}

#[tauri::command]
pub fn get_recent_usage(
    state: State<'_, AppState>,
    limit: Option<usize>,
) -> Result<Vec<orchestrator::UsageEvent>, String> {
    let limit = limit.unwrap_or(50).min(500);
    match state.orchestrator.usage_log() {
        Some(log) => log.recent(limit).map_err(map_err),
        None => Ok(vec![]),
    }
}

fn load_extra_hosts(path: &std::path::Path) -> Vec<String> {
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    if raw.trim().is_empty() {
        return vec![];
    }
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn save_extra_hosts(path: &std::path::Path, hosts: &[String]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(hosts)?;
    std::fs::write(path, raw)?;
    Ok(())
}

// ── M5: manual update check (no background auto-update) ──────────────────

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub available: bool,
    pub current_version: String,
    pub latest_version: Option<String>,
    pub body: Option<String>,
    pub message: String,
}

/// Check GitHub Releases for an update. Does **not** download or install.
/// User must confirm via `install_update` if available.
#[tauri::command]
pub async fn check_for_updates(
    app: tauri::AppHandle,
) -> Result<UpdateCheckResult, String> {
    use tauri_plugin_updater::UpdaterExt;

    let current = app.package_info().version.to_string();
    let updater = app
        .updater()
        .map_err(|e| format!("updater unavailable: {e}"))?;

    match updater.check().await {
        Ok(Some(update)) => Ok(UpdateCheckResult {
            available: true,
            current_version: current,
            latest_version: Some(update.version.clone()),
            body: update.body.clone(),
            message: format!(
                "Update available: {} → {}",
                app.package_info().version,
                update.version
            ),
        }),
        Ok(None) => Ok(UpdateCheckResult {
            available: false,
            current_version: current.clone(),
            latest_version: None,
            body: None,
            message: format!("You are on the latest version ({current})"),
        }),
        Err(e) => {
            // Common before GitHub publish: endpoint 404
            Ok(UpdateCheckResult {
                available: false,
                current_version: current,
                latest_version: None,
                body: None,
                message: format!(
                    "Could not check for updates (is the GitHub release endpoint configured?): {e}"
                ),
            })
        }
    }
}

/// Download and install a pending update after the user confirmed in the GUI.
#[tauri::command]
pub async fn install_update(app: tauri::AppHandle) -> Result<String, String> {
    use tauri_plugin_updater::UpdaterExt;

    let updater = app
        .updater()
        .map_err(|e| format!("updater unavailable: {e}"))?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;

    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    // Relaunch after install (Windows NSIS/MSI updater flow).
    // process plugin exposes restart via JS; from Rust use Tauri's helper:
    tauri::process::restart(&app.env());
}
