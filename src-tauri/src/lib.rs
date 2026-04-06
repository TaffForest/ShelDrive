mod bridge;
mod cache;
mod commands;
pub mod crypto;
pub mod db;
mod fs;
mod safety;
mod state;

use bridge::shelby::ShelbyBridge;
use db::Database;
use log::info;
use state::AppState;
use std::sync::Arc;
use tauri::{tray::TrayIconBuilder, Manager};

fn setup_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    TrayIconBuilder::with_id("sheldrive-tray")
        .icon(app.default_window_icon().unwrap().clone())
        .icon_as_template(true)
        .tooltip("ShelDrive")
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click { .. } = event {
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
        })
        .build(app)?;

    Ok(())
}

/// Register ShelDrive as a login item on macOS so it starts automatically.
fn setup_autolaunch() {
    #[cfg(target_os = "macos")]
    {
        if let Ok(exe) = std::env::current_exe() {
            // Use osascript to add login item (works without special entitlements)
            let app_path = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.to_string_lossy().to_string());

            if let Some(path) = app_path {
                if path.ends_with(".app") {
                    let script = format!(
                        "tell application \"System Events\" to make login item at end with properties {{path:\"{}\", hidden:true}}",
                        path
                    );
                    let _ = std::process::Command::new("osascript")
                        .args(["-e", &script])
                        .output();
                    info!("Registered as login item: {}", path);
                }
            }
        }
    }
}

fn sidecar_path() -> String {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));

    // Check paths in priority order
    let candidates = vec![
        // Dev mode: relative to project root (cargo runs from src-tauri/)
        std::env::current_dir()
            .ok()
            .map(|p| p.join("../sidecar/dist/index.js")),
        // Production macOS .app bundle: Contents/Resources/sidecar-dist/sidecar.mjs
        exe_dir
            .as_ref()
            .map(|d| d.join("../Resources/sidecar-dist/sidecar.mjs")),
        // Production sibling directory
        exe_dir
            .as_ref()
            .map(|d| d.join("sidecar-dist/sidecar.mjs")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if let Ok(resolved) = candidate.canonicalize() {
            return resolved.to_string_lossy().to_string();
        }
    }

    // Fallback
    "sidecar/dist/index.js".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let sidecar = sidecar_path();
    info!("Sidecar path: {}", sidecar);

    let bridge = Arc::new(ShelbyBridge::new(&sidecar));

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
            commands::quit_app,
            commands::save_config,
        ])
        .setup(|app| {
            setup_tray(app)?;
            setup_autolaunch();
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running ShelDrive");
}
