//! `slots.json` schema and loading.
//!
//! Secrets are never stored here — only opaque `auth_ref` names that resolve
//! through the OS keychain (or a test secret store).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::error::{OrchestratorError, Result};

/// Top-level configuration file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotsFile {
    /// Bind address for the MCP HTTP server (e.g. `127.0.0.1:7420`).
    #[serde(default = "default_listen")]
    pub listen: String,

    /// Keyring reference for the MCP bearer token that protects the server.
    #[serde(default = "default_bearer_ref")]
    pub bearer_token_ref: String,

    /// Directory for persisted conversation histories (relative paths resolve
    /// against the config file's parent directory).
    #[serde(default = "default_conversations_dir")]
    pub conversations_dir: String,

    /// Named slots. Key is the slot name (`worker`, `reviewer`, …).
    pub slots: HashMap<String, SlotConfig>,
}

fn default_listen() -> String {
    "127.0.0.1:7420".to_string()
}

fn default_bearer_ref() -> String {
    "mcp_bearer_token".to_string()
}

fn default_conversations_dir() -> String {
    "data/conversations".to_string()
}

/// One slot: a stable name that points at a swappable backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlotConfig {
    /// Human-readable capability description returned by `list_slots`.
    /// Must never mention vendor or model names.
    pub description: String,

    /// Backend adapter kind.
    pub backend: BackendKind,

    /// Base URL for the provider API (e.g. `http://10.0.0.10:8000/v1`).
    pub base_url: String,

    /// Model id sent to the backend.
    pub model: String,

    /// Keyring reference for the API key / token. Optional for local backends
    /// that need no auth.
    #[serde(default)]
    pub auth_ref: Option<String>,

    /// Optional ordered fallback slot names. Present in the schema but
    /// **off by default** — only used when `enable_fallback` is true.
    #[serde(default)]
    pub fallback: Option<Vec<String>>,

    /// When false (default), fallback chains are ignored.
    #[serde(default)]
    pub enable_fallback: bool,
}

/// Which adapter to use for a slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendKind {
    /// Any OpenAI-compatible chat-completions endpoint (CLIProxyAPI, Ollama, …).
    OpenaiCompatible,
    /// Native Anthropic Messages API.
    Anthropic,
}

/// Snapshot of a loaded config plus metadata for hot-reload.
#[derive(Debug, Clone)]
pub struct LoadedConfig {
    pub file: SlotsFile,
    pub path: PathBuf,
    pub modified: Option<SystemTime>,
}

impl LoadedConfig {
    /// Load config from disk.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let raw = fs::read_to_string(&path).map_err(|e| {
            OrchestratorError::Config(format!("failed to read {}: {e}", path.display()))
        })?;
        let file: SlotsFile = serde_json::from_str(&raw).map_err(|e| {
            OrchestratorError::Config(format!("invalid slots.json at {}: {e}", path.display()))
        })?;
        let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
        Ok(Self {
            file,
            path,
            modified,
        })
    }

    /// Re-load from disk if the file's mtime advanced (or always if mtime unknown).
    pub fn reload_if_changed(&self) -> Result<Self> {
        let current_mtime = fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        let needs_reload = match (self.modified, current_mtime) {
            (Some(prev), Some(now)) => now > prev,
            _ => true,
        };
        if needs_reload {
            Self::load(&self.path)
        } else {
            Ok(self.clone())
        }
    }

    /// Absolute path for conversation storage.
    pub fn conversations_path(&self) -> PathBuf {
        let p = PathBuf::from(&self.file.conversations_dir);
        if p.is_absolute() {
            p
        } else if let Some(parent) = self.path.parent() {
            parent.join(p)
        } else {
            p
        }
    }
}

/// Write a default example config next to `path` if it does not exist.
pub fn write_example_if_missing(path: &Path) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let example = r#"{
  "listen": "127.0.0.1:7420",
  "bearer_token_ref": "mcp_bearer_token",
  "conversations_dir": "data/conversations",
  "slots": {
    "worker": {
      "description": "General-purpose coding and reasoning worker",
      "backend": "openai_compatible",
      "base_url": "http://10.0.0.10:8000/v1",
      "model": "qwen35b",
      "auth_ref": "worker_api_key",
      "fallback": [],
      "enable_fallback": false
    }
  }
}
"#;
    fs::write(path, example)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn parses_minimal_slots_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        let mut f = fs::File::create(&path).unwrap();
        write!(
            f,
            r#"{{
              "slots": {{
                "worker": {{
                  "description": "does work",
                  "backend": "openai_compatible",
                  "base_url": "http://localhost:11434/v1",
                  "model": "llama3"
                }}
              }}
            }}"#
        )
        .unwrap();

        let loaded = LoadedConfig::load(&path).unwrap();
        assert_eq!(loaded.file.listen, "127.0.0.1:7420");
        let slot = loaded.file.slots.get("worker").unwrap();
        assert_eq!(slot.backend, BackendKind::OpenaiCompatible);
        assert!(!slot.enable_fallback);
        assert!(slot.auth_ref.is_none());
    }

    #[test]
    fn reload_if_changed_picks_up_edits() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        fs::write(
            &path,
            r#"{
              "slots": {
                "worker": {
                  "description": "v1",
                  "backend": "openai_compatible",
                  "base_url": "http://a.example/v1",
                  "model": "model-a"
                }
              }
            }"#,
        )
        .unwrap();

        let loaded = LoadedConfig::load(&path).unwrap();
        assert_eq!(
            loaded.file.slots["worker"].model,
            "model-a"
        );

        // Ensure mtime advances on Windows (1s resolution on some FS).
        std::thread::sleep(std::time::Duration::from_millis(1100));

        fs::write(
            &path,
            r#"{
              "slots": {
                "worker": {
                  "description": "v2",
                  "backend": "openai_compatible",
                  "base_url": "http://b.example/v1",
                  "model": "model-b"
                }
              }
            }"#,
        )
        .unwrap();

        let reloaded = loaded.reload_if_changed().unwrap();
        assert_eq!(reloaded.file.slots["worker"].model, "model-b");
        assert_eq!(
            reloaded.file.slots["worker"].base_url,
            "http://b.example/v1"
        );
    }
}
