use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::thread::JoinHandle;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MountStatus {
    Disconnected,
    Connecting,
    Mounted,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppStatus {
    pub mount_status: MountStatus,
    pub mount_point: String,
    pub error_message: Option<String>,
}

pub struct AppState {
    pub status: Mutex<AppStatus>,
    /// Handle to the background thread running the FUSE session.
    /// Stored so we can signal unmount.
    pub fuse_thread: Mutex<Option<JoinHandle<()>>>,
    /// Flag to signal the FUSE thread to stop
    pub fuse_unmount_flag: Mutex<bool>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            status: Mutex::new(AppStatus {
                mount_status: MountStatus::Disconnected,
                mount_point: if cfg!(target_os = "macos") {
                    "/Volumes/ShelDrive".to_string()
                } else if cfg!(target_os = "windows") {
                    "S:\\".to_string()
                } else {
                    "/mnt/sheldrive".to_string()
                },
                error_message: None,
            }),
            fuse_thread: Mutex::new(None),
            fuse_unmount_flag: Mutex::new(false),
        }
    }
}
