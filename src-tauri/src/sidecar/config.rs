//! Paths, pinned version, and config.yaml generation for CLIProxyAPI.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Pinned CLIProxyAPI release (see `src-tauri/binaries/VERSION.txt`).
pub const PINNED_VERSION: &str = "7.2.58";

/// Default listen port for our managed sidecar (avoids clashing with a
/// user-installed CLIProxyAPI on 8317).
pub const DEFAULT_PORT: u16 = 18317;

/// Settings persisted under the app config dir.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CliproxySettings {
    /// User opted in to running the sidecar (or auto-enabled after first connect).
    /// When auth credentials exist, autostart forces this true.
    #[serde(default)]
    pub enabled: bool,
    /// Local OpenAI-compatible listen port.
    #[serde(default = "default_port")]
    pub port: u16,
    /// Management API key (plaintext in our settings file under the user config dir;
    /// also written into CLIProxyAPI config.yaml for the sidecar process).
    pub management_key: String,
    /// Proxy API key clients use when calling /v1/* on the sidecar.
    pub proxy_api_key: String,
    /// Per-account model overrides (keyed by auth file name / account id).
    /// Used by `sync_subscription_profiles` instead of the provider heuristic.
    #[serde(default)]
    pub model_overrides: std::collections::HashMap<String, String>,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

impl CliproxySettings {
    pub fn generate_fresh() -> Self {
        Self {
            enabled: false,
            port: DEFAULT_PORT,
            management_key: format!("mgmt-{}", Uuid::new_v4()),
            proxy_api_key: format!("proxy-{}", Uuid::new_v4()),
            model_overrides: std::collections::HashMap::new(),
        }
    }
}

/// Resolved filesystem layout for the sidecar (isolated under app config).
#[derive(Debug, Clone)]
pub struct SidecarPaths {
    /// e.g. `%AppData%/orchestrator/cliproxy`
    pub root: PathBuf,
    pub config_yaml: PathBuf,
    pub auth_dir: PathBuf,
    pub settings_json: PathBuf,
    pub log_dir: PathBuf,
    pub binary: PathBuf,
}

impl SidecarPaths {
    /// Layout under `<config_parent>/cliproxy` where config_parent is the
    /// directory containing `slots.json` (or the app config dir).
    pub fn resolve(slots_config_path: &Path) -> Result<Self> {
        let parent = slots_config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        // Prefer a dedicated app data dir so we never collide with ~/.cli-proxy-api.
        let root = if let Some(cfg) = dirs::config_dir() {
            cfg.join("orchestrator").join("cliproxy")
        } else {
            parent.join("cliproxy")
        };
        let binary = resolve_binary_path()?;
        Ok(Self {
            config_yaml: root.join("config.yaml"),
            auth_dir: root.join("auth"),
            settings_json: root.join("settings.json"),
            log_dir: root.join("logs"),
            binary,
            root,
        })
    }

    pub fn ensure_dirs(&self) -> Result<()> {
        fs::create_dir_all(&self.root)?;
        fs::create_dir_all(&self.auth_dir)?;
        fs::create_dir_all(&self.log_dir)?;
        Ok(())
    }

    pub fn load_or_init_settings(&self) -> Result<CliproxySettings> {
        self.ensure_dirs()?;
        if self.settings_json.exists() {
            let raw = fs::read_to_string(&self.settings_json)?;
            match serde_json::from_str::<CliproxySettings>(&raw) {
                Ok(s) => Ok(s),
                Err(e) => {
                    // Never overwrite a broken file with generate_fresh() (that would
                    // rotate management/proxy keys and clear enabled). Keep a backup.
                    tracing::error!(
                        "failed to parse {}: {e}; leaving file in place",
                        self.settings_json.display()
                    );
                    Err(e).with_context(|| format!("parse {}", self.settings_json.display()))
                }
            }
        } else {
            let s = CliproxySettings::generate_fresh();
            self.save_settings(&s)?;
            Ok(s)
        }
    }

    /// True if the auth dir has at least one `*.json` credential file.
    pub fn has_auth_credentials(&self) -> bool {
        let Ok(rd) = fs::read_dir(&self.auth_dir) else {
            return false;
        };
        rd.filter_map(|e| e.ok()).any(|e| {
            let p = e.path();
            p.is_file()
                && p.extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x.eq_ignore_ascii_case("json"))
                    .unwrap_or(false)
        })
    }

    pub fn save_settings(&self, s: &CliproxySettings) -> Result<()> {
        self.ensure_dirs()?;
        let raw = serde_json::to_string_pretty(s)?;
        fs::write(&self.settings_json, raw)?;
        Ok(())
    }

    /// Write CLIProxyAPI `config.yaml` (host locked to localhost, isolated auth-dir).
    pub fn write_proxy_config(&self, settings: &CliproxySettings) -> Result<()> {
        self.ensure_dirs()?;
        // Use forward slashes in YAML for portability on Windows too.
        let auth = self.auth_dir.to_string_lossy().replace('\\', "/");
        let yaml = format!(
            r#"# Generated by Orchestrator — do not point at the system ~/.cli-proxy-api
# CLIProxyAPI pin: {version}
host: "127.0.0.1"
port: {port}

remote-management:
  allow-remote: false
  secret-key: "{mgmt}"
  disable-control-panel: true

auth-dir: "{auth}"

api-keys:
  - "{proxy_key}"

debug: false
"#,
            version = PINNED_VERSION,
            port = settings.port,
            mgmt = settings.management_key,
            auth = auth,
            proxy_key = settings.proxy_api_key,
        );
        fs::write(&self.config_yaml, yaml)?;
        Ok(())
    }

    pub fn base_url(&self, settings: &CliproxySettings) -> String {
        format!("http://127.0.0.1:{}", settings.port)
    }

    pub fn openai_base_url(&self, settings: &CliproxySettings) -> String {
        format!("{}/v1", self.base_url(settings))
    }
}

