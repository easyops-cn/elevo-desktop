#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod menu;
mod updater;

use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{webview::WebviewWindowBuilder, Emitter, Manager, State, WebviewUrl};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_deep_link::DeepLinkExt;

/// Returns a platform-specific User-Agent string for the application.
fn platform_user_agent() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let (os, os_version) = if cfg!(target_os = "macos") {
        ("macOS", macos_version())
    } else if cfg!(target_os = "windows") {
        ("Windows", windows_version())
    } else {
        ("Linux", linux_version())
    };
    let device_model = device_model();
    format!(
        "ElevoMessenger/{} ({}; {}; {})",
        version, os, os_version, device_model
    )
}

fn macos_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn windows_version() -> String {
    std::process::Command::new("cmd")
        .args(["/C", "ver"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| {
            // Output like: "Microsoft Windows [Version 10.0.19045.3803]"
            s.trim()
                .strip_prefix("Microsoft Windows [Version ")
                .and_then(|v| v.strip_suffix(']'))
                .unwrap_or("Unknown")
                .to_string()
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn linux_version() -> String {
    std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|content| {
            content
                .lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .and_then(|l| l.strip_prefix("PRETTY_NAME="))
                .map(|v| v.trim_matches('"').to_string())
        })
        .unwrap_or_else(|| "Unknown".to_string())
}

fn device_model() -> String {
    if cfg!(target_os = "macos") {
        std::process::Command::new("sysctl")
            .args(["-n", "hw.model"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("wmic")
            .args(["csproduct", "get", "name"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| {
                // Output has header "Name\n<model>\n"
                s.lines()
                    .nth(1)
                    .map(|l| l.trim().to_string())
                    .unwrap_or_default()
            })
            .filter(|s| !s.is_empty())
    } else {
        std::fs::read_to_string("/sys/class/dmi/id/product_name")
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }
    .unwrap_or_else(|| "Unknown".to_string())
}

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
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
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
            // Initialize updater plugin (desktop only).
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            // Register deep link schemes and listen for incoming deep links.
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            {
                // On Linux/Windows, register schemes at runtime so they work in dev mode.
                #[cfg(any(target_os = "linux", windows))]
                app.deep_link().register_all()?;

                // Emit a generic event for every incoming deep link URL so that
                // any frontend feature (SSO, room links, invites, etc.) can handle it.
                let handle = app.handle().clone();
                app.deep_link().on_open_url(move |event| {
                    let main = handle.get_webview_window("main");
                    for url in event.urls() {
                        if let Some(win) = &main {
                            let _ = win.emit("deep-link-received", url.as_str());
                        }
                    }
                });
            }

            // Build and set application menu (desktop only).
            #[cfg(desktop)]
            {
                let m = menu::build_menu(app)?;
                app.set_menu(m)?;

                let handle = app.handle().clone();
                app.on_menu_event(move |_app, event| {
                    if event.id().as_ref() == menu::CHECK_FOR_UPDATES_ID {
                        let h = handle.clone();
                        tauri::async_runtime::spawn(async move {
                            updater::check_for_update(h, false).await;
                        });
                    }
                });
            }

            // Auto-check for updates after a short delay (desktop only, silent).
            #[cfg(desktop)]
            {
                let handle_for_update = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    tauri::async_runtime::spawn(async move {
                        updater::check_for_update(handle_for_update, true).await;
                    });
                });
            }

            // Dev: devUrl from tauri.conf.json (http://localhost:8080) for HMR
            // Release: custom protocol (tauri://localhost) serves bundled frontend
            let window_url = WebviewUrl::App(Default::default());

            let window = WebviewWindowBuilder::new(app, "main".to_string(), window_url)
                .title("Elevo Messenger")
                .user_agent(&platform_user_agent())
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
            #[cfg(target_os = "macos")]
            {
                if let tauri::RunEvent::Reopen { .. } = event {
                    if let Some(win) = app.get_webview_window("main") {
                        let _ = win.show();
                        let _ = win.set_focus();
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
