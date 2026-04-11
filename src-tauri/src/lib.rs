#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod menu;
mod updater;

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{
    webview::{NewWindowResponse, WebviewWindowBuilder},
    Emitter, Manager, State, WebviewUrl,
};
#[cfg(target_os = "macos")]
use tauri::{TitleBarStyle, LogicalPosition};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

/// Managed state that maps each child webview label to its associated roomId.
struct WebviewRoomMap(Mutex<HashMap<String, String>>);

/// Managed state storing the current theme kind ("light" or "dark").
struct CurrentTheme(Mutex<String>);

/// URL scheme used for the OAuth callback redirect URI.
const OAUTH_CALLBACK_SCHEME: &str = "vip.elevo.messenger";

// Allowed domains for in-app webview (supports subdomain matching).
// Replace with actual trusted domains before shipping.
const ALLOWED_DOMAINS: &[&str] = &[
    "localhost",
    "easyops.local",
    "elevo.vip",
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

/// Open a URL in a side panel docked to the right of the main window (desktop only).
/// Adjusts the main window layout (exits fullscreen, resizes, repositions) to make
/// room for the panel, which occupies 1/3 of the screen width.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_side_panel(
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

    let main_window = app
        .get_webview_window("main")
        .ok_or_else(|| "Main window not found".to_string())?;

    // Get monitor info for layout calculations.
    let monitor = main_window
        .current_monitor()
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No monitor found".to_string())?;

    let scale_factor = main_window.scale_factor().map_err(|e| e.to_string())?;
    let monitor_size = monitor.size();
    let monitor_pos = monitor.position();

    let screen_w = monitor_size.width as f64;
    let mon_x = monitor_pos.x as f64;
    let two_thirds_w = (screen_w * 2.0 / 3.0).round();
    let one_third_w = (screen_w / 3.0).round();

    // Exit fullscreen / unmaximize so we can resize and reposition.
    let was_fullscreen = main_window.is_fullscreen().unwrap_or(false);
    if was_fullscreen {
        main_window.set_fullscreen(false).map_err(|e| e.to_string())?;
    }
    let was_maximized = main_window.is_maximized().unwrap_or(false);
    if was_maximized {
        main_window.unmaximize().map_err(|e| e.to_string())?;
    }
    // Allow the window manager to settle after fullscreen/maximize transitions.
    if was_fullscreen || was_maximized {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    // Read geometry after the transition has settled.
    let main_pos = main_window.outer_position().map_err(|e| e.to_string())?;
    let main_size = main_window.outer_size().map_err(|e| e.to_string())?;

    let mut main_w = main_size.width as f64;
    let main_h = main_size.height as f64;
    let mut main_x = main_pos.x as f64;
    let main_y = main_pos.y as f64;

    // Shrink main window if wider than 2/3 screen.
    if main_w > two_thirds_w {
        main_w = two_thirds_w;
        main_window
            .set_size(tauri::PhysicalSize::new(main_w as u32, main_h as u32))
            .map_err(|e| e.to_string())?;
    }

    // Move main window left if not enough room on the right for 1/3 screen.
    let max_main_x = mon_x + screen_w - main_w - one_third_w;
    if main_x > max_main_x {
        main_x = max_main_x;
        main_window
            .set_position(tauri::PhysicalPosition::new(main_x as i32, main_y as i32))
            .map_err(|e| e.to_string())?;
    }

    // Side panel geometry: docked to the right of the main window.
    let panel_x = main_x + main_w;
    let panel_y = main_y;
    let panel_w = one_third_w;
    let panel_h = main_h;

    // If the side panel already exists, reposition/resize and focus it.
    if let Some(existing) = app.get_webview_window(&label) {
        existing
            .set_size(tauri::PhysicalSize::new(panel_w as u32, panel_h as u32))
            .map_err(|e| e.to_string())?;
        existing
            .set_position(tauri::PhysicalPosition::new(panel_x as i32, panel_y as i32))
            .map_err(|e| e.to_string())?;
        existing.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Create the side panel window.
    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let parsed: tauri::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let script = sdk_initialization_script(&label, &room_id, &theme);
    let title = parsed
        .host_str()
        .map(|h| format!("App View: {}", h))
        .unwrap_or_else(|| label.clone());

    // WebviewWindowBuilder takes logical pixels (physical / scale_factor).
    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(&title)
        .inner_size(panel_w / scale_factor, panel_h / scale_factor)
        .position(panel_x / scale_factor, panel_y / scale_factor)
        .initialization_script(&script)
        .build()
        .map_err(|e| e.to_string())?;

    // Override any state restored by tauri-plugin-window-state so the panel
    // always appears at the computed position/size, not the saved one.
    window
        .set_size(tauri::PhysicalSize::new(panel_w as u32, panel_h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(panel_x as i32, panel_y as i32))
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

/// Open a dedicated OAuth/OIDC authentication window (desktop only).
/// The webview navigates to the OIDC provider's authorization URL. When the
/// provider redirects back to the custom-protocol callback URI, the
/// `on_navigation` handler intercepts it, extracts the authorization code (or
/// error), emits a callback event to the main window, and closes the OAuth
/// window. The `label` parameter identifies the window and determines the event
/// names: `{label}--callback` for the OAuth result and `{label}--window-closed`
/// when the user manually closes the window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_oauth_window(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    auth_url: String,
    label: String,
    title: Option<String>,
) -> Result<(), String> {
    let callback_event = format!("{label}--callback");
    let closed_event = format!("{label}--window-closed");
    let event_for_nav = callback_event.clone();
    let event_for_close = closed_event.clone();
    // Close any existing OAuth window with the same label first.
    if let Some(existing) = app.get_webview_window(&label) {
        let _ = existing.close();
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let script = sdk_initialization_script(&label, "", &theme);

    let parsed: tauri::Url = auth_url
        .parse()
        .map_err(|e: url::ParseError| e.to_string())?;

    let app_nav = app.clone();
    let app_event = app.clone();
    let label_for_nav = label.clone();

    // Track whether on_navigation successfully intercepted the callback.
    // This prevents emitting "oauth-window-closed" after a successful redirect,
    // which would cause a race condition with the token exchange.
    let callback_intercepted = Arc::new(AtomicBool::new(false));
    let intercepted_for_close = callback_intercepted.clone();

    let window = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(title.as_deref().unwrap_or("Login"))
        .inner_size(600.0, 700.0)
        .initialization_script(&script)
        .on_navigation(move |url| {
            if url.scheme() != OAUTH_CALLBACK_SCHEME {
                return true; // Allow normal navigation.
            }

            // Mark that we successfully intercepted the callback.
            intercepted_for_close.store(true, Ordering::SeqCst);

            // Collect query parameters from the callback URL.
            let params: HashMap<String, String> = url.query_pairs().into_owned().collect();

            let payload = if let Some(error) = params.get("error") {
                serde_json::json!({
                    "error": error,
                    "errorDescription": params.get("error_description").cloned().unwrap_or_default(),
                })
            } else if let (Some(code), Some(state)) = (params.get("code"), params.get("state")) {
                serde_json::json!({
                    "code": code,
                    "state": state,
                })
            } else {
                serde_json::json!({
                    "error": "invalid_response",
                    "errorDescription": "Missing code or state parameter in callback.",
                })
            };

            // Emit the callback data to the main window.
            if let Some(main) = app_nav.get_webview_window("main") {
                let _ = main.emit(&event_for_nav, payload);
            }

            // Close the OAuth window asynchronously to avoid deadlock.
            let close_handle = app_nav.clone();
            let label_for_close = label_for_nav.clone();
            tauri::async_runtime::spawn(async move {
                if let Some(win) = close_handle.get_webview_window(&label_for_close) {
                    let _ = win.close();
                }
            });

            false // Block navigation to the custom-protocol URL.
        })
        .build()
        .map_err(|e| e.to_string())?;

    // Notify main window when the OAuth window is closed (e.g. user closes it manually).
    // Skip if the callback was already intercepted, to avoid a race with token exchange.
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::Destroyed = event {
            if callback_intercepted.load(Ordering::SeqCst) {
                return;
            }
            if let Some(main) = app_event.get_webview_window("main") {
                let _ = main.emit(&event_for_close, serde_json::json!({}));
            }
        }
    });

    Ok(())
}

// ── App entry point ──────────────────────────────────────────────────────────

pub fn run() {
    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();

    builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // When a second instance is launched, show and focus the existing window.
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(WebviewRoomMap(Mutex::new(HashMap::new())))
        .manage(CurrentTheme(Mutex::new("light".to_string())))
        .invoke_handler(tauri::generate_handler![
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_webview,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_side_panel,
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
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_oauth_window,
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

            // Build and set application menu (macOS only).
            // Windows/Linux use decorations(false) so no native menu is needed.
            #[cfg(target_os = "macos")]
            {
                let m = menu::build_menu(app)?;
                app.set_menu(m)?;

                let handle = app.handle().clone();
                app.on_menu_event(move |_app, event| {
                    if event.id().as_ref() == menu::CHECK_FOR_UPDATES_ID {
                        updater::check_for_update(&handle);
                    }
                });
            }

            // Dev: devUrl from tauri.conf.json (http://localhost:8080) for HMR
            // Release: custom protocol (tauri://localhost) serves bundled frontend
            let window_url = WebviewUrl::App(Default::default());

            let app_handle = app.handle().clone();
            let builder = WebviewWindowBuilder::new(app, "main".to_string(), window_url)
                .title("Elevo Messenger")
                .on_navigation(|url| {
                    // Dev:  http://localhost:8080
                    // Prod: tauri://localhost
                    if cfg!(debug_assertions) {
                        url.scheme() == "http" && url.host_str() == Some("localhost") && url.port() == Some(8080)
                    } else {
                        url.scheme() == "tauri"
                    }
                })
                .on_new_window(move |url, _features| {
                    let _ = app_handle.opener().open_url(url.as_str(), None::<&str>);
                    NewWindowResponse::Deny
                });

            // macOS: overlay titlebar keeps native traffic lights, hides title text
            #[cfg(target_os = "macos")]
            let builder = builder
                .title_bar_style(TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(LogicalPosition::new(12.0, 20.0));

            // Windows/Linux: remove native decorations entirely
            #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
            let builder = builder.decorations(false);

            let window = builder.build()?;

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

                // macOS: use a monochrome template icon so the system auto-adapts
                // to light/dark menu bar. Non-macOS: use the default (colored) icon.
                #[cfg(target_os = "macos")]
                let tray_icon = tauri::image::Image::from_bytes(
                    include_bytes!("../icons/tray_icon.png"),
                )
                .expect("failed to load tray icon");

                #[cfg(not(target_os = "macos"))]
                let tray_icon = app.default_window_icon().unwrap().clone();

                // Build tray context menu (right-click).
                let tray_menu = tauri::menu::MenuBuilder::new(app)
                    .item(
                        &tauri::menu::MenuItem::with_id(app, "open", "Open Elevo Messenger", true, None::<&str>)?,
                    )
                    .separator()
                    .item(
                        &tauri::menu::MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
                    )
                    .build()?;

                let handle = app.handle().clone();
                let handle_menu = app.handle().clone();
                let mut tray_builder = tauri::tray::TrayIconBuilder::new()
                    .icon(tray_icon)
                    .tooltip("Elevo Messenger")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false);

                #[cfg(target_os = "macos")]
                {
                    tray_builder = tray_builder.icon_as_template(true);
                }

                tray_builder
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
                    .on_menu_event(move |_app, event| {
                        match event.id().as_ref() {
                            "open" => {
                                if let Some(win) = handle_menu.get_webview_window("main") {
                                    let _ = win.show();
                                    let _ = win.set_focus();
                                }
                            }
                            "quit" => {
                                handle_menu.exit(0);
                            }
                            _ => {}
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
