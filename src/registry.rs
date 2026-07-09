//! Slot registry with call-time resolution and hot-reload.
//!
//! **Critical invariant:** the slot → backend mapping is never cached across
//! `delegate` calls. Every resolve re-reads `slots.json` (via mtime check) so
//! a config edit takes effect on the very next call with no restart.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::{LoadedConfig, SlotConfig};
use crate::error::{OrchestratorError, Result};

/// Hot-reloadable slot registry.
///
/// Internally keeps a `LoadedConfig` snapshot, but `resolve` always checks
/// whether the on-disk file changed and reloads before returning a slot.
#[derive(Debug)]
pub struct SlotRegistry {
    inner: RwLock<LoadedConfig>,
}

impl SlotRegistry {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let loaded = LoadedConfig::load(path)?;
        Ok(Self {
            inner: RwLock::new(loaded),
        })
    }

    pub fn path(&self) -> PathBuf {
        self.inner.read().unwrap().path.clone()
    }

    /// Force a reload from disk (ignores mtime).
    pub fn force_reload(&self) -> Result<()> {
        let path = self.path();
        let loaded = LoadedConfig::load(path)?;
        *self.inner.write().unwrap() = loaded;
        Ok(())
    }

    /// Return a snapshot of the current config after applying any on-disk changes.
    pub fn current(&self) -> Result<LoadedConfig> {
        // First pass: check mtime under read lock.
        let maybe_new = {
            let guard = self.inner.read().unwrap();
            guard.reload_if_changed()?
        };
        // If path/mtime produced a new snapshot, publish it.
        {
            let mut guard = self.inner.write().unwrap();
            // Re-check once under write lock to avoid clobbering a concurrent reload.
            let latest = guard.reload_if_changed()?;
            *guard = latest.clone();
            let _ = maybe_new;
            Ok(latest)
        }
    }

    /// Resolve a slot **at call time**. Always consults the latest config.
    pub fn resolve(&self, slot_name: &str) -> Result<SlotConfig> {
        let cfg = self.current()?;
        cfg.file
            .slots
            .get(slot_name)
            .cloned()
            .ok_or_else(|| OrchestratorError::UnknownSlot(slot_name.to_string()))
    }

    /// List slot names + capability descriptions only (no vendor/model leakage).
    pub fn list_public(&self) -> Result<Vec<PublicSlot>> {
        let cfg = self.current()?;
        let mut out: Vec<PublicSlot> = cfg
            .file
            .slots
            .iter()
            .map(|(name, slot)| PublicSlot {
                name: name.clone(),
                description: slot.description.clone(),
            })
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }
}

/// What `list_slots` is allowed to reveal.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PublicSlot {
    pub name: String,
    pub description: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn write_slots(path: &Path, model: &str, base: &str) {
        fs::write(
            path,
            format!(
                r#"{{
                  "slots": {{
                    "worker": {{
                      "description": "General worker",
                      "backend": "openai_compatible",
                      "base_url": "{base}",
                      "model": "{model}"
                    }}
                  }}
                }}"#
            ),
        )
        .unwrap();
    }

    #[test]
    fn resolve_picks_up_hot_swap_without_reopen() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        write_slots(&path, "model-a", "http://backend-a/v1");

        let registry = SlotRegistry::open(&path).unwrap();
        let first = registry.resolve("worker").unwrap();
        assert_eq!(first.model, "model-a");
        assert_eq!(first.base_url, "http://backend-a/v1");

        // Windows mtime granularity can be ~1s.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write_slots(&path, "model-b", "http://backend-b/v1");

        // Same registry instance — no restart — must see the new backend.
        let second = registry.resolve("worker").unwrap();
        assert_eq!(second.model, "model-b");
        assert_eq!(second.base_url, "http://backend-b/v1");
    }

    #[test]
    fn list_public_hides_vendor_and_model() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        write_slots(&path, "gpt-secret", "http://openai.internal/v1");
        let registry = SlotRegistry::open(&path).unwrap();
        let list = registry.list_public().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "worker");
        assert_eq!(list[0].description, "General worker");
        let dumped = serde_json::to_string(&list).unwrap();
        assert!(!dumped.contains("gpt-secret"));
        assert!(!dumped.contains("openai.internal"));
    }
}
