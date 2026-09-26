//! The macOS menu bar: the platform's usual menus, with the editor's two
//! views in View.
//!
//! Elsewhere the window has no menu bar, and the same shortcuts reach the
//! webview as key presses.

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Runtime};

/// The event the webview hears when a view is chosen from the menu, with
/// the view's name as its payload.
const VIEW_EVENT: &str = "view";

/// The default menu, with Blocks and PDF at the top of View.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    let blocks = MenuItem::with_id(app, "view:blocks", "Blocks", true, Some("CmdOrCtrl+1"))?;
    let pdf = MenuItem::with_id(app, "view:pdf", "PDF", true, Some("CmdOrCtrl+2"))?;
    let separator = PredefinedMenuItem::separator(app)?;

    for item in menu.items()? {
        if let MenuItemKind::Submenu(submenu) = item
            && submenu.text()? == "View"
        {
            submenu.prepend_items(&[&blocks, &pdf, &separator])?;
        }
    }

    Ok(menu)
}

/// Passes a view chosen from the menu on to the webview.
pub fn chosen<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    if let Some(view) = event.id().as_ref().strip_prefix("view:") {
        let _ = app.emit(VIEW_EVENT, view);
    }
}
