//! Orchestrator headless core: slot-based model delegation over MCP.
//!
//! Library surface for the thin `orchestrator` binary and the Tauri GUI.

pub mod backends;
pub mod config;
pub mod conversation;
pub mod core;
pub mod error;
pub mod mcp;
pub mod notify_debounce;
pub mod ollama;
pub mod registry;
pub mod secrets;
pub mod status;
pub mod usage;

pub use config::{
    write_example_if_missing, write_slots_file, BackendKind, BackendProfile, LoadedConfig,
    SlotConfig, SlotsFile,
};
pub use conversation::ConversationStore;
pub use core::{DelegateRequest, DelegateResult, Orchestrator, SlotBoardItem, WorkerUnavailableHook};
pub use error::{OrchestratorError, Result};
pub use notify_debounce::{NotifyDebouncer, DEFAULT_WINDOW as NOTIFY_DEFAULT_WINDOW};
pub use ollama::{discover_models, list_models_on_host, OllamaModel, DEFAULT_OLLAMA_HOST};
pub use registry::{PublicSlot, SlotRegistry};
pub use secrets::{KeyringSecretStore, MemorySecretStore, SecretStore, KEYRING_SERVICE};
pub use status::{SlotStatus, StatusBoard};
pub use usage::{UsageEvent, UsageLog, DEFAULT_MAX_BYTES as USAGE_DEFAULT_MAX_BYTES};
