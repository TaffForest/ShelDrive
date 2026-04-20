//! WinFSP-based filesystem driver for Windows.
//!
//! Implements the same semantics as the FUSE driver (fuse_driver.rs) but
//! against WinFSP's API. Business logic (SQLite index, staging, disk cache,
//! Shelby bridge, encryption, safety checks) is shared via the same modules.

use crate::bridge::shelby::ShelbyBridge;
use crate::cache::{DiskCache, StagingArea};
use crate::crypto;
use crate::db::index::{self, DirItem, FileEntry};
use crate::db::schema;
use crate::safety;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use log::{debug, error, info, warn};
use rusqlite::Connection;
use sha2::Digest as Sha2Digest;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use winfsp::filesystem::{
    DirInfo, FileInfo, FileSystemContext, FileSystemHost, FspFileSystem, ModificationDescriptor,
    VolumeInfo,
};
use winfsp::host::{FileContextMode, FileSystemParams, VolumeParams};
use winfsp::FspError;

const DEFAULT_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;

pub struct ShelDriveFS {
    db: Mutex<Connection>,
    bridge: Arc<ShelbyBridge>,
    disk_cache: DiskCache,
    staging: StagingArea,
    encryption_key: Option<String>,
    dirty_inodes: Mutex<HashSet<u64>>,
    next_ino: Mutex<u64>,
}

impl ShelDriveFS {
    pub fn new(
        db_path: &str,
        bridge: Arc<ShelbyBridge>,
        encryption_key: Option<String>,
    ) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        schema::initialize(&conn)?;

        Ok(Self {
            db: Mutex::new(conn),
            bridge,
            disk_cache: DiskCache::new(DEFAULT_CACHE_MAX_BYTES),
            staging: StagingArea::new(),
            encryption_key,
            dirty_inodes: Mutex::new(HashSet::new()),
            next_ino: Mutex::new(2),
        })
    }

    fn alloc_ino(&self) -> u64 {
        let mut n = self.next_ino.lock().unwrap();
        let ino = *n;
        *n += 1;
        ino
    }

    fn now_iso() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// Flush content for an inode to Shelby and update CID in SQLite.
    fn sync_to_shelby(&self, ino: u64, path: &str) {
        let is_dirty = self.dirty_inodes.lock().unwrap().remove(&ino);
        if !is_dirty {
            return;
        }

        let content = match self.staging.read_all(ino) {
            Some(c) if !c.is_empty() => c,
            _ => return,
        };

        // Safety check
        let verdict = safety::scan_image(path, &content);
        if verdict.is_blocked() {
            warn!("Content blocked: {} — {}", path, verdict.reason());
            self.staging.remove(ino);
            let conn = self.db.lock().unwrap();
            let blocked_cid = format!("blocked:{}", verdict.reason());
            let _ = index::update_file_cid(&conn, path, &blocked_cid, 0, &Self::now_iso());
            return;
        }

        // Encrypt with per-folder key or fallback to private key
        let upload_data = {
            let conn = self.db.lock().unwrap();
            let folder_key_result = index::get_folder_key_for_path(&conn, path);
            drop(conn);

            if let Some(fk) = folder_key_result {
                if let Some(ref pk) = self.encryption_key {
                    match crypto::unwrap_folder_key(&fk.encrypted_key, pk) {
                        Ok(folder_key) => crypto::encrypt_with_key(&content, &folder_key)
                            .unwrap_or_else(|_| content.clone()),
                        Err(_) => content.clone(),
                    }
                } else {
                    content.clone()
                }
            } else if let Some(ref key) = self.encryption_key {
                crypto::encrypt(&content, key).unwrap_or_else(|_| content.clone())
            } else {
                content.clone()
            }
        };

        let content_b64 = BASE64.encode(&upload_data);
        let filename = path.rsplit('\\').next().unwrap_or(path);

        match self.bridge.pin(&content_b64, Some(filename)) {
            Ok(result) => {
                info!("Pinned {} → CID: {}", path, result.cid);
                self.disk_cache.put(&result.cid, &content);
                self.staging.remove(ino);
                let conn = self.db.lock().unwrap();
                let _ = index::update_file_cid(
                    &conn,
                    path,
                    &result.cid,
                    result.size_bytes,
                    &Self::now_iso(),
                );
            }
            Err(e) => {
                error!("Pin failed for {}: {}", path, e);
                let local_cid = format!("local:{}", hex::encode(&sha2::Sha256::digest(path.as_bytes())[..16]));
                self.disk_cache.put(&local_cid, &content);
                self.dirty_inodes.lock().unwrap().insert(ino);
            }
        }
    }

    fn fetch_content(&self, ino: u64, path: &str, cid: &str) -> Option<Vec<u8>> {
        if let Some(data) = self.staging.read_all(ino) {
            return Some(data);
        }
        if cid == "pending" {
            return None;
        }
        if let Some(data) = self.disk_cache.get(cid) {
            return Some(data);
        }
        if cid.starts_with("stub:") || cid.starts_with("local:") {
            return None;
        }
        match self.bridge.retrieve(cid) {
            Ok(result) => {
                let raw = BASE64.decode(&result.content).unwrap_or_default();
                let data = if let Some(ref pk) = self.encryption_key {
                    let conn = self.db.lock().unwrap();
                    let fk_result = index::get_folder_key_for_path(&conn, path);
                    drop(conn);
                    if let Some(fk) = fk_result {
                        match crypto::unwrap_folder_key(&fk.encrypted_key, pk) {
                            Ok(fkey) => crypto::decrypt_with_key(&raw, &fkey).unwrap_or(raw.clone()),
                            Err(_) => crypto::decrypt(&raw, pk).unwrap_or(raw.clone()),
                        }
                    } else {
                        crypto::decrypt(&raw, pk).unwrap_or(raw.clone())
                    }
                } else {
                    raw
                };
                self.disk_cache.put(cid, &data);
                Some(data)
            }
            Err(e) => {
                warn!("Retrieve failed for {}: {}", path, e);
                None
            }
        }
    }
}

// NOTE: The full WinFSP FileSystemContext trait implementation is ~30 methods.
// This is a scaffold — the actual implementation requires:
//   - open, close, read, write, create, delete, rename
//   - get_file_info, set_file_info, get_security, set_security
//   - read_directory, get_volume_info
//
// For Windows support, each method translates a WinFSP call to our
// SQLite index + staging + Shelby bridge (exactly like fuse_driver.rs does
// for FUSE operations). This is a multi-day integration and will be completed
// iteratively, tested on the Windows VM.

pub struct MountHandle {
    _fs: FileSystemHost<'static>,
}

pub fn mount(
    mount_point: &str,
    db_path: &str,
    bridge: Arc<ShelbyBridge>,
    encryption_key: Option<String>,
) -> Result<MountHandle, String> {
    let _fs = ShelDriveFS::new(db_path, bridge, encryption_key)
        .map_err(|e| format!("Failed to initialize filesystem: {}", e))?;

    // TODO: Full WinFSP integration
    // 1. Initialize WinFSP with winfsp::init()
    // 2. Build VolumeParams (name, sector size, volume serial)
    // 3. Build FileSystemParams with context mode
    // 4. Create FileSystemHost and mount at drive letter
    // 5. Store in MountHandle for Drop-based unmount

    Err(format!(
        "WinFSP driver scaffolded but not yet complete. Mount point: {}",
        mount_point
    ))
}
