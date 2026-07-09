//! Subscription / OpenAI-compatible failure surfaces as `worker unavailable`.
//!
//! CLIProxyAPI is just another openai_compatible backend — core stays unaware.

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
async fn dead_endpoint_is_worker_unavailable() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("slots.json");
    // Nothing listening on this port.
    fs::write(
        &path,
        r#"{
          "slots": {
            "worker": {
              "description": "w",
              "backend": "openai_compatible",
              "base_url": "http://127.0.0.1:1/v1",
              "model": "claude-sonnet-4-5",
              "auth_ref": "cliproxy_proxy_key"
            }
          }
        }"#,
    )
    .unwrap();

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "cliproxy_proxy_key".into(),
        "k".into(),
    )]));
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
    let msg = err.to_caller_message();
    assert!(msg.starts_with("worker unavailable:"), "{msg}");
    assert!(
        msg.contains("connection failed") || msg.contains("error"),
        "{msg}"
    );
}

#[tokio::test]
async fn auth_failure_from_proxy_is_worker_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_string("Invalid API key / oauth revoked"))
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
                    "model": "gpt-5.1",
                    "auth_ref": "cliproxy_proxy_key"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "cliproxy_proxy_key".into(),
        "bad".into(),
    )]));
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
    let msg = err.to_caller_message();
    assert!(msg.starts_with("worker unavailable:"), "{msg}");
    assert!(msg.contains("auth"), "{msg}");
}

#[tokio::test]
async fn quota_from_proxy_is_worker_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("quota exceeded"))
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
                    "model": "claude-sonnet-4-5",
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
    let msg = err.to_caller_message();
    assert!(msg.starts_with("worker unavailable:"), "{msg}");
    assert!(msg.contains("quota") || msg.contains("429"), "{msg}");
}