/// Plain sidecar filename on the host platform.
///
/// This is the name Tauri leaves next to the app executable once it strips the
/// target-triple suffix at install time.
///
/// Deliberately namespaced rather than the upstream `cli-proxy-api`: the Debian
/// package installs `externalBin` into `/usr/bin`, and a generic name there
/// would collide with (or silently shadow) a user's own CLIProxyAPI install.
/// The name *inside* the upstream release archive is still `cli-proxy-api` —
/// the download scripts rename it while staging.
#[cfg(windows)]
pub const SIDECAR_BIN: &str = "orchestrator-cli-proxy-api.exe";
#[cfg(not(windows))]
pub const SIDECAR_BIN: &str = "orchestrator-cli-proxy-api";

/// The `externalBin` filename Tauri stages at bundle time: `<name>-<triple>`
/// (plus `.exe` on Windows). The triple comes from `build.rs`, so this stays
/// correct when cross-compiling or building for aarch64.
pub fn sidecar_bin_triple() -> String {
    let triple = env!("ORCHESTRATOR_TARGET_TRIPLE");
    if cfg!(windows) {
        format!("orchestrator-cli-proxy-api-{triple}.exe")
    } else {
        format!("orchestrator-cli-proxy-api-{triple}")
    }
}

/// Locate the sidecar binary. Override with `ORCHESTRATOR_CLIPROXY_BIN`.
///
/// Production (Tauri `externalBin`): binary sits next to the app executable
/// under [`SIDECAR_BIN`] after the target-triple suffix is stripped at install
/// time. Dev layouts additionally check the repo `binaries/` directories, where
/// `scripts/download-cliproxy.{sh,ps1}` stage both names.
pub fn resolve_binary_path() -> Result<PathBuf> {
    if let Ok(p) = std::env::var("ORCHESTRATOR_CLIPROXY_BIN") {
        return Ok(PathBuf::from(p));
    }

    let triple = sidecar_bin_triple();
    let mut candidates: Vec<PathBuf> = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Installed / release layout (externalBin)
            candidates.push(dir.join(SIDECAR_BIN));
            candidates.push(dir.join(&triple));
            candidates.push(dir.join("binaries").join(SIDECAR_BIN));
            candidates.push(dir.join("binaries").join(&triple));
        }
    }

    // Dev layouts
    candidates.push(PathBuf::from("src-tauri/binaries").join(SIDECAR_BIN));
    candidates.push(PathBuf::from("src-tauri/binaries").join(&triple));
    candidates.push(PathBuf::from("binaries").join(SIDECAR_BIN));
    candidates.push(PathBuf::from("binaries").join(&triple));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(SIDECAR_BIN));
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(&triple));

    for c in candidates {
        if !c.as_os_str().is_empty() && c.exists() {
            return Ok(c.canonicalize().unwrap_or(c));
        }
    }
    // Expected path for error messages when missing.
    Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries").join(SIDECAR_BIN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn writes_config_and_settings() {
        let dir = tempdir().unwrap();
        let slots = dir.path().join("slots.json");
        fs::write(&slots, "{}").unwrap();
        // Force root under temp by using env-less path construction:
        let paths = SidecarPaths {
            root: dir.path().join("cliproxy"),
            config_yaml: dir.path().join("cliproxy/config.yaml"),
            auth_dir: dir.path().join("cliproxy/auth"),
            settings_json: dir.path().join("cliproxy/settings.json"),
            log_dir: dir.path().join("cliproxy/logs"),
            binary: PathBuf::from("stub"),
        };
        let s = CliproxySettings::generate_fresh();
        paths.write_proxy_config(&s).unwrap();
        paths.save_settings(&s).unwrap();
        assert!(paths.config_yaml.exists());
        let raw = fs::read_to_string(&paths.config_yaml).unwrap();
        assert!(raw.contains("127.0.0.1"));
        assert!(raw.contains(&format!("port: {}", s.port)));
        assert!(raw.contains(&s.management_key));
    }
}
