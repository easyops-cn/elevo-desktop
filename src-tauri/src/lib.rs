#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

// mod menu;

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{webview::WebviewWindowBuilder, Emitter, Manager, State, WebviewUrl};

/// Managed state that maps each child webview label to its associated roomId.
struct WebviewRoomMap(Mutex<HashMap<String, String>>);

/// Managed state storing the current theme kind ("light" or "dark").
struct CurrentTheme(Mutex<String>);

// Allowed domains for in-app webview (supports subdomain matching).
// Replace with actual trusted domains before shipping.
const ALLOWED_DOMAINS: &[&str] = &[
    "localhost",
];

fn is_domain_allowed(url: &str) -> bool {
    if let Ok(parsed) = url::Url::parse(url) {
        if let Some(host) = parsed.host_str() {
            return ALLOWED_DOMAINS.iter().any(|d| {
                host == *d || host.ends_with(&format!(".{}", d))
            });
        }
    }
    false
}

/// JS injected into every page of a child webview before any page script runs.
/// The script template lives in `scripts/webview-sdk.js` and is embedded at
/// compile time via `include_str!`. The placeholders `__WEBVIEW_LABEL__` and
/// `__ROOM_ID__` are replaced at runtime with their JSON-encoded values.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn sdk_initialization_script(label: &str, room_id: &str, theme: &str) -> String {
    const TEMPLATE: &str = include_str!("../scripts/webview-sdk.js");
    TEMPLATE
        .replace("__WEBVIEW_LABEL__", &serde_json::to_string(label).unwrap())
        .replace("__ROOM_ID__", &serde_json::to_string(room_id).unwrap())
        .replace("__THEME__", &serde_json::to_string(theme).unwrap())
}

// ── Desktop-only commands ────────────────────────────────────────────────────

/// Open a URL in a new in-app WebviewWindow (desktop only).
/// Reuses an existing window with the same label if one already exists.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_webview(
    app: tauri::AppHandle,
    state: State<'_, WebviewRoomMap>,
    theme_state: State<'_, CurrentTheme>,
    url: String,
    label: String,
    room_id: String,
) -> Result<(), String> {
    if !is_domain_allowed(&url) {
        return Err(format!("Domain not in allowlist: {}", url));
    }

    if let Some(existing) = app.get_webview_window(&label) {
        existing.set_focus().map_err(|e: tauri::Error| e.to_string())?;
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let parsed: tauri::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let script = sdk_initialization_script(&label, &room_id, &theme);
    let title = parsed
        .host_str()
        .map(|h| format!("App View: {}", h))
        .unwrap_or_else(|| label.clone());

    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(&title)
        .inner_size(1024.0, 768.0)
        .initialization_script(&script)
        .build()
        .map_err(|e| e.to_string())?;

    // Store label → roomId mapping for later filtering.
    state
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .insert(label.clone(), room_id);

    // Notify main window when this webview is opened.
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit(
            "webview-opened",
            serde_json::json!({ "label": &label }),
        );
    }

    // Notify main window when this webview is closed.
    let label_clone = label.clone();
    let app_clone = app.clone();
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            if let Some(main) = app_clone.get_webview_window("main") {
                let _ = main.emit(
                    "webview-closed",
                    serde_json::json!({ "label": label_clone }),
                );
            }
        }
    });

    Ok(())
}

