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

    info!("Mounting ShelDrive at {}", mount_point);

    match fuse_driver::mount(&mount_point, &db_path, bridge_clone) {
        Ok(handle) => {
            info!("ShelDrive mounted successfully");
            *state.mount_handle.lock().unwrap() = Some(handle);
            status.mount_status = MountStatus::Mounted;
            status.error_message = None;
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
