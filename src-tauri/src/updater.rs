use std::path::PathBuf;

use serde_json;
use tauri::Manager;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

fn ignored_versions_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_data_dir()
        .expect("failed to resolve app data dir")
        .join("ignored_updates.json")
}

fn load_ignored_versions(app: &tauri::AppHandle) -> Vec<String> {
    let path = ignored_versions_path(app);
    if let Ok(data) = std::fs::read_to_string(&path) {
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn save_ignored_versions(app: &tauri::AppHandle, versions: &[String]) {
    let path = ignored_versions_path(app);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string(versions) {
        let _ = std::fs::write(&path, data);
    }
}

pub async fn check_for_update(app: tauri::AppHandle, silent: bool) {
    if let Err(e) = do_check_for_update(app, silent).await {
        eprintln!("updater error: {e}");
    }
}

async fn do_check_for_update(
    app: tauri::AppHandle,
    silent: bool,
) -> tauri_plugin_updater::Result<()> {
    let update = app.updater()?.check().await?;

    let Some(update) = update else {
        if !silent {
            let app_clone = app.clone();
            let _ = app_clone
                .dialog()
                .message("You're running the latest version.")
                .title("No Updates Available")
                .kind(MessageDialogKind::Info)
                .blocking_show();
        }
        return Ok(());
    };

    let version = update.version.clone();
    let body = update.body.clone().unwrap_or_default();

    // In silent mode, skip if the user has ignored this version.
    if silent {
        let ignored = load_ignored_versions(&app);
        if ignored.contains(&version) {
            return Ok(());
        }
    }

    // Ask user whether to update.
    let message = if body.is_empty() {
        format!("A new version ({version}) is available. Would you like to update now?")
    } else {
        format!("A new version ({version}) is available.\n\n{body}\n\nWould you like to update now?")
    };

    let confirmed = app
        .dialog()
        .message(&message)
        .title("Update Available")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::YesNo)
        .blocking_show();

    if confirmed {
        // User chose to update — download and install.
        update
            .download_and_install(
                |_chunk_length, _content_length| {},
                || {},
            )
            .await?;
        app.restart();
    } else {
        // Ask if they want to ignore this version.
        let ignore = app
            .dialog()
            .message(&format!(
                "Would you like to skip version {version}?\n\nYou won't be reminded about this version again."
            ))
            .title("Skip This Version?")
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::YesNo)
            .blocking_show();

        if ignore {
            let mut ignored = load_ignored_versions(&app);
            if !ignored.contains(&version) {
                ignored.push(version);
                save_ignored_versions(&app, &ignored);
            }
        }
    }

    Ok(())
}
