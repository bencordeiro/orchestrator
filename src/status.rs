//! Live per-slot call status for the GUI.

use std::collections::HashMap;
use std::sync::RwLock;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Snapshot of the most recent call against a slot.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SlotStatus {
    pub last_call_at: Option<DateTime<Utc>>,
    pub last_latency_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_success: Option<bool>,
}

/// Thread-safe board of slot statuses, updated by `delegate`.
#[derive(Debug, Default)]
pub struct StatusBoard {
    inner: RwLock<HashMap<String, SlotStatus>>,
}

impl StatusBoard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_success(&self, slot: &str, latency_ms: u64) {
        let mut map = self.inner.write().unwrap();
        map.insert(
            slot.to_string(),
            SlotStatus {
                last_call_at: Some(Utc::now()),
                last_latency_ms: Some(latency_ms),
                last_error: None,
                last_success: Some(true),
            },
        );
    }

    pub fn record_error(&self, slot: &str, latency_ms: u64, error: impl Into<String>) {
        let mut map = self.inner.write().unwrap();
        map.insert(
            slot.to_string(),
            SlotStatus {
                last_call_at: Some(Utc::now()),
                last_latency_ms: Some(latency_ms),
                last_error: Some(error.into()),
                last_success: Some(false),
            },
        );
    }

    pub fn get(&self, slot: &str) -> SlotStatus {
        self.inner
            .read()
            .unwrap()
            .get(slot)
            .cloned()
            .unwrap_or_default()
    }

    pub fn all(&self) -> HashMap<String, SlotStatus> {
        self.inner.read().unwrap().clone()
    }
}
