use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};

pub const CHECK_FOR_UPDATES_ID: &str = "check_for_updates";

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

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .build()
}