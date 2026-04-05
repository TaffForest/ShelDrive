use crate::bridge::shelby::{ShelbyBridge, ShelbyStatus};
use crate::db::index;
use crate::db::Database;
use crate::fs::fuse_driver;
use crate::state::{AppState, AppStatus, MountStatus};
use log::{error, info};
use std::sync::Arc;
use std::thread;
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

    status.mount_status = MountStatus::Connecting;
    status.error_message = None;
    drop(status);

    *state.fuse_unmount_flag.lock().unwrap() = false;

    let handle = thread::Builder::new()
        .name("sheldrive-fuse".to_string())
        .spawn(move || {
            info!("Mounting ShelDrive at {}", mount_point);
            match fuse_driver::mount(&mount_point, &db_path, bridge_clone) {
                Ok(mount_handle) => {
                    info!("ShelDrive mounted successfully");
                    std::mem::forget(mount_handle);
                }
                Err(e) => {
                    error!("Mount failed: {}", e);
                }
            }
        });

    let mut status = state.status.lock().unwrap();
    match handle {
        Ok(h) => {
            *state.fuse_thread.lock().unwrap() = Some(h);
            status.mount_status = MountStatus::Mounted;
            status.error_message = None;
        }
        Err(e) => {
            status.mount_status = MountStatus::Error;
            status.error_message = Some(format!("Failed to start FUSE thread: {}", e));
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

    *state.fuse_unmount_flag.lock().unwrap() = true;

    let unmount_result = unmount_fuse(&mount_point);

    if let Some(handle) = state.fuse_thread.lock().unwrap().take() {
        let _ = handle.join();
    }

    match unmount_result {
        Ok(()) => {
            status.mount_status = MountStatus::Disconnected;
            status.error_message = None;
            info!("ShelDrive unmounted");
        }
        Err(e) => {
            status.mount_status = MountStatus::Disconnected;
            status.error_message = Some(format!("Unmount warning: {}", e));
            error!("Unmount error: {}", e);
        }
    }

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
