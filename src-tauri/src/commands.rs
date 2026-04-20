use crate::bridge::shelby::{ShelbyBridge, ShelbyStatus};
use crate::db::index;
use crate::db::Database;
use crate::fs::driver as fuse_driver;
use crate::state::{AppState, AppStatus, MountStatus};
use log::{error, info};
use std::sync::Arc;
use tauri::{image::Image, Manager, State};

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> AppStatus {
    state.status.lock().unwrap().clone()
}

fn update_tray_icon(app: &tauri::AppHandle, mounted: bool) {
    if let Some(tray) = app.tray_by_id("sheldrive-tray") {
        let icon_bytes: &[u8] = if mounted {
            include_bytes!("../icons/tray-connected.png")
        } else {
            include_bytes!("../icons/tray-disconnected.png")
        };
        if let Ok(img) = Image::from_bytes(icon_bytes) {
            let _ = tray.set_icon(Some(img));
        }
        let tooltip = if mounted {
            "ShelDrive — Mounted"
        } else {
            "ShelDrive — Disconnected"
        };
        let _ = tray.set_tooltip(Some(tooltip));
    }
}

#[tauri::command]
pub fn mount_drive(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    bridge: State<'_, Arc<ShelbyBridge>>,
) -> AppStatus {
    let mut status = state.status.lock().unwrap();

    if status.mount_status == MountStatus::Mounted {
        return status.clone();
    }

    // Auto-install FUSE-T if not present
    if !is_fuse_installed() {
        info!("FUSE not found — installing FUSE-T...");
        status.mount_status = MountStatus::Connecting;
        status.error_message = Some("Installing FUSE-T...".to_string());
        drop(status);

        if let Err(e) = install_fuse_t() {
            let mut s = state.status.lock().unwrap();
            s.mount_status = MountStatus::Error;
            s.error_message = Some(format!("Failed to install FUSE-T: {}. Install manually: brew install --cask fuse-t", e));
            return s.clone();
        }

        info!("FUSE-T installed successfully");
        status = state.status.lock().unwrap();
        status.error_message = None;
    }

    let mount_point = status.mount_point.clone();
    let db_path = db_path();
    let bridge_clone = (*bridge).clone();

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
            update_tray_icon(&app, true);
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
pub fn unmount_drive(app: tauri::AppHandle, state: State<'_, AppState>) -> AppStatus {
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
    update_tray_icon(&app, false);

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
pub fn share_folder(
    db: State<'_, Database>,
    folder_path: String,
    recipient_address: String,
) -> Result<String, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;

    // Get the folder's encryption key
    let fk = index::get_folder_key(&conn, &folder_path)
        .map_err(|_| format!("No encryption key found for folder: {}", folder_path))?;

    // Load our private key to unwrap then re-wrap for recipient
    let config = crate::bridge::shelby::ShelbyConfig::load();
    let private_key = config.private_key
        .ok_or("No private key configured — cannot share")?;

    // Unwrap the folder key with our private key
    let folder_key = crate::crypto::unwrap_folder_key(&fk.encrypted_key, &private_key)
        .map_err(|e| format!("Failed to unwrap folder key: {}", e))?;

    // Wrap the folder key for the recipient
    let recipient_wrapped = crate::crypto::wrap_folder_key_for_recipient(
        &folder_key,
        &private_key,
        &recipient_address,
    )
    .map_err(|e| format!("Failed to wrap key for recipient: {}", e))?;

    // Store the shared member
    let member = index::SharedMember {
        folder_path: folder_path.clone(),
        member_address: recipient_address.clone(),
        encrypted_key: recipient_wrapped,
        added_at: chrono::Utc::now().to_rfc3339(),
    };
    index::add_shared_member(&conn, &member).map_err(|e| e.to_string())?;

    info!("Shared {} with {}", folder_path, recipient_address);
    Ok(format!("Shared {} with {}", folder_path, recipient_address))
}

#[tauri::command]
pub fn get_shared_folders(db: State<'_, Database>) -> Result<Vec<index::SharedMember>, String> {
    let conn = db.conn.lock().map_err(|e| e.to_string())?;
    // Get all shared members across all folders
    let mut stmt = conn
        .prepare("SELECT folder_path, member_address, encrypted_key, added_at FROM shared_members")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |row| {
            Ok(index::SharedMember {
                folder_path: row.get(0)?,
                member_address: row.get(1)?,
                encrypted_key: row.get(2)?,
                added_at: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

fn install_fuse_t() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let tmp_pkg = "/tmp/fuse-t.pkg";
        info!("Downloading FUSE-T...");
        let dl = std::process::Command::new("curl")
            .args(["-fsSL", "-o", tmp_pkg, "https://github.com/macos-fuse-t/fuse-t/releases/download/1.2.0/fuse-t-macos-installer-1.2.0.pkg"])
            .output()
            .map_err(|e| format!("Download failed: {}", e))?;
        if !dl.status.success() {
            return Err(format!("Download failed: {}", String::from_utf8_lossy(&dl.stderr)));
        }
        info!("Installing FUSE-T (will prompt for password)...");
        let install = std::process::Command::new("osascript")
            .args([
                "-e",
                &format!(
                    "do shell script \"installer -pkg {} -target /\" with administrator privileges",
                    tmp_pkg
                ),
            ])
            .output()
            .map_err(|e| format!("Install failed: {}", e))?;
        let _ = std::fs::remove_file(tmp_pkg);
        if install.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&install.stderr).to_string())
        }
    }

    #[cfg(target_os = "windows")]
    {
        // Download WinFSP installer MSI and launch it with admin privileges
        let tmp_msi = std::env::temp_dir().join("winfsp-installer.msi");
        let tmp_str = tmp_msi.to_string_lossy().to_string();
        info!("Downloading WinFSP...");
        // WinFSP 2.0 installer from GitHub releases
        let dl = std::process::Command::new("powershell")
            .args([
                "-Command",
                &format!(
                    "Invoke-WebRequest -Uri 'https://github.com/winfsp/winfsp/releases/download/v2.0/winfsp-2.0.23075.msi' -OutFile '{}'",
                    tmp_str
                ),
            ])
            .output()
            .map_err(|e| format!("Download failed: {}", e))?;
        if !dl.status.success() {
            return Err(format!("Download failed: {}", String::from_utf8_lossy(&dl.stderr)));
        }
        info!("Installing WinFSP (will prompt for admin)...");
        let install = std::process::Command::new("msiexec")
            .args(["/i", &tmp_str, "/quiet", "/norestart"])
            .output()
            .map_err(|e| format!("Install failed: {}", e))?;
        let _ = std::fs::remove_file(&tmp_msi);
        if install.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&install.stderr).to_string())
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        Err("Auto-install only supported on macOS and Windows".to_string())
    }
}

fn is_fuse_installed() -> bool {
    #[cfg(target_os = "macos")]
    {
        // Check for FUSE-T
        if std::path::Path::new("/usr/local/lib/libfuse3.dylib").exists() {
            return true;
        }
        // Check for macFUSE
        if std::path::Path::new("/Library/Frameworks/macFUSE.framework").exists() {
            return true;
        }
        false
    }
    #[cfg(target_os = "linux")]
    {
        std::path::Path::new("/usr/lib/libfuse3.so").exists()
            || std::path::Path::new("/usr/lib/x86_64-linux-gnu/libfuse3.so").exists()
    }
    #[cfg(target_os = "windows")]
    {
        // WinFSP installs to Program Files (x86) by default
        std::path::Path::new("C:\\Program Files (x86)\\WinFsp\\bin\\winfsp-x64.dll").exists()
            || std::path::Path::new("C:\\Program Files\\WinFsp\\bin\\winfsp-x64.dll").exists()
    }
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
