//! Background delegation jobs.
//!
//! `delegate(background=true)` returns a job id immediately; the caller polls
//! `job_result(job_id)` with cheap fast calls. No connection is held open for
//! the duration of the generation, so no client/bridge timeout can kill a
//! long-running worker job.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use uuid::Uuid;

use crate::core::DelegateResult;

/// Completed jobs are kept for this long after finishing (poll window).
pub const RETENTION: Duration = Duration::from_secs(6 * 3600);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum JobState {
    /// Worker still generating.
    Running,
    /// Finished successfully.
    Done {
        conversation_id: String,
        response: String,
    },
    /// Finished with a worker error (message is the caller-facing reason).
    Failed { error: String },
}

#[derive(Debug)]
struct JobRecord {
    state: JobState,
    slot: String,
    started: Instant,
    finished: Option<Instant>,
}

/// Thread-safe in-memory job table.
///
/// Jobs do not survive an app restart — a restart kills in-flight generations
/// anyway, and `job_result` reports unknown ids clearly.
#[derive(Debug, Default)]
pub struct JobStore {
    inner: Mutex<HashMap<String, JobRecord>>,
}

/// Caller-facing snapshot of a job.
#[derive(Debug, Clone, Serialize)]
pub struct JobView {
    pub job_id: String,
    pub slot: String,
    pub elapsed_seconds: u64,
    #[serde(flatten)]
    pub state: JobState,
}

impl JobStore {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new running job, returning its id.
    pub fn start(&self, slot: &str) -> String {
        let id = Uuid::new_v4().to_string();
        self.inner.lock().unwrap().insert(
            id.clone(),
            JobRecord {
                state: JobState::Running,
                slot: slot.to_string(),
                started: Instant::now(),
                finished: None,
            },
        );
        id
    }

    pub fn complete(&self, id: &str, result: &DelegateResult) {
        if let Some(rec) = self.inner.lock().unwrap().get_mut(id) {
            rec.state = JobState::Done {
                conversation_id: result.conversation_id.clone(),
                response: result.response.clone(),
            };
            rec.finished = Some(Instant::now());
        }
    }

    pub fn fail(&self, id: &str, error: String) {
        if let Some(rec) = self.inner.lock().unwrap().get_mut(id) {
            rec.state = JobState::Failed { error };
            rec.finished = Some(Instant::now());
        }
    }

    /// Look up a job (also prunes expired finished jobs).
    pub fn get(&self, id: &str) -> Option<JobView> {
        let mut map = self.inner.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, r| match r.finished {
            Some(f) => now.duration_since(f) < RETENTION,
            None => true,
        });
        map.get(id).map(|r| JobView {
            job_id: id.to_string(),
            slot: r.slot.clone(),
            elapsed_seconds: now.duration_since(r.started).as_secs(),
            state: r.state.clone(),
        })
    }

    /// All current jobs, newest first (for the GUI).
    pub fn list(&self) -> Vec<JobView> {
        let map = self.inner.lock().unwrap();
        let now = Instant::now();
        let mut out: Vec<(Instant, JobView)> = map
            .iter()
            .map(|(id, r)| {
                (
                    r.started,
                    JobView {
                        job_id: id.clone(),
                        slot: r.slot.clone(),
                        elapsed_seconds: now.duration_since(r.started).as_secs(),
                        state: r.state.clone(),
                    },
                )
            })
            .collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out.into_iter().map(|(_, v)| v).collect()
    }
}
