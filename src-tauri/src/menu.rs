use tauri::menu::{Menu, MenuBuilder, SubmenuBuilder};
use tauri::AppHandle;

/// 创建 macOS 原生菜单
///
/// # Arguments
/// * `app` - Tauri 应用句柄
///
/// # Returns
/// * `Result<Menu<tauri::Wry>, tauri::Error>` - 菜单对象或错误
pub fn create_menu(app: &AppHandle) -> Result<Menu<tauri::Wry>, tauri::Error> {
    // 应用菜单（macOS 第一个菜单）
    let app_menu = SubmenuBuilder::new(app, "Elevo")
        .about(None)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    // 编辑菜单
    let edit_menu = SubmenuBuilder::new(app, "编辑")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    // 视图菜单
    let view_menu = SubmenuBuilder::new(app, "视图")
        .fullscreen()
        .build()?;

    // 窗口菜单
    let window_menu = SubmenuBuilder::new(app, "窗口")
        .minimize()
        .close_window()
        .build()?;

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .build()
}
