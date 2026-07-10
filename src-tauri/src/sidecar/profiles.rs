//! Register CLIProxyAPI-backed backends as generic openai_compatible profiles.
//!
//! The core crate only sees ordinary profiles — zero special-casing.

use std::collections::HashMap;

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

/// Substrings that indicate a non-text / non-chat worker model.
const DEPRIORITIZE: &[&str] = &["image", "audio", "embed", "tts", "whisper", "vision-only"];

pub fn default_model_for(provider: &str) -> &'static str {
    let p = provider.to_lowercase();
    DEFAULT_MODELS
        .iter()
        .find(|(k, _)| *k == p)
        .map(|(_, m)| *m)
        .unwrap_or("default")
}

/// True if the model id looks like image/audio/embed/tts rather than a text worker.
pub fn is_non_text_model(model_id: &str) -> bool {
    let ml = model_id.to_lowercase();
    DEPRIORITIZE.iter().any(|s| ml.contains(s))
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
/// `model_overrides` is keyed by auth file name / account id (e.g. `claude-user@x.json`).
/// When present, that model is used instead of the heuristic.
///
/// Removes stale `sub-*` profiles that no longer match any auth file.
pub fn sync_subscription_profiles(
    registry: &SlotRegistry,
    accounts: &[AuthAccount],
    openai_base_url: &str,
    proxy_api_key_ref: &str,
    models: &[String],
    model_overrides: &HashMap<String, String>,
) -> Result<Vec<String>> {
    let mut active_ids = Vec::new();

    for account in accounts {
        if account.disabled {
            continue;
        }
        let id = profile_id_for_account(account);
        let model = resolve_model_for_account(account, models, model_overrides);
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
            model,
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

/// Prefer user override for this account id/name; else heuristic pick.
pub fn resolve_model_for_account(
    account: &AuthAccount,
    models: &[String],
    overrides: &HashMap<String, String>,
) -> String {
    // Overrides keyed by auth file name / id.
    for key in [&account.id, &account.name] {
        if let Some(m) = overrides.get(key) {
            if !m.trim().is_empty() {
                return m.clone();
            }
        }
    }
    pick_model_for_provider(&account.provider, models)
}

/// Pick a text-capable model for the provider, avoiding image/audio/embed/tts ids.
pub fn pick_model_for_provider(provider: &str, models: &[String]) -> String {
    let p = provider.to_lowercase();
    let text_models: Vec<&String> = models
        .iter()
        .filter(|m| !is_non_text_model(m))
        .collect();

    // Prefer a model from the live list that matches provider heuristics.
    for m in &text_models {
        let ml = m.to_lowercase();
        if p.contains("claude") || p == "anthropic" {
            if ml.contains("claude") {
                return (*m).clone();
            }
        } else if p.contains("codex") || p == "openai" {
            if ml.contains("gpt") || ml.contains("codex") || ml.contains("o1") || ml.contains("o3")
            {
                return (*m).clone();
            }
        } else if p.contains("gemini") || p.contains("antigravity") {
            if ml.contains("gemini") {
                return (*m).clone();
            }
        } else if p.contains("xai") || p.contains("grok") {
            if ml.contains("grok") {
                return (*m).clone();
            }
        } else if p.contains("kimi") {
            if ml.contains("kimi") {
                return (*m).clone();
            }
        }
    }
    // First text model, else default for provider (never first raw model if it's image-only).
    if let Some(first) = text_models.first() {
        return (*first).clone();
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
    fn heuristic_avoids_image_models() {
        let models = vec![
            "gemini-3.1-flash-image".into(),
            "gemini-2.5-pro".into(),
            "some-embed-model".into(),
        ];
        let picked = pick_model_for_provider("antigravity", &models);
        assert_eq!(picked, "gemini-2.5-pro");
        assert!(!is_non_text_model(&picked));
    }

    #[test]
    fn heuristic_skips_all_non_text_to_default() {
        let models = vec![
            "gemini-flash-image".into(),
            "foo-tts".into(),
            "bar-embed".into(),
        ];
        let picked = pick_model_for_provider("antigravity", &models);
        // No text models → provider default
        assert_eq!(picked, "gemini-2.5-pro");
    }

    #[test]
    fn override_respected_over_heuristic() {
        let account = AuthAccount {
            id: "antigravity-user@x.json".into(),
            name: "antigravity-user@x.json".into(),
            provider: "antigravity".into(),
            email: Some("user@x".into()),
            label: None,
            status: "active".into(),
            status_message: String::new(),
            unavailable: false,
            disabled: false,
        };
        let models = vec![
            "gemini-3.1-flash-image".into(),
            "gemini-2.5-pro".into(),
        ];
        let mut overrides = HashMap::new();
        overrides.insert(
            "antigravity-user@x.json".into(),
            "gemini-2.5-flash".into(),
        );
        let m = resolve_model_for_account(&account, &models, &overrides);
        assert_eq!(m, "gemini-2.5-flash");
    }

    #[test]
    fn sync_uses_override() {
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
        let mut overrides = HashMap::new();
        overrides.insert("claude-a@b.com.json".into(), "claude-opus-4".into());
        let ids = sync_subscription_profiles(
            &reg,
            &accounts,
            "http://127.0.0.1:18317/v1",
            "cliproxy_proxy_key",
            &["claude-sonnet-4-5".into()],
            &overrides,
        )
        .unwrap();
        let p = reg.current().unwrap().file.backend_profiles[&ids[0]].clone();
        assert_eq!(p.model, "claude-opus-4");
    }

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
            &HashMap::new(),
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

        let ids2 = sync_subscription_profiles(
            &reg,
            &[],
            "http://127.0.0.1:18317/v1",
            "cliproxy_proxy_key",
            &[],
            &HashMap::new(),
        )
        .unwrap();
        assert!(ids2.is_empty());
        let cfg2 = reg.current().unwrap();
        assert!(!cfg2.file.backend_profiles.contains_key(&ids[0]));

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
