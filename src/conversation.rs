//! Orchestrator-side conversation history, persisted to disk.
//!
//! History lives here (not on the worker backend), so a continued thread still
//! works after a mid-conversation slot swap.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::backends::ChatMessage;
use crate::error::{OrchestratorError, Result};

/// On-disk conversation record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Last slot name used (informational only; not used for routing).
    pub last_slot: Option<String>,
    pub messages: Vec<ChatMessage>,
}

/// Disk-backed conversation store. One JSON file per conversation id.
#[derive(Debug, Clone)]
pub struct ConversationStore {
    dir: PathBuf,
}

impl ConversationStore {
    pub fn new(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self { dir })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, id: &str) -> PathBuf {
        // Guard against path traversal in conversation ids.
        let safe: String = id
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(format!("{safe}.json"))
    }

    /// Load an existing conversation or error if missing.
    pub fn get(&self, id: &str) -> Result<Conversation> {
        let path = self.path_for(id);
        if !path.exists() {
            return Err(OrchestratorError::Conversation(format!(
                "unknown conversation_id '{id}'"
            )));
        }
        let raw = fs::read_to_string(&path)?;
        let conv: Conversation = serde_json::from_str(&raw)?;
        Ok(conv)
    }

    /// Allocate a new conversation id **without** writing to disk.
    ///
    /// Used for fresh `delegate` jobs so a failed backend call does not leave
    /// an orphan empty conversation file.
    pub fn allocate_id() -> String {
        Uuid::new_v4().to_string()
    }

    /// Build an in-memory conversation shell (not persisted).
    pub fn new_empty() -> Conversation {
        let now = Utc::now();
        Conversation {
            id: Self::allocate_id(),
            created_at: now,
            updated_at: now,
            last_slot: None,
            messages: Vec::new(),
        }
    }

    /// Create a brand-new conversation and persist it.
    pub fn create(&self) -> Result<Conversation> {
        let conv = Self::new_empty();
        self.save(&conv)?;
        Ok(conv)
    }

    /// Persist a brand-new conversation with initial messages (fresh successful job).
    pub fn create_with_messages(
        &self,
        id: &str,
        messages: &[ChatMessage],
        last_slot: Option<&str>,
    ) -> Result<Conversation> {
        let now = Utc::now();
        let conv = Conversation {
            id: id.to_string(),
            created_at: now,
            updated_at: now,
            last_slot: last_slot.map(|s| s.to_string()),
            messages: messages.to_vec(),
        };
        self.save(&conv)?;
        Ok(conv)
    }

    /// Append messages and update metadata.
    pub fn append(
        &self,
        id: &str,
        messages: &[ChatMessage],
        last_slot: Option<&str>,
    ) -> Result<Conversation> {
        let mut conv = self.get(id)?;
        conv.messages.extend(messages.iter().cloned());
        conv.updated_at = Utc::now();
        if let Some(slot) = last_slot {
            conv.last_slot = Some(slot.to_string());
        }
        self.save(&conv)?;
        Ok(conv)
    }

    /// True if a conversation file exists for `id`.
    pub fn exists(&self, id: &str) -> bool {
        self.path_for(id).exists()
    }

    /// Replace the full message list (used by tests / repair).
    pub fn save(&self, conv: &Conversation) -> Result<()> {
        let path = self.path_for(&conv.id);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let raw = serde_json::to_string_pretty(conv)?;
        fs::write(path, raw)?;
        Ok(())
    }

    /// List conversation ids (newest first by mtime when available).
    pub fn list_ids(&self) -> Result<Vec<String>> {
        let mut entries: Vec<_> = fs::read_dir(&self.dir)?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|x| x.to_str())
                    .map(|x| x == "json")
                    .unwrap_or(false)
            })
            .collect();
        entries.sort_by_key(|e| std::cmp::Reverse(e.metadata().and_then(|m| m.modified()).ok()));
        Ok(entries
            .into_iter()
            .filter_map(|e| e.path().file_stem()?.to_str().map(|s| s.to_string()))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_append_reload() {
        let dir = tempdir().unwrap();
        let store = ConversationStore::new(dir.path()).unwrap();
        let conv = store.create().unwrap();
        let id = conv.id.clone();

        store
            .append(
                &id,
                &[
                    ChatMessage::user("hello"),
                    ChatMessage::assistant("hi there"),
                ],
                Some("worker"),
            )
            .unwrap();

        let loaded = store.get(&id).unwrap();
        assert_eq!(loaded.messages.len(), 2);
        assert_eq!(loaded.messages[0].content, "hello");
        assert_eq!(loaded.last_slot.as_deref(), Some("worker"));
    }
}
