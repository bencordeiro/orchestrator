//! Fallback chains engage only when `enable_fallback` is true (off by default).

use std::fs;
use std::sync::Arc;

use orchestrator::conversation::ConversationStore;
use orchestrator::core::{DelegateRequest, Orchestrator};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn fallback_off_by_default_does_not_try_chain() {
    let primary = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("primary down"))
        .mount(&primary)
        .await;

    let fb = MockServer::start().await;
    // If fallback were tried, this would answer.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "FROM_FALLBACK" } }]
        })))
        .mount(&fb)
        .await;

    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        json!({
            "slots": {
                "worker": {
                    "description": "primary",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", primary.uri()),
                    "model": "m1",
                    "auth_ref": "k",
                    "fallback": ["backup"],
                    "enable_fallback": false
                },
                "backup": {
                    "description": "backup",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", fb.uri()),
                    "model": "m2",
                    "auth_ref": "k"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let secrets = Arc::new(MemorySecretStore::with_secrets([("k".into(), "v".into())]));
    let registry = Arc::new(SlotRegistry::open(&path).unwrap());
    let conversations =
        Arc::new(ConversationStore::new(dir.path().join("conversations")).unwrap());
    let orch = Orchestrator::new(registry, conversations, secrets);

    let err = orch
        .delegate(DelegateRequest {
            task: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(err.to_caller_message().starts_with("worker unavailable:"));
    // Must NOT have gotten fallback success.
}

#[tokio::test]
async fn fallback_engages_when_opted_in() {
    let primary = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_string("primary down"))
        .mount(&primary)
        .await;

    let fb = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "FROM_FALLBACK" } }]
        })))
        .mount(&fb)
        .await;

    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    fs::write(
        &path,
        json!({
            "slots": {
                "worker": {
                    "description": "primary",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", primary.uri()),
                    "model": "m1",
                    "auth_ref": "k",
                    "fallback": ["backup"],
                    "enable_fallback": true
                },
                "backup": {
                    "description": "backup",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", fb.uri()),
                    "model": "m2",
                    "auth_ref": "k"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let secrets = Arc::new(MemorySecretStore::with_secrets([("k".into(), "v".into())]));
    let registry = Arc::new(SlotRegistry::open(&path).unwrap());
    let conversations =
        Arc::new(ConversationStore::new(dir.path().join("conversations")).unwrap());
    let orch = Orchestrator::new(registry, conversations, secrets);

    let r = orch
        .delegate(DelegateRequest {
            task: "hi".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r.response, "FROM_FALLBACK");
}
