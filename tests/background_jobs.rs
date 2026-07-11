//! Background delegation: start returns instantly, poll reaches the result,
//! failures are reported, unknown ids are clear.

use std::sync::Arc;
use std::time::{Duration, Instant};

use orchestrator::conversation::ConversationStore;
use orchestrator::core::{DelegateRequest, Orchestrator};
use orchestrator::jobs::JobState;
use orchestrator::registry::SlotRegistry;
use orchestrator::secrets::MemorySecretStore;
use serde_json::json;
use tempfile::tempdir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn orchestrator_with_backend(delay: Duration, status: u16) -> (Orchestrator, MockServer, tempfile::TempDir) {
    let backend = MockServer::start().await;
    let template = if status == 200 {
        ResponseTemplate::new(200)
            .set_delay(delay)
            .set_body_json(json!({
                "choices": [{ "message": { "role": "assistant", "content": "BG_RESULT" } }]
            }))
    } else {
        ResponseTemplate::new(status)
            .set_delay(delay)
            .set_body_string("quota exceeded")
    };
    Mock::given(method("POST"))
        .and(path("/v1/chat/completions"))
        .respond_with(template)
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

    let orch = Orchestrator::new(
        Arc::new(SlotRegistry::open(&slots_path).unwrap()),
        Arc::new(ConversationStore::new(dir.path().join("conversations")).unwrap()),
        Arc::new(MemorySecretStore::new()),
    );
    (orch, backend, dir)
}

#[tokio::test]
async fn background_job_starts_instantly_and_completes() {
    let (orch, _backend, _dir) = orchestrator_with_backend(Duration::from_secs(2), 200).await;

    let started = Instant::now();
    let job_id = orch.delegate_background(DelegateRequest {
        task: "big job".into(),
        ..Default::default()
    });
    assert!(
        started.elapsed() < Duration::from_millis(250),
        "start must not block on generation"
    );

    // Immediately: running.
    let view = orch.jobs().get(&job_id).expect("job exists");
    assert!(matches!(view.state, JobState::Running));

    // Poll until done (well under the 2s+margin).
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let view = orch.jobs().get(&job_id).expect("job exists");
        match view.state {
            JobState::Done { ref response, ref conversation_id } => {
                assert_eq!(response, "BG_RESULT");
                assert!(!conversation_id.is_empty());
                break;
            }
            JobState::Failed { ref error } => panic!("unexpected failure: {error}"),
            JobState::Running => {
                assert!(Instant::now() < deadline, "job never completed");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    }
}

#[tokio::test]
async fn background_job_failure_is_reported() {
    let (orch, _backend, _dir) = orchestrator_with_backend(Duration::from_millis(50), 429).await;

    let job_id = orch.delegate_background(DelegateRequest {
        task: "doomed".into(),
        ..Default::default()
    });

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let view = orch.jobs().get(&job_id).expect("job exists");
        match view.state {
            JobState::Failed { ref error } => {
                assert!(error.contains("worker unavailable"), "got: {error}");
                break;
            }
            JobState::Done { .. } => panic!("should have failed"),
            JobState::Running => {
                assert!(Instant::now() < deadline, "job never finished");
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
    }
}

#[tokio::test]
async fn unknown_job_id_is_none() {
    let (orch, _backend, _dir) = orchestrator_with_backend(Duration::from_millis(10), 200).await;
    assert!(orch.jobs().get("nope").is_none());
}
