//! Proves the single most important behavior: slot is resolved at call time.
//!
//! A mid-process edit to slots.json must take effect on the very next
//! `delegate` call with no restart and no reconnect.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use orchestrator::backends::ChatMessage;
use orchestrator::conversation::ConversationStore;
use orchestrator::core::{DelegateRequest, Orchestrator};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_slots(path: &std::path::Path, base_url: &str, model: &str) {
    let doc = json!({
        "listen": "127.0.0.1:0",
        "bearer_token_ref": "mcp_bearer_token",
        "conversations_dir": "conversations",
        "slots": {
            "worker": {
                "description": "General-purpose worker",
                "backend": "openai_compatible",
                "base_url": base_url,
                "model": model,
                "auth_ref": "worker_api_key",
                "enable_fallback": false
            }
        }
    });
    fs::write(path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

async fn mock_backend(label: &str) -> MockServer {
    let server = MockServer::start().await;
    let body = json!({
        "id": format!("chat-{label}"),
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": format!("RESPONSE_FROM_{label}")
            },
            "finish_reason": "stop"
        }]
    });
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn hot_swap_takes_effect_on_next_delegate() {
    let backend_a = mock_backend("A").await;
    let backend_b = mock_backend("B").await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(
        &slots_path,
        &format!("{}/v1", backend_a.uri()),
        "model-a",
    );

    let secrets = Arc::new(MemorySecretStore::with_secrets([
        ("worker_api_key".into(), "test-key".into()),
        ("mcp_bearer_token".into(), "test-token".into()),
    ]));
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let mut orch = Orchestrator::new(registry, conversations, secrets);
    orch.expose_backend_id = true;

    // First call → backend A
    let r1 = orch
        .delegate(DelegateRequest {
            task: "say hello".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r1.response, "RESPONSE_FROM_A");
    assert!(
        r1.backend_id.as_ref().unwrap().contains(&backend_a.uri()),
        "expected backend A id, got {:?}",
        r1.backend_id
    );

    // Hot-swap slots.json on disk (same process, no restart).
    // Windows mtime resolution can be 1s.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    write_slots(
        &slots_path,
        &format!("{}/v1", backend_b.uri()),
        "model-b",
    );

    // Second call on the SAME Orchestrator instance → must hit backend B.
    let r2 = orch
        .delegate(DelegateRequest {
            task: "say hello again".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r2.response, "RESPONSE_FROM_B");
    assert!(
        r2.backend_id.as_ref().unwrap().contains(&backend_b.uri()),
        "expected backend B id after hot-swap, got {:?}",
        r2.backend_id
    );
    assert_ne!(r1.backend_id, r2.backend_id);
}

#[tokio::test]
async fn list_slots_never_leaks_vendor_or_model() {
    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(&slots_path, "http://secret-vendor.internal/v1", "gpt-4o-secret");

    let secrets = Arc::new(MemorySecretStore::new());
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let orch = Orchestrator::new(registry, conversations, secrets);

    let slots = orch.list_slots().unwrap();
    let dumped = serde_json::to_string(&slots).unwrap();
    assert!(dumped.contains("worker"));
    assert!(dumped.contains("General-purpose worker"));
    assert!(!dumped.contains("gpt-4o-secret"));
    assert!(!dumped.contains("secret-vendor"));
    assert!(!dumped.contains("openai"));
    assert!(!dumped.contains("base_url"));

    // Silence unused import warning in some toolchains.
    let _ = ChatMessage::user("x");
}
