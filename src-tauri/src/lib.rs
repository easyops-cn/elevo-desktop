#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod menu;
mod updater;

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{
    webview::{NewWindowResponse, Webview, WebviewBuilder, WebviewWindowBuilder},
    window::WindowBuilder,
    Emitter, Manager, State, WebviewUrl,
};
#[cfg(target_os = "macos")]
use tauri::{TitleBarStyle, LogicalPosition};
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use tauri_plugin_deep_link::DeepLinkExt;
use tauri_plugin_opener::OpenerExt;

/// Managed state that maps each child webview label to its associated roomId.
struct WebviewRoomMap(Arc<Mutex<HashMap<String, String>>>);

/// Managed state storing the current theme kind ("light" or "dark").
struct CurrentTheme(Mutex<String>);

/// Managed state holding the tray icon handle for dynamic updates.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
struct TrayState(Mutex<Option<tauri::tray::TrayIcon>>);

/// URL scheme used for the OAuth callback redirect URI.
const OAUTH_CALLBACK_SCHEME: &str = "vip.elevo.messenger";

// Allowed domains for in-app webview (supports subdomain matching).
// Replace with actual trusted domains before shipping.
const ALLOWED_DOMAINS: &[&str] = &[
    "localhost",
    "easyops.local",
    "elevo.vip",
];

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const EXTERNAL_WEBVIEW_DATA_DIR: &str = "external-webviews";

#[cfg(not(any(target_os = "android", target_os = "ios")))]
const SIDE_PANEL_TITLEBAR_HEIGHT: f64 = 40.0;

#[cfg(all(not(any(target_os = "android", target_os = "ios")), target_os = "macos"))]
const EXTERNAL_WEBVIEW_DATA_STORE_ID: [u8; 16] = *b"elevoextwebview1";

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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn external_webview_data_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join(EXTERNAL_WEBVIEW_DATA_DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Derive a window title from a URL: show only the host for standard HTTPS,
/// otherwise show the full origin (scheme://host:port).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn title_from_url(url: &url::Url, fallback: &str) -> String {
    if url.scheme() == "https" && url.port().is_none() {
        url.host_str().unwrap_or(fallback).to_string()
    } else {
        url.origin().ascii_serialization()
    }
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

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn preview_initialization_script(theme: &str, payload: &serde_json::Value) -> String {
    format!(
        r#"(function () {{
  window.__ElevoPreview_initialTheme__ = {};
  window.__ElevoPreview_initialPayload__ = {};
}})();"#,
        serde_json::to_string(theme).unwrap(),
        serde_json::to_string(payload).unwrap()
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn code_view_initialization_script(theme: &str, payload: &serde_json::Value) -> String {
    format!(
        r#"(function () {{
  window.__ElevoCodeView_initialTheme__ = {};
  window.__ElevoCodeView_initialPayload__ = {};
}})();"#,
        serde_json::to_string(theme).unwrap(),
        serde_json::to_string(payload).unwrap()
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn bridge_explorer_initialization_script(theme: &str, payload: &serde_json::Value) -> String {
    format!(
        r#"(function () {{
  window.__ElevoBridgeExplorer_initialTheme__ = {};
  window.__ElevoBridgeExplorer_initialPayload__ = {};
}})();"#,
        serde_json::to_string(theme).unwrap(),
        serde_json::to_string(payload).unwrap()
    )
}

/// Build a stable, filesystem/label-safe window label for a workspace explorer.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn bridge_explorer_label(workspace_id: &str) -> String {
    let safe: String = workspace_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    format!("bridge-explorer-{}", safe)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn task_board_initialization_script(theme: &str, payload: &serde_json::Value) -> String {
    format!(
        r#"(function () {{
  window.__ElevoTaskBoard_initialTheme__ = {};
  window.__ElevoTaskBoard_initialPayload__ = {};
}})();"#,
        serde_json::to_string(theme).unwrap(),
        serde_json::to_string(payload).unwrap()
    )
}

