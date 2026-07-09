//! Conversation history lives orchestrator-side, so a continued thread still
//! works after the slot is swapped to a different backend mid-thread.

use std::fs;
use std::sync::Arc;
use std::time::Duration;

use orchestrator::conversation::ConversationStore;
use orchestrator::core::{DelegateRequest, Orchestrator};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{body_partial_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn write_slots(path: &std::path::Path, base_url: &str, model: &str) {
    let doc = json!({
        "slots": {
            "worker": {
                "description": "General-purpose worker",
                "backend": "openai_compatible",
                "base_url": base_url,
                "model": model,
                "auth_ref": "worker_api_key"
            }
        }
    });
    fs::write(path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
}

#[tokio::test]
async fn continued_conversation_replays_history() {
    let server = MockServer::start().await;

    // First turn: no prior history required.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "My favorite color is blue." }
            }]
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // Second turn: request body must include the prior user+assistant messages.
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "messages": [
                { "role": "user", "content": "Remember: your favorite color is blue." },
                { "role": "assistant", "content": "My favorite color is blue." },
                { "role": "user", "content": "What is your favorite color?" }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": { "role": "assistant", "content": "blue" }
            }]
        })))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(&slots_path, &format!("{}/v1", server.uri()), "model-x");

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "worker_api_key".into(),
        "k".into(),
    )]));
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let orch = Orchestrator::new(registry, conversations, secrets);

    let r1 = orch
        .delegate(DelegateRequest {
            task: "Remember: your favorite color is blue.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(r1.response.contains("blue"));

    let r2 = orch
        .delegate(DelegateRequest {
            task: "What is your favorite color?".into(),
            conversation_id: Some(r1.conversation_id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(r2.conversation_id, r1.conversation_id);
    assert_eq!(r2.response, "blue");
}

#[tokio::test]
async fn conversation_survives_slot_swap_mid_thread() {
    let backend_a = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "Secret code is ORCH-42. I am backend A."
                }
            }]
        })))
        .mount(&backend_a)
        .await;

    let backend_b = MockServer::start().await;
    // Backend B must receive full prior history (including A's assistant reply).
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .and(body_partial_json(json!({
            "messages": [
                { "role": "user", "content": "Memorize secret code ORCH-42." },
                {
                    "role": "assistant",
                    "content": "Secret code is ORCH-42. I am backend A."
                },
                { "role": "user", "content": "What was the secret code?" }
            ]
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": "ORCH-42 (answered by backend B with prior history)"
                }
            }]
        })))
        .mount(&backend_b)
        .await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(
        &slots_path,
        &format!("{}/v1", backend_a.uri()),
        "model-a",
    );

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "worker_api_key".into(),
        "k".into(),
    )]));
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let mut orch = Orchestrator::new(registry, conversations, secrets);
    orch.expose_backend_id = true;

    let r1 = orch
        .delegate(DelegateRequest {
            task: "Memorize secret code ORCH-42.".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(r1.response.contains("ORCH-42"));
    assert!(r1.backend_id.as_ref().unwrap().contains(&backend_a.uri()));

    // Swap slot to backend B mid-thread.
    tokio::time::sleep(Duration::from_millis(1100)).await;
    write_slots(
        &slots_path,
        &format!("{}/v1", backend_b.uri()),
        "model-b",
    );

    let r2 = orch
        .delegate(DelegateRequest {
            task: "What was the secret code?".into(),
            conversation_id: Some(r1.conversation_id.clone()),
            ..Default::default()
        })
        .await
        .unwrap();

    assert_eq!(r2.conversation_id, r1.conversation_id);
    assert!(
        r2.backend_id.as_ref().unwrap().contains(&backend_b.uri()),
        "expected backend B after swap, got {:?}",
        r2.backend_id
    );
    assert!(
        r2.response.contains("ORCH-42") && r2.response.contains("backend B"),
        "backend B should answer using orchestrator-side history: {}",
        r2.response
    );
}

#[tokio::test]
async fn omit_conversation_id_starts_fresh() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "choices": [{ "message": { "role": "assistant", "content": "ok" } }]
        })))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(&slots_path, &format!("{}/v1", server.uri()), "m");

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "worker_api_key".into(),
        "k".into(),
    )]));
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let orch = Orchestrator::new(registry, conversations, secrets);

    let r1 = orch
        .delegate(DelegateRequest {
            task: "first".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    let r2 = orch
        .delegate(DelegateRequest {
            task: "second".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_ne!(r1.conversation_id, r2.conversation_id);
}

#[tokio::test]
async fn backend_error_returns_worker_unavailable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(ResponseTemplate::new(429).set_body_string("rate limited"))
        .mount(&server)
        .await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    write_slots(&slots_path, &format!("{}/v1", server.uri()), "m");

    let secrets = Arc::new(MemorySecretStore::with_secrets([(
        "worker_api_key".into(),
        "k".into(),
    )]));
    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations = Arc::new(
        ConversationStore::new(dir.path().join("conversations")).unwrap(),
    );
    let orch = Orchestrator::new(registry, conversations, secrets);

    let err = orch
        .delegate(DelegateRequest {
            task: "go".into(),
            ..Default::default()
        })
        .await
        .unwrap_err();
    let msg = err.to_caller_message();
    assert!(msg.starts_with("worker unavailable:"), "{msg}");
    assert!(msg.contains("quota") || msg.contains("429"), "{msg}");
}
