//! Tauri commands for the slot board GUI.

use orchestrator::config::{BackendKind, BackendProfile, SlotConfig};
use orchestrator::core::SlotBoardItem;
use serde::{Deserialize, Serialize};
use tauri::State;

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
    state
        .orchestrator
        .mcp_setup_command(state.bearer_token.as_str())
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

#[tauri::command]
pub async fn list_subscription_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<crate::sidecar::AuthAccount>, String> {
    // If not running, return empty with no hard error — GUI shows status separately.
    match state.sidecar.client().await.list_auth_files().await {
        Ok(a) => Ok(a),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("not running")
                || msg.contains("connection")
                || msg.contains("timed out")
                || msg.contains("error sending")
            {
                Ok(vec![])
            } else {
                Err(msg)
            }
        }
    }
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
