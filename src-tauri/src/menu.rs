use tauri::menu::{MenuBuilder, MenuItemBuilder, MenuItemKind, SubmenuBuilder};

use crate::updater;

pub const CHECK_FOR_UPDATES_ID: &str = "check_for_updates";
pub const TOGGLE_DEVTOOLS_ID: &str = "toggle_developer_tools";

fn update_item_in_items(items: &[MenuItemKind<tauri::Wry>], id: &str, text: &str, enabled: bool) -> bool {
    for item in items {
        if item.id() == id {
            if let Some(mi) = item.as_menuitem() {
                let _ = mi.set_text(text);
                let _ = mi.set_enabled(enabled);
            }
            return true;
        }
        if let Some(sub) = item.as_submenu() {
            if let Ok(children) = sub.items() {
                if update_item_in_items(&children, id, text, enabled) {
                    return true;
                }
            }
        }
    }
    false
}

#[tauri::command]
pub fn update_menu_state(app: tauri::AppHandle, apply: bool, downloaded: bool, disabled: bool) {
    updater::set_apply_mode(apply);

    let text = if downloaded {
        "Restart to Update"
    } else if apply {
        "Update and Restart"
    } else {
        "Check for Updates..."
    };

    if let Some(menu) = app.menu() {
        if let Ok(items) = menu.items() {
            update_item_in_items(&items, CHECK_FOR_UPDATES_ID, text, !disabled);
        }
    }
}

pub fn build_menu(app: &tauri::App) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    let check_for_updates = MenuItemBuilder::with_id(CHECK_FOR_UPDATES_ID, "Check for Updates...")
        .build(app)?;

    let app_menu = SubmenuBuilder::new(app, "Elevo Messenger")
        .about(Some(Default::default()))
        .separator()
        .item(&check_for_updates)
        .separator()
        .hide()
        .hide_others()
        .show_all()
        .separator()
        .quit()
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .fullscreen()
        .build()?;

    let window_menu = SubmenuBuilder::new(app, "Window")
        .minimize()
        .build()?;

    let toggle_devtools = MenuItemBuilder::with_id(TOGGLE_DEVTOOLS_ID, "Toggle Developer Tools")
        .build(app)?;

    let help_menu = SubmenuBuilder::new(app, "Help")
        .item(&toggle_devtools)
        .build()?;

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .item(&help_menu)
        .build()
}