/// Relay a message from a child webview to the main window via a Tauri event.
/// Event name: "elevo-messenger-sdk-message"
/// Payload: { source: String, roomId: String, channel: String, data: Value }
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn relay_sdk_message(
    app: tauri::AppHandle,
    source_label: String,
    room_id: String,
    channel: String,
    data: serde_json::Value,
) -> Result<(), String> {
    if let Some(main) = app.get_webview_window("main") {
        main.emit(
            "elevo-messenger-sdk-message",
            serde_json::json!({ "source": source_label, "roomId": room_id, "channel": channel, "data": data }),
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Push a message from the main window into a child webview by calling
/// `window.__ElevoMessengerSDK_receive__` via eval.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn send_to_webview(
    app: tauri::AppHandle,
    label: String,
    channel: String,
    data: serde_json::Value,
) -> Result<(), String> {
    if let Some(child) = app.get_webview_window(&label) {
        let js = format!(
            "window.__ElevoMessengerSDK_receive__ && window.__ElevoMessengerSDK_receive__({}, {})",
            serde_json::to_string(&channel).unwrap(),
            serde_json::to_string(&data).unwrap(),
        );
        child.eval(&js).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Broadcast a message from the main window to child webviews that belong to
/// the given roomId, by calling `window.__ElevoMessengerSDK_receive__` via eval
/// on each matching webview.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn send_to_all_webviews(
    app: tauri::AppHandle,
    state: State<'_, WebviewRoomMap>,
    room_id: String,
    channel: String,
    data: serde_json::Value,
) -> Result<(), String> {
    let js = format!(
        "window.__ElevoMessengerSDK_receive__ && window.__ElevoMessengerSDK_receive__({}, {})",
        serde_json::to_string(&channel).unwrap(),
        serde_json::to_string(&data).unwrap(),
    );
    let map = state.0.lock().map_err(|e| e.to_string())?;
    for (label, window) in app.webview_windows() {
        if window.label() == "main" {
            continue;
        }
        if map.get(&label).map(|r| r == &room_id).unwrap_or(false) {
            let _ = window.eval(&js);
        }
    }
    Ok(())
}

/// Update the current theme kind and broadcast a theme_change message to all child webviews.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn set_theme(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    theme_kind: String,
) -> Result<(), String> {
    if theme_kind != "light" && theme_kind != "dark" {
        return Err(format!("Invalid theme kind: {}", theme_kind));
    }
    *theme_state.0.lock().map_err(|e| e.to_string())? = theme_kind.clone();

    let js = format!(
        "window.__ElevoMessengerSDK_receive__ && window.__ElevoMessengerSDK_receive__({}, {})",
        serde_json::to_string("theme_change").unwrap(),
        serde_json::to_string(&theme_kind).unwrap(),
    );
    for (_, window) in app.webview_windows() {
        if window.label() != "main" {
            let _ = window.eval(&js);
        }
    }
    Ok(())
}

/// Close a child webview by label and remove its roomId mapping.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn close_webview(
    app: tauri::AppHandle,
    state: State<'_, WebviewRoomMap>,
    label: String,
) -> Result<(), String> {
    if let Some(w) = app.get_webview_window(&label) {
        w.close().map_err(|e| e.to_string())?;
    }
    state.0.lock().map_err(|e| e.to_string())?.remove(&label);
    Ok(())
}

// ── App entry point ──────────────────────────────────────────────────────────

pub fn run() {
    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();

    builder
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .manage(WebviewRoomMap(Mutex::new(HashMap::new())))
        .manage(CurrentTheme(Mutex::new("light".to_string())))
        .invoke_handler(tauri::generate_handler![
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_webview,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            relay_sdk_message,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            send_to_webview,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            send_to_all_webviews,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            set_theme,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            close_webview,
        ])
        .setup(move |app| {
            // Dev: devUrl from tauri.conf.json (http://localhost:8080) for HMR
            // Release: custom protocol (tauri://localhost) serves bundled frontend
            let window_url = WebviewUrl::App(Default::default());

            let window = WebviewWindowBuilder::new(app, "main".to_string(), window_url)
                .title("Elevo Messenger")
                .build()?;

            // Desktop: intercept close to hide the window instead of quitting;
            // the tray icon lets the user bring it back.
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                let win_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = win_clone.hide();
                    }
                });

                let handle = app.handle().clone();
                tauri::tray::TrayIconBuilder::new()
                    .icon(app.default_window_icon().unwrap().clone())
                    .tooltip("Elevo Messenger")
                    .on_tray_icon_event(move |_tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(win) = handle.get_webview_window("main") {
                                let _ = win.show();
                                let _ = win.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            #[cfg(any(target_os = "android", target_os = "ios"))]
            drop(window);

            Ok(())
        })
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Reopen { .. } = event {
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.show();
                    let _ = win.set_focus();
                }
            }
        });
}
