//! Orchestrator headless core: slot-based model delegation over MCP.
//!
//! Library surface for the thin `orchestrator` binary and the Tauri GUI.

pub mod backends;
pub mod config;
pub mod conversation;
pub mod core;
pub mod error;
pub mod mcp;
pub mod registry;
pub mod secrets;
pub mod status;

pub use config::{
    write_example_if_missing, write_slots_file, BackendKind, BackendProfile, LoadedConfig,
    SlotConfig, SlotsFile,
};
pub use conversation::ConversationStore;
pub use core::{DelegateRequest, DelegateResult, Orchestrator, SlotBoardItem};
pub use error::{OrchestratorError, Result};
pub use registry::{PublicSlot, SlotRegistry};
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretStore, KEYRING_SERVICE};
pub use status::{SlotStatus, StatusBoard};
