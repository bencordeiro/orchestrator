//! Debounce helper for tray notifications (max one per slot per window).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Default: one notification per slot per minute.
pub const DEFAULT_WINDOW: Duration = Duration::from_secs(60);

/// Returns whether a notification for `slot` should fire now.
#[derive(Debug, Default)]
pub struct NotifyDebouncer {
    window: Duration,
    last: Mutex<HashMap<String, Instant>>,
}

impl NotifyDebouncer {
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            last: Mutex::new(HashMap::new()),
        }
    }

    pub fn with_default_window() -> Self {
        Self::new(DEFAULT_WINDOW)
    }

    /// `true` if this is the first notice for the slot in the window (caller should notify).
    pub fn should_notify(&self, slot: &str) -> bool {
        let now = Instant::now();
        let mut map = self.last.lock().unwrap();
        if let Some(prev) = map.get(slot) {
            if now.duration_since(*prev) < self.window {
                return false;
            }
        }
        map.insert(slot.to_string(), now);
        true
    }

    /// Test helper: force a prior timestamp.
    #[cfg(test)]
    pub fn force_last(&self, slot: &str, when: Instant) {
        self.last.lock().unwrap().insert(slot.to_string(), when);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debounce_blocks_second_within_window() {
        let d = NotifyDebouncer::new(Duration::from_secs(60));
        assert!(d.should_notify("worker"));
        assert!(!d.should_notify("worker"));
        // Different slot still allowed.
        assert!(d.should_notify("reviewer"));
    }

    #[test]
    fn allows_after_window() {
        let d = NotifyDebouncer::new(Duration::from_millis(50));
        assert!(d.should_notify("worker"));
        d.force_last("worker", Instant::now() - Duration::from_millis(100));
        assert!(d.should_notify("worker"));
    }
}
