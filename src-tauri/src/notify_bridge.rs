//! Tray notification bridge with per-slot debounce.

use std::sync::Mutex;

use orchestrator::NotifyDebouncer;
use tauri::{AppHandle, Emitter};
use tauri_plugin_notification::NotificationExt;

/// Holds AppHandle once the Tauri runtime is up; used from the MCP worker thread.
pub struct NotifyBridge {
    app: Mutex<Option<AppHandle>>,
    debouncer: NotifyDebouncer,
}

impl NotifyBridge {
    pub fn new() -> Self {
        Self {
            app: Mutex::new(None),
            debouncer: NotifyDebouncer::with_default_window(),
        }
    }

    pub fn set_app(&self, app: AppHandle) {
        *self.app.lock().unwrap() = Some(app);
    }

    /// Debounced tray notification: "Worker unavailable — slot X: reason"
    pub fn on_worker_unavailable(&self, slot: &str, reason: &str) {
        if !self.debouncer.should_notify(slot) {
            tracing::debug!("suppress tray notify for slot {slot} (debounce)");
            return;
        }
        let app = match self.app.lock().unwrap().clone() {
            Some(a) => a,
            None => {
                tracing::debug!("tray notify skipped (app handle not ready): {slot}: {reason}");
                return;
            }
        };
        let title = format!("Worker unavailable — {slot}");
        let body = format!("{reason}\nOpen Orchestrator to swap the slot backend.");
        if let Err(e) = app
            .notification()
            .builder()
            .title(&title)
            .body(&body)
            .show()
        {
            tracing::warn!("tray notification failed: {e}");
        }
        // Also surface in the UI if the main window is open.
        let _ = app.emit("worker-unavailable", serde_json::json!({ "slot": slot, "reason": reason }));
    }
}

impl Default for NotifyBridge {
    fn default() -> Self {
        Self::new()
    }
}
