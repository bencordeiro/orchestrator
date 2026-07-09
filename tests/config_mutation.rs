//! GUI config mutation path: write → force_reload → next resolve sees change.
//!
//! Does **not** rely on filesystem mtime.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use orchestrator::config::{BackendKind, BackendProfile, SlotConfig};
use orchestrator::conversation::ConversationStore;
use orchestrator::core::{DelegateRequest, Orchestrator};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn mutate_then_resolve_without_mtime_delay() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        r#"{
          "slots": {
            "worker": {
              "description": "w",
              "backend": "openai_compatible",
              "base_url": "http://old/v1",
              "model": "old-model"
            }
          }
        }"#,
    )
    .unwrap();

    let registry = SlotRegistry::open(&path).unwrap();
    assert_eq!(registry.resolve("worker").unwrap().model, "old-model");

    // Immediate write via mutate (force_reload) — no sleep.
    registry
        .mutate(|file| {
            let s = file.slots.get_mut("worker").unwrap();
            s.model = "new-model".into();
            s.base_url = "http://new/v1".into();
            Ok(())
        })
        .unwrap();

    let slot = registry.resolve("worker").unwrap();
    assert_eq!(slot.model, "new-model");
    assert_eq!(slot.base_url, "http://new/v1");
}

#[test]
fn force_reload_after_external_write_without_mtime_wait() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://a/v1","model":"a"}}}"#,
    )
    .unwrap();

    let registry = SlotRegistry::open(&path).unwrap();

    // External writer (simulates GUI file write) — may not advance mtime on all FS.
    fs::write(
        &path,
        r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://b/v1","model":"b"}}}"#,
    )
    .unwrap();

    // Without force_reload, mtime-based current() might still return "a" on coarse FS.
    // GUI contract: always force_reload after write.
    registry.force_reload().unwrap();
    assert_eq!(registry.resolve("worker").unwrap().model, "b");
}

#[tokio::test]
async fn gui_swap_affects_next_delegate() {
    let backend_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "FROM_A" } }]
        })))
        .mount(&backend_a)
        .await;

    let backend_b = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "FROM_B" } }]
        })))
        .mount(&backend_b)
        .await;

    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    let doc = json!({
        "backend_profiles": {
            "a": {
                "label": "A",
                "backend": "openai_compatible",
                "base_url": format!("{}/v1", backend_a.uri()),
                "model": "ma",
                "auth_ref": "k"
            },
            "b": {
                "label": "B",
                "backend": "openai_compatible",
                "base_url": format!("{}/v1", backend_b.uri()),
                "model": "mb",
                "auth_ref": "k"
            }
        },
        "slots": {
            "worker": {
                "description": "General worker",
                "backend": "openai_compatible",
                "base_url": format!("{}/v1", backend_a.uri()),
                "model": "ma",
                "auth_ref": "k"
            }
        }
    });
    fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let secrets = Arc::new(MemorySecretStore::with_secrets([("k".into(), "v".into())]));
    let registry = Arc::new(SlotRegistry::open(&path).unwrap());
    let conversations =
        Arc::new(ConversationStore::new(dir.path().join("conversations")).unwrap());
    let orch = Orchestrator::new(registry.clone(), conversations, secrets);

    let r1 = orch
        .delegate(DelegateRequest {
            task: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r1.response, "FROM_A");

    // GUI swap via assign_backend → force_reload (no mtime wait).
    registry.assign_backend("worker", "b").unwrap();

    let r2 = orch
        .delegate(DelegateRequest {
            task: "hi again".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r2.response, "FROM_B");
}

#[tokio::test]
async fn failed_fresh_delegate_leaves_no_orphan_conversation() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("boom"))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        json!({
            "slots": {
                "worker": {
                    "description": "w",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", server.uri()),
                    "model": "m",
                    "auth_ref": "k"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let conv_dir = dir.path().join("conversations");
    let secrets = Arc::new(MemorySecretStore::with_secrets([("k".into(), "v".into())]));
    let registry = Arc::new(SlotRegistry::open(&path).unwrap());
    let conversations = Arc::new(ConversationStore::new(&conv_dir).unwrap());
    let orch = Orchestrator::new(registry, conversations.clone(), secrets);

    let err = orch
        .delegate(DelegateRequest {
            task: "fail please".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_caller_message().starts_with("worker unavailable:"));

    // No conversation files should exist.
    let ids = conversations.list_ids().unwrap();
    assert!(
        ids.is_empty(),
        "expected no orphan conversations, found {ids:?}"
    );
    assert_eq!(fs::read_dir(&conv_dir).unwrap().count(), 0);
}

#[test]
fn upsert_and_remove_slot() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://a/v1","model":"m"}}}"#,
    )
    .unwrap();

    let registry = SlotRegistry::open(&path).unwrap();
    registry
        .upsert_slot(
            "reviewer",
            SlotConfig {
                description: "Reviews code".into(),
                backend: BackendKind::OpenaiCompatible,
                base_url: "http://r/v1".into(),
                model: "review".into(),
                auth_ref: None,
                fallback: None,
                enable_fallback: false,
            },
        )
        .unwrap();

    assert!(registry.resolve("reviewer").is_ok());
    registry.remove_slot("reviewer").unwrap();
    assert!(registry.resolve("reviewer").is_err());
}

#[test]
fn upsert_backend_profile() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        r#"{"slots":{"worker":{"description":"w","backend":"openai_compatible","base_url":"http://a/v1","model":"m"}}}"#,
    )
    .unwrap();

    let registry = SlotRegistry::open(&path).unwrap();
    registry
        .upsert_backend_profile(
            "ollama",
            BackendProfile {
                label: "Ollama local".into(),
                backend: BackendKind::OpenaiCompatible,
                base_url: "http://127.0.0.1:11434/v1".into(),
                model: "llama3".into(),
                auth_ref: None,
            },
        )
        .unwrap();

    // Tiny delay not required; force_reload already ran.
    let _ = Duration::from_millis(0);
    let cfg = registry.current().unwrap();
    assert!(cfg.file.backend_profiles.contains_key("ollama"));
}
