//! The macOS menu bar: the platform's usual menus, with setting up and
//! removing Stemma in the application menu and the editor's two views in
//! View.
//!
//! Elsewhere the window has no menu bar, and the same shortcuts reach the
//! webview as key presses.

use tauri::menu::{Menu, MenuEvent, MenuItem, MenuItemKind, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Runtime};

/// The event the webview hears when a view is chosen from the menu, with
/// the view's name as its payload.
const VIEW_EVENT: &str = "view";

/// The event the webview hears when setting this machine up or removing
/// Stemma from it is chosen, with `setup` or `remove` as its payload.
const MACHINE_EVENT: &str = "machine";

/// The default menu, with this machine's items after About and Blocks and
/// PDF at the top of View.
pub fn build<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<Menu<R>> {
    let menu = Menu::default(app)?;
    let blocks = MenuItem::with_id(app, "view:blocks", "Blocks", true, Some("CmdOrCtrl+1"))?;
    let pdf = MenuItem::with_id(app, "view:pdf", "PDF", true, Some("CmdOrCtrl+2"))?;
    let set_up = MenuItem::with_id(
        app,
        "machine:setup",
        "Set Up This Machine…",
        true,
        None::<&str>,
    )?;
    let remove = MenuItem::with_id(
        app,
        "machine:remove",
        "Remove Stemma from This Machine…",
        true,
        None::<&str>,
    )?;

    for (at, item) in menu.items()?.into_iter().enumerate() {
        let MenuItemKind::Submenu(submenu) = item else {
            continue;
        };

        // The first menu is the application's own.
        if at == 0 {
            submenu.insert_items(&[&PredefinedMenuItem::separator(app)?, &set_up, &remove], 1)?;
        } else if submenu.text()? == "View" {
            submenu.prepend_items(&[&blocks, &pdf, &PredefinedMenuItem::separator(app)?])?;
        }
    }

    Ok(menu)
}

/// Passes a view or a machine item chosen from the menu on to the
/// webview.
pub fn chosen<R: Runtime>(app: &AppHandle<R>, event: MenuEvent) {
    let id = event.id().as_ref();
    if let Some(view) = id.strip_prefix("view:") {
        let _ = app.emit(VIEW_EVENT, view);
    } else if let Some(action) = id.strip_prefix("machine:") {
        let _ = app.emit(MACHINE_EVENT, action);
    }
}
