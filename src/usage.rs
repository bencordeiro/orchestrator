//! JSONL usage log per slot/backend with simple size-based rotation.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::{OrchestratorError, Result};

/// Default max log file size before rotation (5 MiB).
pub const DEFAULT_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// One usage event (success or failure).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageEvent {
    pub ts: DateTime<Utc>,
    pub slot: String,
    /// Matched backend profile id, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    pub base_url: String,
    pub model: String,
    pub latency_ms: u64,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Thread-safe JSONL writer with rotation.
#[derive(Debug)]
pub struct UsageLog {
    path: PathBuf,
    max_bytes: u64,
    lock: Mutex<()>,
}

impl UsageLog {
    pub fn open(path: impl AsRef<Path>, max_bytes: u64) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        Ok(Self {
            path,
            max_bytes: max_bytes.max(1024),
            lock: Mutex::new(()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&self, event: &UsageEvent) -> Result<()> {
        let _g = self.lock.lock().unwrap();
        self.rotate_if_needed()?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let line = serde_json::to_string(event)?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    fn rotate_if_needed(&self) -> Result<()> {
        let meta = match fs::metadata(&self.path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        if meta.len() < self.max_bytes {
            return Ok(());
        }
        let backup = self.path.with_extension("jsonl.1");
        let _ = fs::remove_file(&backup);
        fs::rename(&self.path, &backup).map_err(|e| {
            OrchestratorError::Other(anyhow::anyhow!("usage log rotate: {e}"))
        })?;
        // Touch empty primary file.
        File::create(&self.path)?;
        Ok(())
    }

    /// Read the most recent `limit` events (newest first).
    pub fn recent(&self, limit: usize) -> Result<Vec<UsageEvent>> {
        let _g = self.lock.lock().unwrap();
        if !self.path.exists() {
            return Ok(vec![]);
        }
        let f = File::open(&self.path)?;
        let reader = BufReader::new(f);
        let mut events: Vec<UsageEvent> = Vec::new();
        for line in reader.lines() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(ev) = serde_json::from_str::<UsageEvent>(&line) {
                events.push(ev);
            }
        }
        events.reverse();
        events.truncate(limit);
        Ok(events)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_and_recent() {
        let dir = tempdir().unwrap();
        let log = UsageLog::open(dir.path().join("usage.jsonl"), DEFAULT_MAX_BYTES).unwrap();
        for i in 0..5 {
            log.record(&UsageEvent {
                ts: Utc::now(),
                slot: "worker".into(),
                profile_id: Some("p".into()),
                base_url: "http://localhost:11434/v1".into(),
                model: "m".into(),
                latency_ms: i,
                success: i % 2 == 0,
                reason: if i % 2 == 0 {
                    None
                } else {
                    Some("err".into())
                },
            })
            .unwrap();
        }
        let recent = log.recent(3).unwrap();
        assert_eq!(recent.len(), 3);
        // Newest first: last written has latency 4
        assert_eq!(recent[0].latency_ms, 4);
    }

    #[test]
    fn rotates_when_over_cap() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("usage.jsonl");
        let log = UsageLog::open(&path, 200).unwrap();
        for i in 0..50 {
            log.record(&UsageEvent {
                ts: Utc::now(),
                slot: format!("slot-{i}"),
                profile_id: None,
                base_url: "http://localhost:11434/v1".into(),
                model: "llama3.2".into(),
                latency_ms: 1,
                success: true,
                reason: None,
            })
            .unwrap();
        }
        assert!(path.exists());
        // Either rotated backup exists or primary stayed small after rotate.
        let backup = path.with_extension("jsonl.1");
        let primary_len = fs::metadata(&path).unwrap().len();
        assert!(
            backup.exists() || primary_len < 2000,
            "expected rotation; primary={primary_len} backup={}",
            backup.exists()
        );
    }
}
