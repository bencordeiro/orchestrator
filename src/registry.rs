//! Slot registry with call-time resolution and hot-reload.
//!
//! **Critical invariant:** the slot → backend mapping is never cached across
//! `delegate` calls. Every resolve re-reads `slots.json` (via mtime check) so
//! a config edit takes effect on the very next call with no restart.
//!
//! GUI mutations **must** call [`SlotRegistry::force_reload`] after writing
//! the config (via [`SlotRegistry::mutate`]) — do not rely on mtime alone.

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use crate::config::{
    write_slots_file, BackendProfile, LoadedConfig, SlotConfig, SlotsFile,
};
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
    ///
    /// Required after any GUI (or programmatic) write so the next `resolve`
    /// sees the change immediately even when filesystem mtime is coarse.
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

    /// Load config from disk (forced), apply `f`, write, then [`force_reload`].
    ///
    /// This is the only supported mutation path for the GUI.
    pub fn mutate<F>(&self, f: F) -> Result<()>
    where
        F: FnOnce(&mut SlotsFile) -> Result<()>,
    {
        let path = self.path();
        // Always start from disk as source of truth.
        let mut loaded = LoadedConfig::load(&path)?;
        f(&mut loaded.file)?;
        write_slots_file(&path, &loaded.file)?;
        // Critical: do not rely on mtime — force_reload immediately.
        self.force_reload()?;
        Ok(())
    }

    /// Replace the entire slots map / file contents via mutate + force_reload.
    pub fn replace_file(&self, file: SlotsFile) -> Result<()> {
        let path = self.path();
        write_slots_file(&path, &file)?;
        self.force_reload()?;
        Ok(())
    }

    pub fn upsert_slot(&self, name: &str, config: SlotConfig) -> Result<()> {
        if name.trim().is_empty() {
            return Err(OrchestratorError::Config("slot name must not be empty".into()));
        }
        let name = name.to_string();
        self.mutate(|file| {
            file.slots.insert(name, config);
            Ok(())
        })
    }

    pub fn remove_slot(&self, name: &str) -> Result<()> {
        self.mutate(|file| {
            if file.slots.remove(name).is_none() {
                return Err(OrchestratorError::UnknownSlot(name.to_string()));
            }
            Ok(())
        })
    }

    /// Assign a named backend profile to a slot (swap).
    pub fn assign_backend(&self, slot_name: &str, profile_id: &str) -> Result<()> {
        self.mutate(|file| {
            let profile = file
                .backend_profiles
                .get(profile_id)
                .cloned()
                .ok_or_else(|| {
                    OrchestratorError::Config(format!("unknown backend profile '{profile_id}'"))
                })?;
            let slot = file.slots.get_mut(slot_name).ok_or_else(|| {
                OrchestratorError::UnknownSlot(slot_name.to_string())
            })?;
            profile.apply_to_slot(slot);
            Ok(())
        })
    }

    pub fn upsert_backend_profile(&self, id: &str, profile: BackendProfile) -> Result<()> {
        if id.trim().is_empty() {
            return Err(OrchestratorError::Config(
                "backend profile id must not be empty".into(),
            ));
        }
        let id = id.to_string();
        self.mutate(|file| {
            file.backend_profiles.insert(id, profile);
            Ok(())
        })
    }

    pub fn remove_backend_profile(&self, id: &str) -> Result<()> {
        self.mutate(|file| {
            if file.backend_profiles.remove(id).is_none() {
                return Err(OrchestratorError::Config(format!(
                    "unknown backend profile '{id}'"
                )));
            }
            Ok(())
        })
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

    #[test]
    fn mutate_force_reload_takes_effect_immediately_without_mtime_wait() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        write_slots(&path, "model-a", "http://backend-a/v1");

        let registry = SlotRegistry::open(&path).unwrap();
        assert_eq!(registry.resolve("worker").unwrap().model, "model-a");

        // No sleep — GUI path must not depend on mtime granularity.
        registry
            .mutate(|file| {
                let slot = file.slots.get_mut("worker").unwrap();
                slot.model = "model-b-immediate".into();
                slot.base_url = "http://backend-b/v1".into();
                Ok(())
            })
            .unwrap();

        let after = registry.resolve("worker").unwrap();
        assert_eq!(after.model, "model-b-immediate");
        assert_eq!(after.base_url, "http://backend-b/v1");
    }

    #[test]
    fn assign_backend_profile_swaps_slot() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        fs::write(
            &path,
            r#"{
              "backend_profiles": {
                "alpha": {
                  "label": "Alpha",
                  "backend": "openai_compatible",
                  "base_url": "http://a/v1",
                  "model": "m-a"
                },
                "beta": {
                  "label": "Beta",
                  "backend": "openai_compatible",
                  "base_url": "http://b/v1",
                  "model": "m-b"
                }
              },
              "slots": {
                "worker": {
                  "description": "General worker",
                  "backend": "openai_compatible",
                  "base_url": "http://a/v1",
                  "model": "m-a"
                }
              }
            }"#,
        )
        .unwrap();

        let registry = SlotRegistry::open(&path).unwrap();
        registry.assign_backend("worker", "beta").unwrap();
        let slot = registry.resolve("worker").unwrap();
        assert_eq!(slot.model, "m-b");
        assert_eq!(slot.base_url, "http://b/v1");
        // Description preserved.
        assert_eq!(slot.description, "General worker");
    }
}
