//! Sidecar process lifecycle: spawn, health, restart with backoff, clean shutdown.

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use serde::Serialize;
use tokio::sync::RwLock;

use super::client::CliProxyClient;
use super::config::{CliproxySettings, SidecarPaths};
use super::profiles::sync_subscription_profiles;
use orchestrator::{KeyringSecretStore, SecretStore, SlotRegistry};

/// Keyring ref for the proxy API key used by openai_compatible profiles.
pub const PROXY_KEY_REF: &str = "cliproxy_proxy_key";

/// OAuth providers we can launch via CLIProxyAPI login flags.
pub const OAUTH_PROVIDERS: &[(&str, &str)] = &[
    ("claude", "-claude-login"),
    ("codex", "-codex-login"),
    ("codex_device", "-codex-device-login"),
    ("antigravity", "-antigravity-login"),
    ("kimi", "-kimi-login"),
    ("xai", "-xai-login"),
];

/// Coarse lifecycle state for calm UI rendering (no error spam when off).
#[derive(Debug, Clone, Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SidecarPresence {
    /// Binary missing from install / download not run.
    NotInstalled,
    /// User has not enabled subscriptions and no auth is present.
    #[default]
    Disabled,
    /// Enabled (or auth present) but process not running.
    Stopped,
    /// Process up, health OK.
    Running,
    /// Process up but health check failing.
    Unhealthy,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct SidecarStatus {
    pub enabled: bool,
    pub running: bool,
    pub healthy: bool,
    /// Structured presence for UI (`not_installed`, `disabled`, `stopped`, `running`, `unhealthy`).
    pub presence: SidecarPresence,
    pub binary_present: bool,
    pub has_auth_credentials: bool,
    pub version_pin: String,
    pub base_url: String,
    pub openai_base_url: String,
    pub port: u16,
    pub binary_path: String,
    pub config_path: String,
    pub last_error: Option<String>,
    pub restart_count: u32,
}

pub struct SidecarManager {
    paths: SidecarPaths,
    settings: Arc<RwLock<CliproxySettings>>,
    child: Mutex<Option<Child>>,
    pub status: Arc<RwLock<SidecarStatus>>,
    stop_flag: AtomicBool,
    supervise_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
    registry: Arc<SlotRegistry>,
}

impl SidecarManager {
    pub fn new(paths: SidecarPaths, settings: CliproxySettings, registry: Arc<SlotRegistry>) -> Self {
        let status = SidecarStatus {
            enabled: settings.enabled,
            version_pin: super::config::PINNED_VERSION.to_string(),
            base_url: paths.base_url(&settings),
            openai_base_url: paths.openai_base_url(&settings),
            port: settings.port,
            binary_path: paths.binary.display().to_string(),
            config_path: paths.config_yaml.display().to_string(),
            ..Default::default()
        };
        Self {
            paths,
            settings: Arc::new(RwLock::new(settings)),
            child: Mutex::new(None),
            status: Arc::new(RwLock::new(status)),
            stop_flag: AtomicBool::new(false),
            supervise_handle: Mutex::new(None),
            registry,
        }
    }

    pub fn paths(&self) -> &SidecarPaths {
        &self.paths
    }

    pub async fn settings_snapshot(&self) -> CliproxySettings {
        self.settings.read().await.clone()
    }

    pub async fn client(&self) -> CliProxyClient {
        let s = self.settings.read().await;
        CliProxyClient::new(
            self.paths.base_url(&s),
            s.management_key.clone(),
            s.proxy_api_key.clone(),
        )
    }

