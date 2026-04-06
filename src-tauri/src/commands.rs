use crate::bridge::shelby::{ShelbyBridge, ShelbyStatus};
use crate::db::index;
use crate::db::Database;
use crate::fs::fuse_driver;
use crate::state::{AppState, AppStatus, MountStatus};
use log::{error, info};
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    state.status.lock().unwrap().clone()
}

#[tauri::command]
pub fn mount_drive(
    state: State<'_, AppState>,
    bridge: State<'_, Arc<ShelbyBridge>>,
) -> AppStatus {
    let mut status = state.status.lock().unwrap();

    if status.mount_status == MountStatus::Mounted {
        return status.clone();
    }

    let mount_point = status.mount_point.clone();
    let db_path = db_path();
    let bridge_clone = (*bridge).clone();

    // Load encryption key from config (uses private_key as passphrase)
    let config = crate::bridge::shelby::ShelbyConfig::load();
    let encryption_key = config.private_key;

    info!("Mounting ShelDrive at {}", mount_point);

    match fuse_driver::mount(&mount_point, &db_path, bridge_clone, encryption_key) {
        Ok(handle) => {
            info!("ShelDrive mounted successfully");
            *state.mount_handle.lock().unwrap() = Some(handle);
            status.mount_status = MountStatus::Mounted;
            status.error_message = None;

            // Open the mounted drive in Finder
            let _ = std::process::Command::new("open")
                .arg(&mount_point)
                .output();
        }
        Err(e) => {
            error!("Mount failed: {}", e);
            status.mount_status = MountStatus::Error;
            status.error_message = Some(e);
        }
    }

    status.clone()
}

#[tauri::command]
pub fn unmount_drive(state: State<'_, AppState>) -> AppStatus {
    let mut status = state.status.lock().unwrap();

    if status.mount_status == MountStatus::Disconnected {
        return status.clone();
    }

    let mount_point = status.mount_point.clone();

    // Drop the BackgroundSession — this triggers unmount
    state.mount_handle.lock().unwrap().take();

    // Also run umount as cleanup
    let _ = unmount_fuse(&mount_point);

    status.mount_status = MountStatus::Disconnected;
    status.error_message = None;
    info!("ShelDrive unmounted");

    status.clone()
}

#[tauri::command]
pub fn get_file_count(db: State<'_, Database>) -> Result<i64, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    // Force WAL read to see FUSE driver's writes from its separate connection
    let _ = conn.execute_batch("BEGIN; END;");
    index::count_files(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_shelby_status(bridge: State<'_, Arc<ShelbyBridge>>) -> Result<ShelbyStatus, String> {
    bridge.status()
}

#[tauri::command]
pub fn shelby_ping(bridge: State<'_, Arc<ShelbyBridge>>) -> Result<bool, String> {
    bridge.ping()
}

#[tauri::command]
pub fn save_config(
    rpc_url: Option<String>,
    api_key: Option<String>,
    private_key: Option<String>,
) -> Result<(), String> {
    let config_path = dirs::home_dir()
        .ok_or("No home directory")?
        .join(".sheldrive")
        .join("config.toml");

    // Read existing config or start fresh
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    lines.push("# ShelDrive Configuration".to_string());
    lines.push("".to_string());
    lines.push("[shelby]".to_string());

    // Preserve network from existing, default to TESTNET
    let network = existing
        .lines()
        .find(|l| l.trim().starts_with("network"))
        .and_then(|l| l.split('=').nth(1))
        .map(|v| v.trim().trim_matches('"').to_string())
        .unwrap_or_else(|| "TESTNET".to_string());
    lines.push(format!("network = \"{}\"", network));

    if let Some(ref key) = api_key {
        if !key.is_empty() {
            lines.push(format!("api_key = \"{}\"", key));
        }
    } else if let Some(existing_key) = extract_value(&existing, "api_key") {
        lines.push(format!("api_key = \"{}\"", existing_key));
    }

    if let Some(ref url) = rpc_url {
        if !url.is_empty() {
            lines.push(format!("rpc_url = \"{}\"", url));
        }
    } else if let Some(existing_url) = extract_value(&existing, "rpc_url") {
        lines.push(format!("rpc_url = \"{}\"", existing_url));
    }

    if let Some(ref pk) = private_key {
        if !pk.is_empty() {
            lines.push(format!("private_key = \"{}\"", pk));
        }
    } else if let Some(existing_pk) = extract_value(&existing, "private_key") {
        lines.push(format!("private_key = \"{}\"", existing_pk));
    }

    lines.push(String::new());
    std::fs::write(&config_path, lines.join("\n"))
        .map_err(|e| format!("Failed to write config: {}", e))?;

    info!("Config saved to {:?}", config_path);
    Ok(())
}

fn extract_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        if line.starts_with(key) {
            line.split('=').nth(1).map(|v| v.trim().trim_matches('"').to_string())
        } else {
            None
        }
    })
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn db_path() -> String {
    let home = dirs::home_dir().expect("Could not determine home directory");
    home.join(".sheldrive")
        .join("index.db")
        .to_string_lossy()
        .to_string()
}

fn unmount_fuse(mount_point: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("umount")
            .arg(mount_point)
            .output()
            .map_err(|e| format!("umount exec failed: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("umount failed: {}", stderr.trim()))
        }
    }

    #[cfg(target_os = "linux")]
    {
        let output = std::process::Command::new("fusermount")
            .arg("-u")
            .arg(mount_point)
            .output()
            .map_err(|e| format!("fusermount exec failed: {}", e))?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("fusermount -u failed: {}", stderr.trim()))
        }
    }

    #[cfg(target_os = "windows")]
    {
        Err("Windows unmount not yet implemented".to_string())
    }
}
