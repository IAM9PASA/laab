//! Windows: closing the main window hides LAAB to the notification area
//! instead of quitting, so new messages still notify, as LINE for Windows
//! does. The tray icon opens it again; its menu starts LAAB with Windows
//! (straight to the tray) and quits.

use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::state::AppState;

/// What starting with Windows passes: stay in the tray, no window.
pub const HIDDEN: &str = "--hidden";
const TRAY: &str = "laab";

pub fn setup(app: &AppHandle, state: &AppState) -> tauri::Result<()> {
    // On by default, once: a choice made in the menu afterwards sticks.
    // Not in demo mode, whose data is thrown away.
    let chosen = state.paths.root.join("autostart-chosen");
    if !state.demo && !chosen.exists() {
        if let Err(e) = app.autolaunch().enable() {
            tracing::warn!("start with Windows: {e}");
        }
        let _ = std::fs::write(&chosen, b"");
    }

    let open = MenuItem::with_id(app, "open", "เปิด LAAB", true, None::<&str>)?;
    let autostart = CheckMenuItem::with_id(
        app,
        "autostart",
        "เปิดเมื่อเริ่ม Windows",
        !state.demo,
        app.autolaunch().is_enabled().unwrap_or(false),
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", "ออกจาก LAAB", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[&open, &autostart, &PredefinedMenuItem::separator(app)?, &quit],
    )?;

    let mut tray = TrayIconBuilder::with_id(TRAY)
        .tooltip("LAAB")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show(app),
            "autostart" => {
                let launcher = app.autolaunch();
                let on = launcher.is_enabled().unwrap_or(false);
                let result = if on { launcher.disable() } else { launcher.enable() };
                if let Err(e) = result {
                    tracing::warn!("start with Windows: {e}");
                }
                // The item flips itself on click; show what really happened.
                let _ = autostart.set_checked(launcher.is_enabled().unwrap_or(on));
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;

    if !std::env::args().any(|a| a == HIDDEN) {
        show(app);
    }
    Ok(())
}

/// Whether the tray icon is up, so a hidden window can come back.
pub fn exists(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY).is_some()
}

/// Bring the main window back from the tray.
pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// The tray icon's tooltip: the unread count, as the taskbar title shows it.
pub fn set_unread(app: &AppHandle, label: Option<&str>) {
    if let Some(tray) = app.tray_by_id(TRAY) {
        let text = match label {
            Some(n) => format!("LAAB · ยังไม่ได้อ่าน {n}"),
            None => "LAAB".into(),
        };
        let _ = tray.set_tooltip(Some(text));
    }
}
