use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

pub async fn check_for_update(app: tauri::AppHandle) {
    if let Err(e) = do_check_for_update(&app).await {
        eprintln!("updater error: {e}");
        let _ = app
            .dialog()
            .message(&format!("Failed to check for updates: {e}"))
            .title("Update Check Failed")
            .kind(MessageDialogKind::Error)
            .blocking_show();
    }
}

/// Silent startup check: emits an event to the webview instead of showing dialogs.
/// Called on app launch — no UI feedback, no download.
pub async fn check_update_silent(app: tauri::AppHandle) {
    if let Err(e) = do_silent_check(&app).await {
        eprintln!("silent updater check error: {e}");
    }
}

async fn do_silent_check(app: &tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    let update = app.updater()?.check().await?;

    let Some(update) = update else {
        return Ok(());
    };

    let version = update.version.clone();
    let body = update.body.clone().unwrap_or_default();
    let date = update.date.as_ref().map(|d| d.to_string());
    let current_version = app
        .package_info()
        .version
        .to_string();

    // Emit event to the main webview so the JS frontend can handle it.
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit(
            "update-available",
            serde_json::json!({
                "version": version,
                "body": body,
                "date": date,
                "currentVersion": current_version,
            }),
        );
    }

    Ok(())
}

async fn do_check_for_update(app: &tauri::AppHandle) -> tauri_plugin_updater::Result<()> {
    let update = app.updater()?.check().await?;

    let Some(update) = update else {
        let _ = app
            .dialog()
            .message("You're running the latest version.")
            .title("No Updates Available")
            .kind(MessageDialogKind::Info)
            .blocking_show();
        return Ok(());
    };

    let version = update.version.clone();
    let body = update.body.clone().unwrap_or_default();

    // Notify the user that an update is being downloaded.
    let message = if body.is_empty() {
        format!("A new version ({version}) is available. Downloading...")
    } else {
        format!("A new version ({version}) is available.\n\n{body}\n\nDownloading...")
    };

    let _ = app
        .dialog()
        .message(&message)
        .title("Update Available")
        .kind(MessageDialogKind::Info)
        .blocking_show();

    // On Windows, `install` exits the application due to installer limitations,
    // so we only download first, then let the user decide whether to install.
    #[cfg(target_os = "windows")]
    {
        update
            .download(
                |_chunk_length, _content_length| {},
                || {},
            )
            .await?;

        let install_now = app
            .dialog()
            .message(
                "Update downloaded successfully.\n\nInstall and restart now to apply the update?",
            )
            .title("Update Ready")
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show();

        if install_now {
            app.restart();
        }
    }

    // On non-Windows, download + install together, then ask about restarting.
    #[cfg(not(target_os = "windows"))]
    {
        update
            .download_and_install(
                |_chunk_length, _content_length| {},
                || {},
            )
            .await?;

        let restart_now = app
            .dialog()
            .message(
                "Update downloaded and installed successfully.\n\nRestart now to apply the update?",
            )
            .title("Update Complete")
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show();

        if restart_now {
            app.restart();
        }
    }

    Ok(())
}
