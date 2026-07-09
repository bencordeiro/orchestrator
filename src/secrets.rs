//! Secret resolution via OS keychain.
//!
//! Config files only store opaque reference names (`auth_ref`). Actual API keys
//! and the MCP bearer token live in the keychain under a fixed service name.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use keyring::Entry;

use crate::error::{OrchestratorError, Result};

/// Keyring service identifier for all orchestrator secrets.
pub const KEYRING_SERVICE: &str = "orchestrator-mcp";

/// Abstraction over secret storage so tests can inject an in-memory map.
pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<String>>;
    fn set(&self, name: &str, value: &str) -> Result<()>;
    fn delete(&self, name: &str) -> Result<()>;
}

/// Production store backed by the OS keychain (`keyring` crate).
#[derive(Debug, Default, Clone)]
pub struct KeyringSecretStore;

impl SecretStore for KeyringSecretStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        let entry = Entry::new(KEYRING_SERVICE, name).map_err(|e| {
            OrchestratorError::Secret(format!("keyring entry '{name}': {e}"))
        })?;
        match entry.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(OrchestratorError::Secret(format!(
                "keyring get '{name}': {e}"
            ))),
        }
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, name).map_err(|e| {
            OrchestratorError::Secret(format!("keyring entry '{name}': {e}"))
        })?;
        entry
            .set_password(value)
            .map_err(|e| OrchestratorError::Secret(format!("keyring set '{name}': {e}")))
    }

    fn delete(&self, name: &str) -> Result<()> {
        let entry = Entry::new(KEYRING_SERVICE, name).map_err(|e| {
            OrchestratorError::Secret(format!("keyring entry '{name}': {e}"))
        })?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(OrchestratorError::Secret(format!(
                "keyring delete '{name}': {e}"
            ))),
        }
    }
}

/// In-memory secret store for unit/integration tests.
#[derive(Debug, Default, Clone)]
pub struct MemorySecretStore {
    inner: Arc<Mutex<HashMap<String, String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_secrets(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let store = Self::new();
        {
            let mut guard = store.inner.lock().unwrap();
            for (k, v) in pairs {
                guard.insert(k, v);
            }
        }
        store
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> Result<Option<String>> {
        Ok(self.inner.lock().unwrap().get(name).cloned())
    }

    fn set(&self, name: &str, value: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .insert(name.to_string(), value.to_string());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<()> {
        self.inner.lock().unwrap().remove(name);
        Ok(())
    }
}

/// Resolve an optional auth_ref to a bearer/API key string.
pub fn resolve_auth(
    store: &dyn SecretStore,
    auth_ref: Option<&str>,
) -> Result<Option<String>> {
    match auth_ref {
        None => Ok(None),
        Some(name) => {
            let value = store.get(name)?;
            if value.is_none() {
                return Err(OrchestratorError::Secret(format!(
                    "auth_ref '{name}' not found in keychain — set it with `orchestrator secrets set {name} <value>`"
                )));
            }
            Ok(value)
        }
    }
}
