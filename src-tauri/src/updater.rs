use tauri::{Emitter, Manager};

/// Menu "Check for Updates…" handler: tell the webview to open
/// the Settings → About panel and start an update check there.
pub fn check_for_update(app: &tauri::AppHandle) {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit(
            "check-for-updates",
            serde_json::json!({ "openSettings": true }),
        );
    }
}