    /// Seed proxy API key into the OS keychain for openai_compatible profiles.
    pub fn seed_proxy_key_in_keychain(&self, settings: &CliproxySettings) -> Result<()> {
        let store = KeyringSecretStore;
        store.set(PROXY_KEY_REF, &settings.proxy_api_key)?;
        Ok(())
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<()> {
        {
            let mut s = self.settings.write().await;
            s.enabled = enabled;
            self.paths.save_settings(&s)?;
            self.paths.write_proxy_config(&s)?;
            self.seed_proxy_key_in_keychain(&s)?;
        }
        self.status.write().await.enabled = enabled;
        if enabled {
            self.ensure_running().await?;
        } else {
            self.stop().await?;
        }
        Ok(())
    }

    /// Start if enabled **or** if auth files / `sub-*` profiles already exist.
    ///
    /// Important: presence of credentials forces `enabled=true` and persists it.
    /// `stop()` / app quit must **never** write `enabled: false`.
    pub async fn maybe_autostart(&self) -> Result<()> {
        // Re-load settings from disk so we don't act on a stale in-memory snapshot.
        if let Ok(disk) = self.paths.load_or_init_settings() {
            *self.settings.write().await = disk;
        }

        let has_auth = self.paths.has_auth_credentials();
        let has_sub_profiles = self
            .registry
            .current()
            .map(|c| c.file.backend_profiles.keys().any(|k| k.starts_with("sub-")))
            .unwrap_or(false);
        let already_enabled = self.settings.read().await.enabled;

        let should = already_enabled || has_auth || has_sub_profiles;
        tracing::info!(
            already_enabled,
            has_auth,
            has_sub_profiles,
            should,
            auth_dir = %self.paths.auth_dir.display(),
            "cliproxy maybe_autostart"
        );

        if !should {
            return Ok(());
        }

        // Persist enabled=true whenever we autostart due to credentials.
        {
            let mut s = self.settings.write().await;
            if !s.enabled {
                s.enabled = true;
                self.paths.save_settings(&s)?;
                tracing::info!("cliproxy: persisted enabled=true (auth or sub-profiles present)");
            }
            self.paths.write_proxy_config(&s)?;
            // Keychain seed is best-effort; do not fail autostart if OS keyring is flaky.
            if let Err(e) = self.seed_proxy_key_in_keychain(&s) {
                tracing::warn!("cliproxy: seed proxy key failed (continuing): {e}");
            }
        }
        self.status.write().await.enabled = true;

        match self.ensure_running().await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Keep enabled=true so the next launch still tries.
                tracing::warn!("cliproxy: ensure_running failed (enabled stays true): {e:#}");
                Err(e)
            }
        }
    }

    pub async fn ensure_running(&self) -> Result<()> {
        self.stop_flag.store(false, Ordering::SeqCst);
        {
            let s = self.settings.read().await;
            self.paths.write_proxy_config(&s)?;
            self.seed_proxy_key_in_keychain(&s)?;
        }
        self.spawn_if_needed()?;
        // Wait briefly for health.
        for _ in 0..20 {
            if self.health_check().await.is_ok() {
                let mut st = self.status.write().await;
                st.running = true;
                st.healthy = true;
                st.last_error = None;
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let err = "sidecar started but health check timed out".to_string();
        self.status.write().await.last_error = Some(err.clone());
        Err(anyhow!(err))
    }

    fn spawn_if_needed(&self) -> Result<()> {
        let mut guard = self.child.lock().unwrap();
        if let Some(child) = guard.as_mut() {
            match child.try_wait() {
                Ok(None) => return Ok(()), // still running
                Ok(Some(_)) => {
                    *guard = None;
                }
                Err(_) => {
                    *guard = None;
                }
            }
        }
        if !self.paths.binary.exists() {
            return Err(anyhow!(
                "CLIProxyAPI binary not found at {} — run scripts/download-cliproxy.ps1 (pinned {})",
                self.paths.binary.display(),
                super::config::PINNED_VERSION
            ));
        }
        // config.yaml must already be written by ensure_running / set_enabled.
        let mut cmd = Command::new(&self.paths.binary);
        cmd.arg("-config")
            .arg(&self.paths.config_yaml)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .current_dir(&self.paths.root);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }
        let child = cmd
            .spawn()
            .with_context(|| format!("spawn {}", self.paths.binary.display()))?;
        *guard = Some(child);
        Ok(())
    }

    /// Background restart loop (call once from bootstrap with Arc).
    pub fn spawn_supervisor_loop(self: &Arc<Self>) {
        let this = Arc::clone(self);
        let handle = tokio::spawn(async move {
            let mut backoff = Duration::from_secs(1);
            loop {
                if this.stop_flag.load(Ordering::SeqCst) {
                    break;
                }
                let enabled = this.settings.read().await.enabled;
                if !enabled {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
                // Check child liveness.
                let dead = {
                    let mut g = this.child.lock().unwrap();
                    match g.as_mut() {
                        None => true,
                        Some(c) => match c.try_wait() {
                            Ok(None) => false,
                            Ok(Some(_)) => {
                                *g = None;
                                true
                            }
                            Err(_) => {
                                *g = None;
                                true
                            }
                        },
                    }
                };
                if dead {
                    match this.spawn_if_needed() {
                        Ok(()) => {
                            this.status.write().await.restart_count += 1;
                            this.status.write().await.last_error =
                                Some("sidecar restarted after crash".into());
                            backoff = Duration::from_secs(1);
                        }
                        Err(e) => {
                            this.status.write().await.last_error = Some(e.to_string());
                            tokio::time::sleep(backoff).await;
                            backoff = (backoff * 2).min(Duration::from_secs(30));
                            continue;
                        }
                    }
                }
                match this.health_check().await {
                    Ok(()) => {
                        let mut st = this.status.write().await;
                        st.running = true;
                        st.healthy = true;
                        if st.last_error.as_deref() == Some("sidecar restarted after crash") {
                            st.last_error = None;
                        }
                        backoff = Duration::from_secs(1);
                    }
                    Err(e) => {
                        let mut st = this.status.write().await;
                        st.healthy = false;
                        st.last_error = Some(e.to_string());
                    }
                }
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        });
        *self.supervise_handle.lock().unwrap() = Some(handle);
    }

    pub async fn health_check(&self) -> Result<()> {
        self.client().await.health().await
    }

    /// Kill the process only. **Does not** write `enabled: false` to settings.
    pub async fn stop(&self) -> Result<()> {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Some(h) = self.supervise_handle.lock().unwrap().take() {
            h.abort();
        }
        // Drop the std::sync::MutexGuard *before* any .await so the future stays Send.
        {
            let mut guard = self.child.lock().unwrap();
            if let Some(mut child) = guard.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
        let mut st = self.status.write().await;
        st.running = false;
        st.healthy = false;
        // Intentionally leave settings.enabled unchanged on disk.
        Ok(())
    }

    pub async fn status_snapshot(&self) -> SidecarStatus {
        let mut st = self.status.read().await.clone();
        let binary_present = self.paths.binary.exists();
        let has_auth = self.paths.has_auth_credentials();
        // Refresh running bit.
        let running = {
            let mut g = self.child.lock().unwrap();
            match g.as_mut() {
                Some(c) => matches!(c.try_wait(), Ok(None)),
                None => false,
            }
        };
        let healthy = if running {
            self.health_check().await.is_ok()
        } else {
            false
        };
        let s = self.settings.read().await;
        st.enabled = s.enabled;
        st.running = running;
        st.healthy = healthy;
        st.binary_present = binary_present;
        st.has_auth_credentials = has_auth;
        st.port = s.port;
        st.base_url = self.paths.base_url(&s);
        st.openai_base_url = self.paths.openai_base_url(&s);
        st.binary_path = self.paths.binary.display().to_string();
        st.config_path = self.paths.config_yaml.display().to_string();
        st.presence = if !binary_present {
            SidecarPresence::NotInstalled
        } else if !s.enabled && !has_auth {
            SidecarPresence::Disabled
        } else if running && healthy {
            SidecarPresence::Running
        } else if running {
            SidecarPresence::Unhealthy
        } else {
            SidecarPresence::Stopped
        };
        // Clear noisy last_error when calmly disabled / not installed.
        if matches!(
            st.presence,
            SidecarPresence::Disabled | SidecarPresence::NotInstalled
        ) {
            st.last_error = None;
        }
        st
    }

    /// Sync accounts → backend profiles (generic openai_compatible).
    pub async fn sync_profiles(&self) -> Result<Vec<String>> {
        let st = self.status_snapshot().await;
        if matches!(
            st.presence,
            SidecarPresence::NotInstalled | SidecarPresence::Disabled
        ) {
            return Ok(vec![]);
        }
        if !st.running && !st.healthy {
            if self.health_check().await.is_err() {
                return Err(anyhow!("subscription sidecar not running"));
            }
        }
        let client = self.client().await;
        let accounts = client.list_auth_files().await?;
        let models = client.list_models().await.unwrap_or_default();
        // If we see live accounts, force-enable so relaunch autostarts.
        if !accounts.is_empty() {
            let mut s = self.settings.write().await;
            if !s.enabled {
                s.enabled = true;
                self.paths.save_settings(&s)?;
            }
        }
        let (openai, overrides, settings_snap) = {
            let s = self.settings.read().await;
            (
                self.paths.openai_base_url(&s),
                s.model_overrides.clone(),
                s.clone(),
            )
        };
        let _ = self.seed_proxy_key_in_keychain(&settings_snap);
        let ids = sync_subscription_profiles(
            &self.registry,
            &accounts,
            &openai,
            PROXY_KEY_REF,
            &models,
            &overrides,
        )?;
        Ok(ids)
    }

    /// List models exposed by the running sidecar (`GET /v1/models`).
    pub async fn list_proxy_models(&self) -> Result<Vec<String>> {
        let st = self.status_snapshot().await;
        if !st.binary_present {
            return Err(anyhow!("sidecar not installed"));
        }
        if matches!(st.presence, SidecarPresence::Disabled) {
            return Err(anyhow!("subscriptions disabled"));
        }
        if self.health_check().await.is_err() {
            // Try start once if enabled/auth.
            let _ = self.maybe_autostart().await;
        }
        self.client().await.list_models().await
    }

    /// Persist a per-account model override and re-sync profiles.
    pub async fn set_account_model_override(
        &self,
        account_id: &str,
        model: &str,
    ) -> Result<Vec<String>> {
        {
            let mut s = self.settings.write().await;
            s.model_overrides
                .insert(account_id.to_string(), model.to_string());
            self.paths.save_settings(&s)?;
        }
        self.sync_profiles().await
    }

    pub async fn clear_account_model_override(&self, account_id: &str) -> Result<Vec<String>> {
        {
            let mut s = self.settings.write().await;
            s.model_overrides.remove(account_id);
            self.paths.save_settings(&s)?;
        }
        self.sync_profiles().await
    }

    /// Launch provider OAuth login (separate process; opens browser).
    pub async fn start_oauth(&self, provider: &str) -> Result<()> {
        // Ensure sidecar config exists and enable.
        {
            let mut s = self.settings.write().await;
            s.enabled = true;
            self.paths.save_settings(&s)?;
            self.paths.write_proxy_config(&s)?;
            self.seed_proxy_key_in_keychain(&s)?;
        }
        // Prefer having the main server running so auth files are watched.
        let _ = self.ensure_running().await;

        let flag = OAUTH_PROVIDERS
            .iter()
            .find(|(name, _)| *name == provider)
            .map(|(_, f)| *f)
            .ok_or_else(|| {
                anyhow!(
                    "unknown provider '{provider}'. supported: {}",
                    OAUTH_PROVIDERS
                        .iter()
                        .map(|(n, _)| *n)
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        if !self.paths.binary.exists() {
            return Err(anyhow!(
                "CLIProxyAPI binary missing at {}",
                self.paths.binary.display()
            ));
        }

        let mut cmd = Command::new(&self.paths.binary);
        cmd.arg("-config")
            .arg(&self.paths.config_yaml)
            .arg(flag)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Login process should show browser — allow window on Windows.
        cmd.spawn()
            .with_context(|| format!("spawn oauth {provider}"))?;
        Ok(())
    }

    pub async fn disconnect_account(&self, name: &str) -> Result<()> {
        let client = self.client().await;
        client.delete_auth_file(name).await?;
        // Also remove file if API left residue.
        let path = self.paths.auth_dir.join(name);
        if path.exists() {
            let _ = std::fs::remove_file(path);
        }
        let _ = self.sync_profiles().await;
        Ok(())
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::SeqCst);
        if let Ok(mut g) = self.child.lock() {
            if let Some(mut c) = g.take() {
                let _ = c.kill();
                let _ = c.wait();
            }
        }
    }
}

/// Test helper: spawn a stub HTTP server as a "sidecar".
#[cfg(test)]
pub fn write_stub_sidecar(dir: &std::path::Path) -> PathBuf {
    // Cross-platform-ish stub: a PowerShell script is awkward as Child executable.
    // Use a tiny python server script invoked via python.
    let script = dir.join("stub_sidecar.py");
    std::fs::write(
        &script,
        r#"
import sys, json
from http.server import BaseHTTPRequestHandler, HTTPServer

port = int(sys.argv[1]) if len(sys.argv) > 1 else 18319

class H(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path.startswith("/v0/management/auth-files"):
            body = b'{"files":[]}'
        elif self.path.startswith("/v1/models"):
            body = b'{"data":[],"object":"list"}'
        else:
            body = b'{"message":"CLI Proxy API Server","endpoints":[]}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    def log_message(self, *a):
        pass

HTTPServer(("127.0.0.1", port), H).serve_forever()
"#,
    )
    .unwrap();
    script
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator::SlotRegistry;
    use std::fs;
    use std::sync::Arc;
    use tempfile::tempdir;

    #[test]
    fn has_auth_credentials_detects_json() {
        let dir = tempdir().unwrap();
        let auth = dir.path().join("auth");
        fs::create_dir_all(&auth).unwrap();
        let paths = SidecarPaths {
            root: dir.path().to_path_buf(),
            config_yaml: dir.path().join("config.yaml"),
            auth_dir: auth.clone(),
            settings_json: dir.path().join("settings.json"),
            log_dir: dir.path().join("logs"),
            binary: dir.path().join("missing.exe"),
        };
        assert!(!paths.has_auth_credentials());
        fs::write(auth.join("claude-user.json"), "{}").unwrap();
        assert!(paths.has_auth_credentials());
    }

    #[tokio::test]
    async fn maybe_autostart_enables_when_auth_present() {
        let dir = tempdir().unwrap();
        let slots = dir.path().join("slots.json");
        fs::write(
            &slots,
            r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://x/v1","model":"m"}}}"#,
        )
        .unwrap();
        let reg = Arc::new(SlotRegistry::open(&slots).unwrap());
        let root = dir.path().join("cliproxy");
        let auth = root.join("auth");
        fs::create_dir_all(&auth).unwrap();
        fs::write(auth.join("acct.json"), r#"{"type":"claude"}"#).unwrap();

        let paths = SidecarPaths {
            root: root.clone(),
            config_yaml: root.join("config.yaml"),
            auth_dir: auth,
            settings_json: root.join("settings.json"),
            log_dir: root.join("logs"),
            binary: root.join("no-such-binary.exe"),
        };
        // Start with enabled=false on disk.
        let mut settings = CliproxySettings::generate_fresh();
        settings.enabled = false;
        paths.save_settings(&settings).unwrap();

        let mgr = Arc::new(SidecarManager::new(paths.clone(), settings, reg));
        // ensure_running will fail (no binary) but enabled must still flip true.
        let _ = mgr.maybe_autostart().await;
        let disk: CliproxySettings =
            serde_json::from_str(&fs::read_to_string(&paths.settings_json).unwrap()).unwrap();
        assert!(
            disk.enabled,
            "expected enabled=true after auth-based autostart"
        );
        assert!(mgr.settings.read().await.enabled);
    }

    #[tokio::test]
    async fn stop_does_not_clear_enabled_flag() {
        let dir = tempdir().unwrap();
        let slots = dir.path().join("slots.json");
        fs::write(
            &slots,
            r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://x/v1","model":"m"}}}"#,
        )
        .unwrap();
        let reg = Arc::new(SlotRegistry::open(&slots).unwrap());
        let root = dir.path().join("cliproxy");
        fs::create_dir_all(root.join("auth")).unwrap();
        let paths = SidecarPaths {
            root: root.clone(),
            config_yaml: root.join("config.yaml"),
            auth_dir: root.join("auth"),
            settings_json: root.join("settings.json"),
            log_dir: root.join("logs"),
            binary: root.join("missing.exe"),
        };
        let mut settings = CliproxySettings::generate_fresh();
        settings.enabled = true;
        paths.save_settings(&settings).unwrap();
        let mgr = Arc::new(SidecarManager::new(paths.clone(), settings, reg));
        mgr.stop().await.unwrap();
        let disk: CliproxySettings =
            serde_json::from_str(&fs::read_to_string(&paths.settings_json).unwrap()).unwrap();
        assert!(disk.enabled, "stop() must not write enabled=false");
    }

    #[tokio::test]
    async fn lifecycle_spawn_health_shutdown_with_stub() {
        // Requires python on PATH.
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("skip: python not available");
            return;
        }
        let dir = tempdir().unwrap();
        let slots = dir.path().join("slots.json");
        fs::write(
            &slots,
            r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://x/v1","model":"m"}}}"#,
        )
        .unwrap();
        let reg = Arc::new(SlotRegistry::open(&slots).unwrap());
        let stub = write_stub_sidecar(dir.path());
        let port = 18319u16;
        let paths = SidecarPaths {
            root: dir.path().join("cliproxy"),
            config_yaml: dir.path().join("cliproxy/config.yaml"),
            auth_dir: dir.path().join("cliproxy/auth"),
            settings_json: dir.path().join("cliproxy/settings.json"),
            log_dir: dir.path().join("cliproxy/logs"),
            // Use python as binary with script args — manager expects a single binary.
            // So we wrap: create a .cmd that runs python.
            binary: {
                let cmd_path = dir.path().join("stub.cmd");
                fs::write(
                    &cmd_path,
                    format!(
                        "@echo off\r\npython \"{}\" {port}\r\n",
                        stub.display()
                    ),
                )
                .unwrap();
                cmd_path
            },
        };
        let mut settings = CliproxySettings::generate_fresh();
        settings.enabled = true;
        settings.port = port;
        paths.write_proxy_config(&settings).unwrap();

        let mgr = Arc::new(SidecarManager::new(paths, settings, reg));
        // Manual spawn via Command with python for reliability in test:
        let mut child = Command::new("python")
            .arg(&stub)
            .arg(port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        *mgr.child.lock().unwrap() = Some(child);

        // Wait for health
        let mut ok = false;
        for _ in 0..30 {
            if mgr.health_check().await.is_ok() {
                ok = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(ok, "stub health failed");
        mgr.stop().await.unwrap();
        let st = mgr.status_snapshot().await;
        assert!(!st.running);
    }
}
