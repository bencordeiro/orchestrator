//! Orchestrator headless core: slot-based model delegation over MCP.
//!
//! Library surface for the thin `orchestrator` binary and (later) the Tauri GUI.

pub mod backends;
pub mod config;
pub mod conversation;
pub mod core;
pub mod error;
pub mod mcp;
pub mod registry;
pub mod secrets;

pub use config::{BackendKind, LoadedConfig, SlotConfig, SlotsFile};
pub use conversation::ConversationStore;
pub use core::{DelegateRequest, DelegateResult, Orchestrator};
pub use error::{OrchestratorError, Result};
pub use registry::{PublicSlot, SlotRegistry};
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretStore, KEYRING_SERVICE};
