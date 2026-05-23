//! Build the system tray icon + menu. Returns the icon (keep it alive) and the
//! menu item ids the app polls.

use tray_icon::menu::{Menu, MenuId, MenuItem};
use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

/// The tray icon plus the ids of its "Open" and "Quit" menu items.
pub struct Tray {
    pub icon: TrayIcon,
    pub open_id: MenuId,
    pub quit_id: MenuId,
}

/// Build a small solid-colour RGBA icon (no asset file needed).
fn solid_icon() -> Icon {
    const SIZE: u32 = 32;
    let mut rgba = Vec::with_capacity((SIZE * SIZE * 4) as usize);
    for _ in 0..(SIZE * SIZE) {
        rgba.extend_from_slice(&[0x2e, 0x7d, 0x32, 0xff]); // green
    }
    Icon::from_rgba(rgba, SIZE, SIZE).expect("valid icon")
}

/// Create the tray icon + menu. Call on the main thread (after the event loop
/// starts on Windows — i.e., from the eframe creation closure).
pub fn build() -> Result<Tray, String> {
    let open = MenuItem::new("Open Salvaê", true, None);
    let quit = MenuItem::new("Quit", true, None);
    let menu = Menu::new();
    menu.append(&open).map_err(|e| e.to_string())?;
    menu.append(&quit).map_err(|e| e.to_string())?;
    let open_id = open.id().clone();
    let quit_id = quit.id().clone();
    let icon = TrayIconBuilder::new()
        .with_tooltip("Salvaê")
        .with_menu(Box::new(menu))
        .with_icon(solid_icon())
        .build()
        .map_err(|e| e.to_string())?;
    Ok(Tray {
        icon,
        open_id,
        quit_id,
    })
}
