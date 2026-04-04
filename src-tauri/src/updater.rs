use tauri::{Emitter, Manager};

/// Menu "Check for Updates…" handler: tell the webview to open
/// the Settings → About panel and start an update check there.
pub fn check_for_update(app: &tauri::AppHandle) {
    emit_check_event(app, true);
}

/// Silent startup check: tell the webview to check for updates
/// without opening any UI.
pub fn check_update_silent(app: &tauri::AppHandle) {
    emit_check_event(app, false);
}

fn emit_check_event(app: &tauri::AppHandle, open_settings: bool) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit(
            "check-for-updates",
            serde_json::json!({ "openSettings": open_settings }),
        );
    }
}
