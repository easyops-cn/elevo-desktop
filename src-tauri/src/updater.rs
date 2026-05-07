use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};

/// Whether the menu should trigger "apply-update" (true) or "check-for-updates" (false).
static APPLY_MODE: AtomicBool = AtomicBool::new(false);

pub fn set_apply_mode(apply: bool) {
    APPLY_MODE.store(apply, Ordering::Relaxed);
}

/// Menu click handler: emit the appropriate event based on current mode.
pub fn check_for_update(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let event_name = if APPLY_MODE.load(Ordering::Relaxed) {
            "apply-update"
        } else {
            "check-for-updates"
        };
        let _ = main.emit(
            event_name,
            serde_json::json!({ "openSettings": true }),
        );
    }
}
