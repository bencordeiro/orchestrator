//! CLIProxyAPI sidecar integration (subscription OAuth).
//!
//! Lives entirely in the Tauri host — the `orchestrator` core crate never
//! learns about CLIProxyAPI. Subscription backends are registered as normal
//! `openai_compatible` profiles pointing at the local proxy URL.

mod client;
mod config;
pub mod manager;
mod profiles;

pub use client::{classify_proxy_error, AuthAccount, CliProxyClient};
pub use config::{CliproxySettings, SidecarPaths, PINNED_VERSION};
pub use manager::{
    SidecarManager, SidecarPresence, SidecarStatus, OAUTH_PROVIDERS, PROXY_KEY_REF,
};
pub use profiles::{profile_id_for_account, sync_subscription_profiles, DEFAULT_MODELS};
