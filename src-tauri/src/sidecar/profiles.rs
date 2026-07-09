//! Register CLIProxyAPI-backed backends as generic openai_compatible profiles.
//!
//! The core crate only sees ordinary profiles — zero special-casing.

use anyhow::Result;
use orchestrator::config::{BackendKind, BackendProfile};
use orchestrator::SlotRegistry;

use super::client::AuthAccount;

/// Default model ids per subscription provider when `/v1/models` is empty.
pub const DEFAULT_MODELS: &[(&str, &str)] = &[
    ("claude", "claude-sonnet-4-5"),
    ("anthropic", "claude-sonnet-4-5"),
    ("codex", "gpt-5.1"),
    ("openai", "gpt-5.1"),
    ("gemini", "gemini-2.5-pro"),
    ("antigravity", "gemini-2.5-pro"),
    ("xai", "grok-4"),
    ("grok", "grok-4"),
    ("kimi", "kimi-k2"),
];

pub fn default_model_for(provider: &str) -> &'static str {
    let p = provider.to_lowercase();
    DEFAULT_MODELS
        .iter()
        .find(|(k, _)| *k == p)
        .map(|(_, m)| *m)
        .unwrap_or("default")
}

pub fn profile_id_for_account(account: &AuthAccount) -> String {
    let email = account
        .email
        .as_deref()
        .or(account.label.as_deref())
        .unwrap_or("account");
    let safe: String = email
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("sub-{}-{}", account.provider.to_lowercase(), safe)
}

/// Upsert openai_compatible profiles for each connected subscription account.
///
/// Removes stale `sub-*` profiles that no longer match any auth file.
pub fn sync_subscription_profiles(
    registry: &SlotRegistry,
    accounts: &[AuthAccount],
    openai_base_url: &str,
    proxy_api_key_ref: &str,
    models: &[String],
) -> Result<Vec<String>> {
    // Ensure proxy API key is referenced by auth_ref name stored in keychain by caller.
    let mut active_ids = Vec::new();

    for account in accounts {
        if account.disabled {
            continue;
        }
        let id = profile_id_for_account(account);
        let model = pick_model_for_provider(&account.provider, models);
        let label = format!(
            "Subscription · {} · {}",
            account.provider,
            account
                .email
                .as_deref()
                .or(account.label.as_deref())
                .unwrap_or(&account.name)
        );
        let profile = BackendProfile {
            label,
            backend: BackendKind::OpenaiCompatible,
            base_url: openai_base_url.to_string(),
            model: model.to_string(),
            auth_ref: Some(proxy_api_key_ref.to_string()),
        };
        registry.upsert_backend_profile(&id, profile)?;
        active_ids.push(id);
    }

    // Drop orphaned subscription profiles.
    let cfg = registry.current()?;
    let stale: Vec<String> = cfg
        .file
        .backend_profiles
        .keys()
        .filter(|k| k.starts_with("sub-") && !active_ids.contains(k))
        .cloned()
        .collect();
    for id in stale {
        let _ = registry.remove_backend_profile(&id);
    }

    Ok(active_ids)
}

fn pick_model_for_provider(provider: &str, models: &[String]) -> String {
    let p = provider.to_lowercase();
    // Prefer a model from the live list that matches provider heuristics.
    for m in models {
        let ml = m.to_lowercase();
        if p.contains("claude") || p == "anthropic" {
            if ml.contains("claude") {
                return m.clone();
            }
        } else if p.contains("codex") || p == "openai" {
            if ml.contains("gpt") || ml.contains("codex") || ml.contains("o1") || ml.contains("o3")
            {
                return m.clone();
            }
        } else if p.contains("gemini") || p.contains("antigravity") {
            if ml.contains("gemini") {
                return m.clone();
            }
        } else if p.contains("xai") || p.contains("grok") {
            if ml.contains("grok") {
                return m.clone();
            }
        } else if p.contains("kimi") {
            if ml.contains("kimi") {
                return m.clone();
            }
        }
    }
    if let Some(first) = models.first() {
        return first.clone();
    }
    default_model_for(provider).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orchestrator::config::SlotConfig;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn sync_registers_and_prunes_sub_profiles() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("slots.json");
        fs::write(
            &path,
            r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://x/v1","model":"m"}},"backend_profiles":{}}"#,
        )
        .unwrap();
        let reg = SlotRegistry::open(&path).unwrap();

        let accounts = vec![AuthAccount {
            id: "claude-a@b.com.json".into(),
            name: "claude-a@b.com.json".into(),
            provider: "claude".into(),
            email: Some("a@b.com".into()),
            label: None,
            status: "active".into(),
            status_message: String::new(),
            unavailable: false,
            disabled: false,
        }];

        let ids = sync_subscription_profiles(
            &reg,
            &accounts,
            "http://127.0.0.1:18317/v1",
            "cliproxy_proxy_key",
            &[],
        )
        .unwrap();
        assert_eq!(ids.len(), 1);
        assert!(ids[0].starts_with("sub-claude-"));

        let cfg = reg.current().unwrap();
        let p = cfg.file.backend_profiles.get(&ids[0]).unwrap();
        assert_eq!(p.backend, BackendKind::OpenaiCompatible);
        assert_eq!(p.base_url, "http://127.0.0.1:18317/v1");
        assert_eq!(p.model, "claude-sonnet-4-5");
        assert_eq!(p.auth_ref.as_deref(), Some("cliproxy_proxy_key"));

        // Prune when accounts empty.
        let ids2 = sync_subscription_profiles(
            &reg,
            &[],
            "http://127.0.0.1:18317/v1",
            "cliproxy_proxy_key",
            &[],
        )
        .unwrap();
        assert!(ids2.is_empty());
        let cfg2 = reg.current().unwrap();
        assert!(!cfg2.file.backend_profiles.contains_key(&ids[0]));

        // Keep non-sub profiles intact.
        reg.upsert_backend_profile(
            "local-qwen",
            BackendProfile {
                label: "Local".into(),
                backend: BackendKind::OpenaiCompatible,
                base_url: "http://localhost:11434/v1".into(),
                model: "llama3.2".into(),
                auth_ref: None,
            },
        )
        .unwrap();
        let _ = SlotConfig {
            description: "x".into(),
            backend: BackendKind::OpenaiCompatible,
            base_url: "u".into(),
            model: "m".into(),
            auth_ref: None,
            fallback: None,
            enable_fallback: false,
        };
        assert!(reg.current().unwrap().file.backend_profiles.contains_key("local-qwen"));
    }
}
