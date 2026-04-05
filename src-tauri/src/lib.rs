mod bridge;
mod cache;
mod commands;
pub mod db;
mod fs;
mod state;

use bridge::shelby::ShelbyBridge;
use db::Database;
use log::info;
use state::AppState;
use std::sync::Arc;
use tauri::{
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    Manager,
};

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let show_item = MenuItem::with_id(app, "show", "Show Panel", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit ShelDrive", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_item, &quit_item])?;

    TrayIconBuilder::with_id("sheldrive-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .icon_as_template(true)
        .menu(&menu)
        .tooltip("ShelDrive — Disconnected")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("tray-panel") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { button, .. } = event {
                if button == tauri::tray::MouseButton::Left {
                    let app = tray.app_handle();
                    if let Some(window) = app.get_webview_window("tray-panel") {
                        if window.is_visible().unwrap_or(false) {
                            let _ = window.hide();
                        } else {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}

fn sidecar_path() -> String {
    // In development, use the sidecar source directly via tsx
    // In production, this would point to the bundled compiled JS
    let dev_path = std::env::current_dir()
        .ok()
        .map(|p| p.join("../sidecar/dist/index.js"))
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().to_string());

    dev_path.unwrap_or_else(|| {
        // Fallback: look relative to the binary
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()));

        if let Some(dir) = exe_dir {
            // Check macOS .app bundle resources
            let app_bundle = dir.join("../Resources/sidecar/dist/index.js");
            if app_bundle.exists() {
                return app_bundle.to_string_lossy().to_string();
            }
            // Check sibling sidecar dir
            let sibling = dir.join("sidecar/dist/index.js");
            if sibling.exists() {
                return sibling.to_string_lossy().to_string();
            }
        }

        "sidecar/dist/index.js".to_string()
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let sidecar = sidecar_path();
    info!("Sidecar path: {}", sidecar);

    let bridge = Arc::new(ShelbyBridge::new(&sidecar));

    // Start sidecar immediately
    if let Err(e) = bridge.start() {
        log::error!("Failed to start Shelby sidecar: {}", e);
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(AppState::new())
        .manage(Database::open().expect("Failed to open ShelDrive database"))
        .manage(bridge.clone())
        .invoke_handler(tauri::generate_handler![
            commands::get_status,
            commands::mount_drive,
            commands::unmount_drive,
            commands::get_file_count,
            commands::get_shelby_status,
            commands::shelby_ping,
        ])
        .setup(|app| {
            setup_tray(app)?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ShelDrive");
}
