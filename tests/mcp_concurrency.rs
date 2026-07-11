//! Proves one MCP session handles simultaneous `delegate` calls concurrently
//! (they overlap in flight rather than queueing behind each other).
//!
//! Regression context: an agent session blamed slow bulk jobs on a
//! "single-worker queue" — this pins down that no such queue exists.

use std::sync::Arc;
use std::time::{Duration, Instant};

use orchestrator::conversation::ConversationStore;
use orchestrator::core::Orchestrator;
use orchestrator::mcp::server::{build_router, McpState};
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const TOKEN: &str = "test-token";

async fn post_mcp(
    client: &reqwest::Client,
    url: &str,
    session: Option<&str>,
    body: serde_json::Value,
) -> reqwest::Response {
    let mut req = client
        .post(url)
        .bearer_auth(TOKEN)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .body(body.to_string());
    if let Some(s) = session {
        req = req.header("mcp-session-id", s);
    }
    req.send().await.expect("mcp post")
}

#[tokio::test]
async fn two_delegates_in_one_session_run_concurrently() {
    // Backend that takes 2s per completion.
    let backend = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(2))
                .set_body_json(json!({
                    "choices": [{ "message": { "role": "assistant", "content": "SLOW_OK" } }]
                })),
        )
        .mount(&backend)
        .await;

    let dir = tempdir().unwrap();
    let slots_path = dir.path().join("slots.json");
    std::fs::write(
        &slots_path,
        json!({
            "slots": {
                "worker": {
                    "description": "w",
                    "backend": "openai_compatible",
                    "base_url": format!("{}/v1", backend.uri()),
                    "model": "m"
                }
            }
        })
        .to_string(),
    )
    .unwrap();

    let registry = Arc::new(SlotRegistry::open(&slots_path).unwrap());
    let conversations =
        Arc::new(ConversationStore::new(dir.path().join("conversations")).unwrap());
    let secrets = Arc::new(MemorySecretStore::new());
    let orchestrator = Orchestrator::new(registry, conversations, secrets);

    let state = McpState {
        orchestrator,
        bearer_token: Arc::new(TOKEN.to_string()),
    };
    let app = build_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let url = format!("http://{addr}/mcp");

    let client = reqwest::Client::new();

    // Initialize ONE session.
    let init = post_mcp(
        &client,
        &url,
        None,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "test", "version": "0" }
            }
        }),
    )
    .await;
    let session = init
        .headers()
        .get("mcp-session-id")
        .expect("session id header")
        .to_str()
        .unwrap()
        .to_string();
    let _ = init.text().await;

    let _ = post_mcp(
        &client,
        &url,
        Some(&session),
        json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }),
    )
    .await;

    // Fire two delegates simultaneously in the SAME session.
    let call = |id: u32| {
        post_mcp(
            &client,
            &url,
            Some(&session),
            json!({
                "jsonrpc": "2.0", "id": id, "method": "tools/call",
                "params": { "name": "delegate", "arguments": { "task": format!("job {id}") } }
            }),
        )
    };

    let started = Instant::now();
    let (r1, r2) = tokio::join!(call(2), call(3));
    let b1 = r1.text().await.unwrap();
    let b2 = r2.text().await.unwrap();
    let elapsed = started.elapsed();

    assert!(b1.contains("SLOW_OK"), "first call failed: {b1}");
    assert!(b2.contains("SLOW_OK"), "second call failed: {b2}");

    // Concurrent: ~2s. Serialized: ~4s. Allow generous margin.
    assert!(
        elapsed < Duration::from_millis(3500),
        "delegates serialized within one session: took {elapsed:?} (expected ~2s)"
    );
}