/// Build a stable, filesystem/label-safe window label for a task board.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn task_board_label(workspace_id: &str) -> String {
    let safe: String = workspace_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    format!("task-board-{}", safe)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn side_panel_titlebar_label(label: &str) -> String {
    format!("{}--titlebar", label)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn side_panel_content_label(label: &str) -> String {
    format!("{}--content", label)
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn toggle_webview_devtools(webview: &Webview<tauri::Wry>) {
    if webview.is_devtools_open() {
        webview.close_devtools();
    } else {
        webview.open_devtools();
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn toggle_focused_devtools(app: &tauri::AppHandle) {
    for (label, window) in app.windows() {
        if window.is_focused().unwrap_or(false) {
            if let Some(content) = app.get_webview(&side_panel_content_label(&label)) {
                toggle_webview_devtools(&content);
                return;
            }
        }
    }

    for (label, webview_window) in app.webview_windows() {
        if webview_window.is_focused().unwrap_or(false) {
            if let Some(content) = app.get_webview(&side_panel_content_label(&label)) {
                toggle_webview_devtools(&content);
            } else {
                toggle_webview_devtools(webview_window.as_ref());
            }
            break;
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn titlebar_initialization_script(label: &str, title: &str, url: &str) -> String {
    let state = serde_json::json!({
        "label": label,
        "title": title,
        "url": url,
        "canGoBack": false,
        "canGoForward": false,
    });
    format!(
        r#"(function () {{
  window.__ElevoWebviewTitlebar_initialState__ = {};
}})();"#,
        state
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn history_initialization_script(label: &str) -> String {
    format!(
        r#"(function () {{
  if (window.__ElevoWebviewHistoryInstalled__) return;
  window.__ElevoWebviewHistoryInstalled__ = true;
  const LABEL = {};
  const tauriInternals = window.__TAURI_INTERNALS__;
  const tauriInvoke = (cmd, payload) => {{
    if (!tauriInternals) return;
    const callback = tauriInternals.transformCallback(function () {{}}, true);
    const error = tauriInternals.transformCallback(function () {{}}, true);
    tauriInternals.postMessage({{ cmd, callback, error, payload }});
  }};
  const currentTitle = () => {{
    try {{
      return window.location.protocol === "https:" && !window.location.port
        ? window.location.hostname
        : window.location.origin;
    }} catch (_) {{
      return "";
    }}
  }};
  const stateKey = "__elevoWebviewHistoryIndex";
  let index = Number(history.state && history.state[stateKey]);
  if (!Number.isFinite(index)) {{
    index = Number(sessionStorage.getItem("__elevo_history_index__") || "0");
    try {{
      history.replaceState(Object.assign({{}}, history.state, {{ [stateKey]: index }}), "", location.href);
    }} catch (_) {{}}
  }}
  let maxIndex = Number(sessionStorage.getItem("__elevo_history_max_index__") || String(index));
  maxIndex = Math.max(maxIndex, index);
  const report = () => {{
    try {{
      tauriInvoke("webview_titlebar_update_state", {{
        label: LABEL,
        title: currentTitle(),
        url: window.location.href,
        canGoBack: index > 0,
        canGoForward: index < maxIndex
      }});
    }} catch (_) {{}}
  }};
  const persist = () => {{
    sessionStorage.setItem("__elevo_history_index__", String(index));
    sessionStorage.setItem("__elevo_history_max_index__", String(maxIndex));
  }};
  const push = () => {{
    index += 1;
    maxIndex = index;
    try {{
      history.replaceState(Object.assign({{}}, history.state, {{ [stateKey]: index }}), "", location.href);
    }} catch (_) {{}}
    persist();
    window.setTimeout(report, 0);
  }};
  const originalPushState = history.pushState;
  const originalReplaceState = history.replaceState;
  history.pushState = function () {{
    const result = originalPushState.apply(this, arguments);
    push();
    return result;
  }};
  history.replaceState = function () {{
    if (arguments.length > 0) {{
      arguments[0] = Object.assign({{}}, arguments[0], {{ [stateKey]: index }});
    }}
    const result = originalReplaceState.apply(this, arguments);
    window.setTimeout(report, 0);
    return result;
  }};
  window.addEventListener("popstate", (event) => {{
    const nextIndex = Number(event.state && event.state[stateKey]);
    index = Number.isFinite(nextIndex) ? nextIndex : Math.max(0, index - 1);
    maxIndex = Math.max(maxIndex, index);
    persist();
    window.setTimeout(report, 0);
  }});
  window.addEventListener("pageshow", report);
  window.setTimeout(report, 0);
}})();"#,
        serde_json::to_string(label).unwrap()
    )
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn emit_webview_titlebar_state(
    app: &tauri::AppHandle,
    label: &str,
    title: &str,
    url: &str,
    can_go_back: bool,
    can_go_forward: bool,
) {
    let state = serde_json::json!({
        "label": label,
        "title": title,
        "url": url,
        "canGoBack": can_go_back,
        "canGoForward": can_go_forward,
    });
    if let Some(titlebar) = app.get_webview(&side_panel_titlebar_label(label)) {
        let js = format!(
            "window.__ElevoWebviewTitlebar_receive__ && window.__ElevoWebviewTitlebar_receive__({})",
            state
        );
        let _ = titlebar.eval(js);
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("webview-titlebar-state", state);
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn resize_side_panel_children(app: &tauri::AppHandle, label: &str, width: f64, height: f64) {
    if let Some(titlebar) = app.get_webview(&side_panel_titlebar_label(label)) {
        let _ = titlebar.set_position(tauri::LogicalPosition::new(0.0, 0.0));
        let _ = titlebar.set_size(tauri::LogicalSize::new(width, SIDE_PANEL_TITLEBAR_HEIGHT));
    }
    if let Some(content) = app.get_webview(&side_panel_content_label(label)) {
        let content_h = (height - SIDE_PANEL_TITLEBAR_HEIGHT).max(1.0);
        let _ = content.set_position(tauri::LogicalPosition::new(0.0, SIDE_PANEL_TITLEBAR_HEIGHT));
        let _ = content.set_size(tauri::LogicalSize::new(width, content_h));
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn activate_window(window: &tauri::WebviewWindow) -> Result<(), tauri::Error> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn activate_window_lossy(window: &tauri::WebviewWindow) {
    let _ = window.show();
    let _ = window.unminimize();
    let _ = window.set_focus();
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn activate_plain_window(window: &tauri::Window) -> Result<(), tauri::Error> {
    window.show()?;
    window.unminimize()?;
    window.set_focus()?;
    Ok(())
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
        activate_window(&existing).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let parsed: tauri::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;
    let script = sdk_initialization_script(&label, &room_id, &theme);
    let title = title_from_url(&parsed, &label);

    let app_for_load = app.clone();
    let label_for_load = label.clone();

    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(&title)
        .inner_size(1024.0, 768.0)
        .data_directory(external_webview_data_dir(&app)?)
        .initialization_script(&script)
        .on_page_load(move |_webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                let new_title = title_from_url(payload.url(), &label_for_load);
                if let Some(win) = app_for_load.get_webview_window(&label_for_load) {
                    let _ = win.set_title(&new_title);
                }
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(EXTERNAL_WEBVIEW_DATA_STORE_ID);

    let window = builder.build().map_err(|e| e.to_string())?;

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

/// Open or update the singleton media preview window (desktop only).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_preview_window(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    payload: serde_json::Value,
) -> Result<(), String> {
    const LABEL: &str = "media-preview";

    if let Some(existing) = app.get_webview_window(LABEL) {
        let js = format!(
            "window.__ElevoPreview_receive__ && window.__ElevoPreview_receive__({})",
            serde_json::to_string(&payload).map_err(|e| e.to_string())?,
        );
        existing.eval(&js).map_err(|e| e.to_string())?;
        activate_window(&existing).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let script = preview_initialization_script(&theme, &payload);

    WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::App(PathBuf::from("preview.html")),
    )
    .title("Media Preview")
    .inner_size(960.0, 720.0)
    .min_inner_size(420.0, 320.0)
    .initialization_script(&script)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Open or update the singleton code view window (desktop only).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_code_view_window(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    payload: serde_json::Value,
) -> Result<(), String> {
    const LABEL: &str = "code-view";

    if let Some(existing) = app.get_webview_window(LABEL) {
        let js = format!(
            "window.__ElevoCodeView_receive__ && window.__ElevoCodeView_receive__({})",
            serde_json::to_string(&payload).map_err(|e| e.to_string())?,
        );
        existing.eval(&js).map_err(|e| e.to_string())?;
        activate_window(&existing).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let script = code_view_initialization_script(&theme, &payload);

    WebviewWindowBuilder::new(
        &app,
        LABEL,
        WebviewUrl::App(PathBuf::from("code-view.html")),
    )
    .title("Code View")
    .inner_size(1200.0, 760.0)
    .min_inner_size(520.0, 360.0)
    .initialization_script(&script)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Payload for opening a bridge workspace explorer window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct BridgeExplorerPayload {
    workspace_id: String,
    workspace_name: String,
    /// Bridge provider segment, verbatim from room state (already includes `-bridge`).
    bridge_provider: String,
    matrix_token: String,
    homeserver_url: String,
    /// Optional file path to select on open / push to an already-open window.
    #[serde(default)]
    initial_file_path: Option<String>,
}

/// Open (or focus) a read-only file explorer window for a bridge-provider
/// workspace (desktop only). Each workspace gets its own window keyed by a
/// stable label, so reopening the same workspace focuses the existing window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_bridge_explorer_window(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    payload: BridgeExplorerPayload,
) -> Result<(), String> {
    let label = bridge_explorer_label(&payload.workspace_id);

    if let Some(existing) = app.get_webview_window(&label) {
        activate_window(&existing).map_err(|e| e.to_string())?;
        // Push the requested file selection to the already-open window so it
        // switches to the file instead of merely focusing.
        if let Some(path) = payload.initial_file_path.as_ref().filter(|p| !p.is_empty()) {
            let js = format!(
                "window.__ElevoMessengerSDK_receive__ && window.__ElevoMessengerSDK_receive__({}, {})",
                serde_json::to_string("bridge-explorer-select-file").unwrap(),
                serde_json::to_string(path).unwrap(),
            );
            let _ = existing.eval(&js);
        }
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let payload_json = serde_json::to_value(&payload).map_err(|e| e.to_string())?;
    let script = bridge_explorer_initialization_script(&theme, &payload_json);

    WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App(PathBuf::from("bridge-explorer.html")),
    )
    .title("Workspace Explorer")
    .inner_size(1100.0, 740.0)
    .min_inner_size(520.0, 360.0)
    .initialization_script(&script)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Payload for opening a task board window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskBoardPayload {
    workspace_id: String,
    workspace_name: String,
    /// Bridge provider segment, verbatim from room state (already includes `-bridge`).
    bridge_provider: String,
    matrix_token: String,
    homeserver_url: String,
}

/// Open (or focus) a read-only task board window for a bridge-provider
/// workspace (desktop only). Each workspace gets its own window keyed by a
/// stable label, so reopening the same workspace focuses the existing window.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn open_task_board_window(
    app: tauri::AppHandle,
    theme_state: State<'_, CurrentTheme>,
    payload: TaskBoardPayload,
) -> Result<(), String> {
    let label = task_board_label(&payload.workspace_id);

    if let Some(existing) = app.get_webview_window(&label) {
        activate_window(&existing).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let payload_json = serde_json::to_value(&payload).map_err(|e| e.to_string())?;
    let script = task_board_initialization_script(&theme, &payload_json);

    WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App(PathBuf::from("task-board.html")),
    )
    .title("Task Board")
    .inner_size(1100.0, 740.0)
    .min_inner_size(520.0, 360.0)
    .initialization_script(&script)
    .build()
    .map_err(|e| e.to_string())?;

    Ok(())
}

/// Open a URL in a side panel docked to the right of the main window (desktop only).
/// Adjusts the main window layout (exits fullscreen, resizes, repositions) to make
/// room for the panel, which occupies 1/2 of the screen width.
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
    let work_area = monitor.work_area();
    let was_fullscreen = main_window.is_fullscreen().unwrap_or(false);
    let was_maximized = main_window.is_maximized().unwrap_or(false);

    let screen_w = if was_maximized {
        work_area.size.width as f64
    } else {
        monitor_size.width as f64
    };
    let mon_x = if was_maximized {
        work_area.position.x as f64
    } else {
        monitor_pos.x as f64
    };
    let half_w = (screen_w / 2.0).round();

    // Exit fullscreen / unmaximize so we can resize and reposition.
    if was_fullscreen {
        main_window.set_fullscreen(false).map_err(|e| e.to_string())?;
    }
    if was_maximized {
        main_window.unmaximize().map_err(|e| e.to_string())?;
        main_window
            .set_position(tauri::PhysicalPosition::new(
                work_area.position.x,
                work_area.position.y,
            ))
            .map_err(|e| e.to_string())?;
        main_window
            .set_size(tauri::PhysicalSize::new(
                half_w as u32,
                work_area.size.height,
            ))
            .map_err(|e| e.to_string())?;
    }
    // Allow the window manager to settle after fullscreen transitions.
    if was_fullscreen {
        std::thread::sleep(std::time::Duration::from_millis(300));
    }

    // Read geometry after the transition has settled.
    let main_pos = main_window.outer_position().map_err(|e| e.to_string())?;
    let main_size = main_window.outer_size().map_err(|e| e.to_string())?;

    let mut main_w = if was_maximized {
        half_w
    } else {
        main_size.width as f64
    };
    let main_h = if was_maximized {
        work_area.size.height as f64
    } else {
        main_size.height as f64
    };
    let mut main_x = if was_maximized {
        work_area.position.x as f64
    } else {
        main_pos.x as f64
    };
    let main_y = if was_maximized {
        work_area.position.y as f64
    } else {
        main_pos.y as f64
    };

    // Shrink main window if wider than 1/2 screen.
    if main_w > half_w || was_maximized {
        main_w = half_w;
        main_window
            .set_size(tauri::PhysicalSize::new(main_w as u32, main_h as u32))
            .map_err(|e| e.to_string())?;
    }

    // Move main window left if not enough room on the right for 1/2 screen.
    let max_main_x = mon_x + screen_w - main_w - half_w;
    if main_x > max_main_x {
        main_x = max_main_x;
        main_window
            .set_position(tauri::PhysicalPosition::new(main_x as i32, main_y as i32))
            .map_err(|e| e.to_string())?;
    }

    // Side panel geometry: docked to the right of the main window.
    let panel_x = main_x + main_w;
    let panel_y = main_y;
    let panel_w = half_w;
    let panel_h = main_h;

    let panel_w_logical = panel_w / scale_factor;
    let panel_h_logical = panel_h / scale_factor;

    let parsed: tauri::Url = url.parse().map_err(|e: url::ParseError| e.to_string())?;

    // If the side panel already exists, reposition/resize children, navigate it,
    // and focus it. Re-navigation matters for workspace deep links such as
    // `?file=...` when the user picks a different file from code view.
    if let Some(existing) = app.get_window(&label) {
        existing
            .set_size(tauri::PhysicalSize::new(panel_w as u32, panel_h as u32))
            .map_err(|e| e.to_string())?;
        existing
            .set_position(tauri::PhysicalPosition::new(panel_x as i32, panel_y as i32))
            .map_err(|e| e.to_string())?;
        resize_side_panel_children(&app, &label, panel_w_logical, panel_h_logical);
        let title = title_from_url(&parsed, &label);
        existing.set_title(&title).map_err(|e| e.to_string())?;
        emit_webview_titlebar_state(&app, &label, &title, parsed.as_str(), false, false);
        if let Some(content) = app.get_webview(&side_panel_content_label(&label)) {
            let js = format!(
                "if (window.location.href !== {}) window.location.assign({});",
                serde_json::to_string(parsed.as_str()).map_err(|e| e.to_string())?,
                serde_json::to_string(parsed.as_str()).map_err(|e| e.to_string())?,
            );
            content.eval(&js).map_err(|e| e.to_string())?;
        }
        activate_plain_window(&existing).map_err(|e| e.to_string())?;
        return Ok(());
    }

    // Create the side panel window.
    let theme = theme_state.0.lock().map_err(|e| e.to_string())?.clone();
    let title = title_from_url(&parsed, &label);
    let script = format!(
        "{}\n{}",
        sdk_initialization_script(&label, &room_id, &theme),
        history_initialization_script(&label)
    );

    let app_for_load = app.clone();
    let label_for_load = label.clone();

    let window = WindowBuilder::new(&app, &label)
        .title(&title)
        .inner_size(panel_w_logical, panel_h_logical)
        .position(panel_x / scale_factor, panel_y / scale_factor);

    #[cfg(target_os = "macos")]
    let window = window
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true);

    #[cfg(not(any(target_os = "macos", target_os = "android", target_os = "ios")))]
    let window = window.decorations(false);

    let window = window
        .build()
        .map_err(|e| e.to_string())?;

    let titlebar_builder = WebviewBuilder::new(
        side_panel_titlebar_label(&label),
        WebviewUrl::App(PathBuf::from("webview-titlebar.html")),
    )
    .initialization_script(titlebar_initialization_script(&label, &title, parsed.as_str()));

    window
        .add_child(
            titlebar_builder,
            tauri::LogicalPosition::new(0.0, 0.0),
            tauri::LogicalSize::new(panel_w_logical, SIDE_PANEL_TITLEBAR_HEIGHT),
        )
        .map_err(|e| e.to_string())?;

    let builder = WebviewBuilder::new(
        side_panel_content_label(&label),
        WebviewUrl::External(parsed),
    )
    .data_directory(external_webview_data_dir(&app)?)
    .initialization_script(&script)
    .on_page_load(move |_webview, payload| {
        if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
            let new_title = title_from_url(payload.url(), &label_for_load);
            if let Some(win) = app_for_load.get_window(&label_for_load) {
                let _ = win.set_title(&new_title);
            }
            emit_webview_titlebar_state(
                &app_for_load,
                &label_for_load,
                &new_title,
                payload.url().as_str(),
                false,
                false,
            );
        }
    });

    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(EXTERNAL_WEBVIEW_DATA_STORE_ID);

    let _content = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(0.0, SIDE_PANEL_TITLEBAR_HEIGHT),
            tauri::LogicalSize::new(
                panel_w_logical,
                (panel_h_logical - SIDE_PANEL_TITLEBAR_HEIGHT).max(1.0),
            ),
        )
        .map_err(|e| e.to_string())?;

    // Override any state restored by tauri-plugin-window-state so the panel
    // always appears at the computed position/size, not the saved one.
    window
        .set_size(tauri::PhysicalSize::new(panel_w as u32, panel_h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(tauri::PhysicalPosition::new(panel_x as i32, panel_y as i32))
        .map_err(|e| e.to_string())?;
    resize_side_panel_children(&app, &label, panel_w_logical, panel_h_logical);

    // Store label → roomId mapping for later filtering.
    state
        .0
        .lock()
        .map_err(|e| e.to_string())?
        .insert(label.clone(), room_id);

    // Notify main window when this webview is opened.
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.emit("webview-opened", serde_json::json!({ "label": &label }));
    }

    // Notify main window when this webview is closed.
    let label_clone = label.clone();
    let app_clone = app.clone();
    let state_for_close = state.inner().0.clone();
    window.on_window_event(move |event| match event {
        tauri::WindowEvent::Resized(size) => {
            let scale_factor = app_clone
                .get_window(&label_clone)
                .and_then(|window| window.scale_factor().ok())
                .unwrap_or(1.0);
            resize_side_panel_children(
                &app_clone,
                &label_clone,
                size.width as f64 / scale_factor,
                size.height as f64 / scale_factor,
            );
        }
        tauri::WindowEvent::Destroyed => {
            if let Ok(mut map) = state_for_close.lock() {
                map.remove(&label_clone);
            }
            if let Some(main) = app_clone.get_webview_window("main") {
                let _ = main.emit(
                    "webview-closed",
                    serde_json::json!({ "label": label_clone }),
                );
            }
        }
        _ => {}
    });

    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_update_state(
    app: tauri::AppHandle,
    label: String,
    title: String,
    url: String,
    can_go_back: bool,
    can_go_forward: bool,
) -> Result<(), String> {
    emit_webview_titlebar_state(&app, &label, &title, &url, can_go_back, can_go_forward);
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_go_back(app: tauri::AppHandle, label: String) -> Result<(), String> {
    let content_label = side_panel_content_label(&label);
    if let Some(content) = app.get_webview(&content_label) {
        content
            .eval("window.history.back();")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_go_forward(app: tauri::AppHandle, label: String) -> Result<(), String> {
    let content_label = side_panel_content_label(&label);
    if let Some(content) = app.get_webview(&content_label) {
        content
            .eval("window.history.forward();")
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_reload(app: tauri::AppHandle, label: String) -> Result<(), String> {
    let content_label = side_panel_content_label(&label);
    if let Some(content) = app.get_webview(&content_label) {
        content.reload().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_is_maximized(
    app: tauri::AppHandle,
    label: String,
) -> Result<bool, String> {
    app.get_window(&label)
        .ok_or_else(|| format!("Window not found: {}", label))?
        .is_maximized()
        .map_err(|e| e.to_string())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_start_dragging(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    if let Some(window) = app.get_window(&label) {
        window.start_dragging().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_minimize(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if let Some(window) = app.get_window(&label) {
        window.minimize().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_toggle_maximize(
    app: tauri::AppHandle,
    label: String,
) -> Result<(), String> {
    if let Some(window) = app.get_window(&label) {
        if window.is_maximized().unwrap_or(false) {
            window.unmaximize().map_err(|e| e.to_string())?;
        } else {
            window.maximize().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn webview_titlebar_close(app: tauri::AppHandle, label: String) -> Result<(), String> {
    if let Some(window) = app.get_window(&label) {
        window.close().map_err(|e| e.to_string())?;
    }
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
    if let Some(child) = app
        .get_webview(&side_panel_content_label(&label))
        .or_else(|| app.get_webview(&label))
    {
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
    for (label, _room) in map.iter().filter(|(_, r)| *r == &room_id) {
        if let Some(content) = app.get_webview(&side_panel_content_label(label)) {
            let _ = content.eval(&js);
        } else if let Some(window) = app.get_webview_window(label) {
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
    state: State<'_, WebviewRoomMap>,
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
    let map = state.0.lock().map_err(|e| e.to_string())?;
    for label in map.keys() {
        if let Some(content) = app.get_webview(&side_panel_content_label(label)) {
            let _ = content.eval(&js);
        }
    }
    drop(map);

    for (label, window) in app.webview_windows() {
        if let Some(content) = app.get_webview(&side_panel_content_label(&label)) {
            let _ = content.eval(&js);
        } else if window.label() != "main"
            && !label.ends_with("--titlebar")
            && !label.ends_with("--content")
        {
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
    } else if let Some(w) = app.get_window(&label) {
        w.close().map_err(|e| e.to_string())?;
    }
    state.0.lock().map_err(|e| e.to_string())?.remove(&label);
    Ok(())
}

/// Update the tray icon to show the unread message count next to the app icon.
/// When count is 0, restores the original tray icon (no number shown).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
#[tauri::command]
async fn update_tray_badge(
    #[allow(unused_variables)] app: tauri::AppHandle,
    state: State<'_, TrayState>,
    count: u32,
    sync_status: Option<String>,
) -> Result<(), String> {
    use image::{Rgba, RgbaImage};

    let tray = {
        let guard = state.0.lock().map_err(|e| e.to_string())?;
        guard
            .as_ref()
            .ok_or_else(|| "Tray icon not initialized".to_string())?
            .clone()
    };

    let disconnected = sync_status.as_deref() == Some("disconnected");
    let status_label = disconnected.then_some("Disconnected");

    // Restore original icon when there are no unread messages or sync status marks.
    if count == 0 && !disconnected {
        #[cfg(target_os = "macos")]
        {
            let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray_icon.png"))
                .map_err(|e| e.to_string())?;
            tray.set_icon_with_as_template(Some(icon), true)
                .map_err(|e| e.to_string())?;
        }
        #[cfg(not(target_os = "macos"))]
        {
            let icon = app.default_window_icon().unwrap().clone();
            tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;
        }
        tray.set_tooltip(Some("Elevo Messenger"))
            .map_err(|e| e.to_string())?;
        return Ok(());
    }

    fn load_system_font() -> Option<Vec<u8>> {
        #[cfg(target_os = "macos")]
        let paths: &[&str] = &[
            "/System/Library/Fonts/Supplemental/Arial Bold.ttf",
            "/System/Library/Fonts/Supplemental/Arial.ttf",
            "/System/Library/Fonts/SFNSText.ttf",
        ];

        #[cfg(target_os = "windows")]
        let paths: &[&str] = &[
            r"C:\Windows\Fonts\arialbd.ttf",
            r"C:\Windows\Fonts\arial.ttf",
            r"C:\Windows\Fonts\segoeui.ttf",
        ];

        #[cfg(target_os = "linux")]
        let paths: &[&str] = &[
            "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationSans-Bold.ttf",
            "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
        ];

        for path in paths {
            if let Ok(bytes) = std::fs::read(path) {
                return Some(bytes);
            }
        }
        None
    }

    // ── Load base icon first so we can match its height ─────────────────────

    let base_bytes: &[u8] = if disconnected {
        include_bytes!("../icons/tray_icon_disconnected.png")
    } else {
        #[cfg(target_os = "macos")]
        {
            include_bytes!("../icons/tray_icon.png")
        }
        #[cfg(not(target_os = "macos"))]
        {
            include_bytes!("../icons/icon.png")
        }
    };

    let base_img = image::load_from_memory(base_bytes)
        .map_err(|e| format!("Failed to load base icon: {e}"))?
        .to_rgba8();

    let base_w = base_img.width();
    let base_h = base_img.height();

    // ── Render badge text (anti-aliased) ────────────────────────────────────

    let badge_img = if count > 0 {
        let font_bytes = load_system_font().ok_or_else(|| "No suitable system font found".to_string())?;
        let font = fontdue::Font::from_bytes(font_bytes, fontdue::FontSettings::default())
            .map_err(|e| format!("Failed to parse font: {e}"))?;

        let text = if count > 99 {
            "99+".to_string()
        } else {
            count.to_string()
        };

        let font_size: f32 = 50.0;

        // Rasterize each character individually and collect metrics.
        let chars: Vec<char> = text.chars().collect();
        let n = chars.len();
        let mut char_results: Vec<(fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(n);
        let mut total_advance: usize = 0;
        let mut glyph_top: f32 = 0.0;
        let mut glyph_bottom: f32 = 0.0;

        for &ch in &chars {
            let (metrics, bitmap) = font.rasterize(ch, font_size);
            let ymin = metrics.ymin as f32;
            let bottom = metrics.ymin as f32 + metrics.height as f32;
            if ymin < glyph_top {
                glyph_top = ymin;
            }
            if bottom > glyph_bottom {
                glyph_bottom = bottom;
            }
            total_advance += metrics.advance_width.ceil() as usize;
            char_results.push((metrics, bitmap));
        }

        let char_gap: usize = 1;
        let pad: usize = 2;
        let inner_w = total_advance + if n > 1 { (n - 1) * char_gap } else { 0 };
        let img_w: u32 = (inner_w + 2 * pad) as u32;

        #[cfg(target_os = "macos")]
        let color = Rgba([255, 255, 255, 255]);
        #[cfg(not(target_os = "macos"))]
        let color = Rgba([231, 29, 54, 255]);

        // Create badge image with same height as base icon for perfect vertical alignment.
        let mut badge_img = RgbaImage::new(img_w, base_h);

        // Align text baseline at ~76% of icon height.
        // Digits have no descenders, so baseline alignment is visually centered.
        let baseline_y = base_h as f32 * 0.76;
        let y_base: f32 = glyph_top.abs() + baseline_y - glyph_bottom;
        let mut x_off: usize = pad;

        for (metrics, bitmap) in &char_results {
            let glyph_w = metrics.width;
            let glyph_h = metrics.height;
            let gx = x_off + metrics.xmin.max(0) as usize;
            let gy = (y_base + (metrics.ymin as f32).max(0.0)).ceil() as usize;
            for py in 0..glyph_h {
                for px in 0..glyph_w {
                    let idx = py * glyph_w + px;
                    if idx < bitmap.len() {
                        let alpha = bitmap[idx];
                        if alpha > 0 {
                            let ix = gx + px;
                            let iy = gy + py;
                            if ix < img_w as usize && iy < base_h as usize {
                                badge_img.put_pixel(ix as u32, iy as u32, Rgba([color[0], color[1], color[2], alpha]));
                            }
                        }
                    }
                }
            }
            x_off += metrics.advance_width.ceil() as usize + char_gap;
        }
        Some(badge_img)
    } else {
        None
    };

    // ── Compose base icon + optional badge number ───────────────────────────

    let badge_w = badge_img.as_ref().map_or(0, RgbaImage::width);

    let gap: u32 = 12;
    let total_w = if badge_w > 0 {
        base_w + gap + badge_w
    } else {
        base_w
    };

    let mut composite = RgbaImage::new(total_w, base_h);

    // Both images are same height — no vertical offset needed.
    image::imageops::overlay(&mut composite, &base_img, 0, 0);

    // Badge number on the right.
    if let Some(badge_img) = &badge_img {
        let badge_x = base_w + gap;
        image::imageops::overlay(&mut composite, badge_img, badge_x as i64, 0);
    }

    // Encode to PNG and set as tray icon.
    let mut png_buf = std::io::Cursor::new(Vec::new());
    composite
        .write_to(&mut png_buf, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode PNG: {e}"))?;
    let png_bytes = png_buf.into_inner();

    let icon = tauri::image::Image::from_bytes(&png_bytes)
        .map_err(|e| e.to_string())?;
    // macOS: keep template mode so the monochrome tray icon adapts to the menu bar.
    #[cfg(target_os = "macos")]
    tray.set_icon_with_as_template(Some(icon), true)
        .map_err(|e| e.to_string())?;
    #[cfg(not(target_os = "macos"))]
    tray.set_icon(Some(icon)).map_err(|e| e.to_string())?;

    let tooltip = match (count, status_label) {
        (0, Some(status)) => format!("Elevo Messenger - {status}"),
        (_, Some(status)) => format!("Elevo Messenger ({count}) - {status}"),
        (_, None) => format!("Elevo Messenger ({count})"),
    };
    tray.set_tooltip(Some(tooltip)).map_err(|e| e.to_string())?;

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

    let title = title_from_url(&parsed, "Login");

    let app_nav = app.clone();
    let app_event = app.clone();
    let app_for_load = app.clone();
    let label_for_nav = label.clone();
    let label_for_load = label.clone();

    // Track whether on_navigation successfully intercepted the callback.
    // This prevents emitting "oauth-window-closed" after a successful redirect,
    // which would cause a race condition with the token exchange.
    let callback_intercepted = Arc::new(AtomicBool::new(false));
    let intercepted_for_close = callback_intercepted.clone();

    let builder = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(parsed))
        .title(&title)
        .inner_size(600.0, 700.0)
        .data_directory(external_webview_data_dir(&app)?)
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
        .on_page_load(move |_webview, payload| {
            if matches!(payload.event(), tauri::webview::PageLoadEvent::Started) {
                let new_title = title_from_url(payload.url(), "Login");
                if let Some(win) = app_for_load.get_webview_window(&label_for_load) {
                    let _ = win.set_title(&new_title);
                }
            }
        });

    #[cfg(target_os = "macos")]
    let builder = builder.data_store_identifier(EXTERNAL_WEBVIEW_DATA_STORE_ID);

    let window = builder.build().map_err(|e| e.to_string())?;

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
                activate_window_lossy(&win);
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_deep_link::init())
        .manage(WebviewRoomMap(Arc::new(Mutex::new(HashMap::new()))))
        .manage(CurrentTheme(Mutex::new("light".to_string())))
        .invoke_handler(tauri::generate_handler![
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_webview,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_side_panel,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_preview_window,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_code_view_window,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_bridge_explorer_window,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_task_board_window,
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
            webview_titlebar_update_state,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_go_back,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_go_forward,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_reload,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_is_maximized,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_start_dragging,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_minimize,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_toggle_maximize,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            webview_titlebar_close,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            update_tray_badge,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            open_oauth_window,
            #[cfg(not(any(target_os = "android", target_os = "ios")))]
            menu::update_menu_state,
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
                    match event.id().as_ref() {
                        menu::CHECK_FOR_UPDATES_ID => {
                            updater::check_for_update(&handle);
                        }
                        menu::TOGGLE_DEVTOOLS_ID => {
                            toggle_focused_devtools(&handle);
                        }
                        _ => {}
                    }
                });
            }

            // Dev: devUrl from tauri.conf.json (http://localhost:8080) for HMR
            // Release: custom protocol (tauri://localhost) serves bundled frontend
            let window_url = WebviewUrl::App(Default::default());

            let app_handle = app.handle().clone();
            let builder = WebviewWindowBuilder::new(app, "main".to_string(), window_url)
                .title("Elevo Messenger")
                .disable_drag_drop_handler()
                .on_navigation(|url| {
                    // Dev:  http://localhost:8080
                    // Prod: tauri://localhost (macOS/Linux), http://tauri.localhost (Windows)
                    if cfg!(debug_assertions) {
                        url.scheme() == "http" && url.host_str() == Some("localhost") && url.port() == Some(8080)
                    } else {
                        url.scheme() == "tauri"
                            || (url.scheme() == "http" && url.host_str() == Some("tauri.localhost"))
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
                app.manage(TrayState(Mutex::new(None)));

                let win_clone = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();

                        #[cfg(target_os = "macos")]
                        {
                            if win_clone.is_fullscreen().unwrap_or(false) {
                                let _ = win_clone.set_fullscreen(false);
                                // Avoid leaving a black fullscreen space on macOS.
                                std::thread::sleep(std::time::Duration::from_millis(300));
                            }
                        }

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

                let tray_icon = tray_builder
                    .on_tray_icon_event(move |_tray, event| {
                        if let tauri::tray::TrayIconEvent::Click {
                            button: tauri::tray::MouseButton::Left,
                            button_state: tauri::tray::MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(win) = handle.get_webview_window("main") {
                                activate_window_lossy(&win);
                            }
                        }
                    })
                    .on_menu_event(move |_app, event| {
                        match event.id().as_ref() {
                            "open" => {
                                if let Some(win) = handle_menu.get_webview_window("main") {
                                    activate_window_lossy(&win);
                                }
                            }
                            "quit" => {
                                handle_menu.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .build(app)?;

                // Store tray icon handle so update_tray_badge can access it.
                *app.state::<TrayState>().0.lock().unwrap() = Some(tray_icon);
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
                        activate_window_lossy(&win);
                    }
                }
            }

            #[cfg(not(target_os = "macos"))]
            let _ = (app, event);
        });
}
