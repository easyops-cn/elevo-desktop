#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod menu;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::WebviewWindowBuilder,
    Manager, RunEvent, WebviewUrl, WindowEvent,
};

pub fn run() {
    let port: u16 = 44548;
    let context = tauri::generate_context!();
    let builder = tauri::Builder::default();

    // #[cfg(target_os = "macos")]
    // {
    //     builder = builder.menu(menu::menu());
    // }

    builder
        .plugin(
            tauri_plugin_localhost::Builder::new(port)
                .on_request(|_req, res| {
                    // 覆盖默认 CSP，允许 fetch 请求访问 localhost
                    res.add_header(
                        "Content-Security-Policy",
                        "default-src 'self' http://localhost:* https://*; \
                         script-src 'self' 'unsafe-inline' 'unsafe-eval' blob:; \
                         connect-src 'self' http://localhost:* https://* wss://*; \
                         img-src 'self' data: blob: https://*; \
                         media-src 'self' blob: https://*; \
                         font-src 'self' data:; \
                         style-src 'self' 'unsafe-inline'",
                    );
                })
                .build(),
        )
        .plugin(tauri_plugin_window_state::Builder::default().build())
        // 窗口事件处理：关闭时隐藏而非退出
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // 隐藏窗口而非关闭
                let _ = window.hide();
                // 阻止默认关闭行为
                api.prevent_close();
            }
        })
        .setup(move |app| {
            // 创建主窗口
            let url = format!("http://localhost:{}", port).parse().unwrap();
            let window_url = WebviewUrl::External(url);
            WebviewWindowBuilder::new(app, "main".to_string(), window_url)
                .title("Elevo")
                .build()?;

            // 创建托盘菜单
            let show_item = MenuItem::with_id(app, "show", "显示窗口", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            // 加载托盘图标（使用应用默认图标）
            let icon = app
                .default_window_icon()
                .cloned()
                .expect("加载托盘图标失败");

            // 创建系统托盘
            TrayIconBuilder::new()
                .icon(icon)
                .menu(&tray_menu)
                .show_menu_on_left_click(false) // 左键点击不显示菜单，而是恢复窗口
                // 托盘图标点击事件
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                // 托盘菜单事件
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.unminimize();
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // macOS 菜单设置
            #[cfg(target_os = "macos")]
            {
                let menu = menu::create_menu(app.handle())?;
                app.set_menu(menu)?;
            }

            Ok(())
        })
        .build(context)
        .expect("error while building tauri application")
        .run(|app, event| {
            // macOS: 点击 Dock 图标时恢复窗口
            if let RunEvent::Reopen { .. } = event {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.unminimize();
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        });
}
