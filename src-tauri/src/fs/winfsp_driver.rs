//! WinFSP-based filesystem driver for Windows.
//!
//! Implements the same semantics as the FUSE driver (fuse_driver.rs) but
//! against WinFSP's FileSystemContext trait. Business logic (SQLite index,
//! staging, disk cache, Shelby bridge, encryption, safety checks) is shared
//! via the same modules.

#![cfg(windows)]

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
use std::collections::{HashMap, HashSet};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use winfsp::filesystem::{
    DirInfo, DirMarker, FileInfo, FileSecurity, FileSystemContext, ModificationDescriptor,
    OpenFileInfo, VolumeInfo, WideNameInfo,
};
use winfsp::host::{FileSystemHost, VolumeParams};
use winfsp::{FspError, Result as FspResult, U16CStr};
use windows::Win32::Foundation::{
    STATUS_ACCESS_DENIED, STATUS_DIRECTORY_NOT_EMPTY, STATUS_END_OF_FILE,
    STATUS_FILE_IS_A_DIRECTORY, STATUS_NOT_A_DIRECTORY, STATUS_OBJECT_NAME_COLLISION,
    STATUS_OBJECT_NAME_NOT_FOUND,
};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_DIRECTORY, FILE_ATTRIBUTE_NORMAL, FILE_DIRECTORY_FILE,
};

const DEFAULT_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const SECTOR_SIZE: u16 = 512;
const VOLUME_LABEL: &str = "ShelDrive";
const FILESYSTEM_NAME: &str = "ShelDrive";

/// WinFSP-specific file context: tracks an open handle to a file or directory.
#[derive(Debug, Clone)]
pub struct FileContext {
    pub path: String,
    pub is_directory: bool,
    pub ino: u64,
}

pub struct ShelDriveFS {
    db: Mutex<Connection>,
    bridge: Arc<ShelbyBridge>,
    disk_cache: DiskCache,
    staging: StagingArea,
    encryption_key: Option<String>,
    dirty_inodes: Mutex<HashSet<u64>>,
    path_to_ino: Mutex<HashMap<String, u64>>,
    next_ino: Mutex<u64>,
    /// Inodes flagged for deletion on cleanup
    pending_delete: Mutex<HashSet<u64>>,
}

impl ShelDriveFS {
    pub fn new(
        db_path: &str,
        bridge: Arc<ShelbyBridge>,
        encryption_key: Option<String>,
    ) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        schema::initialize(&conn)?;

        let fs = Self {
            db: Mutex::new(conn),
            bridge,
            disk_cache: DiskCache::new(DEFAULT_CACHE_MAX_BYTES),
            staging: StagingArea::new(),
            encryption_key,
            dirty_inodes: Mutex::new(HashSet::new()),
            path_to_ino: Mutex::new(HashMap::new()),
            next_ino: Mutex::new(2), // 1 reserved for root
            pending_delete: Mutex::new(HashSet::new()),
        };

        // Seed root
        fs.path_to_ino.lock().unwrap().insert("\\".to_string(), 1);
        Ok(fs)
    }

    fn alloc_ino(&self, path: &str) -> u64 {
        let mut map = self.path_to_ino.lock().unwrap();
        if let Some(&ino) = map.get(path) {
            return ino;
        }
        let mut next = self.next_ino.lock().unwrap();
        let ino = *next;
        *next += 1;
        map.insert(path.to_string(), ino);
        ino
    }

    fn now_iso() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// Convert Windows path (`\foo\bar.txt`) to our internal Unix-like path (`/foo/bar.txt`).
    fn to_internal_path(win_path: &str) -> String {
        let p = win_path.replace('\\', "/");
        if p.is_empty() || p == "/" {
            "/".to_string()
        } else if p.starts_with('/') {
            p
        } else {
            format!("/{}", p)
        }
    }

    fn u16_to_string(s: &U16CStr) -> String {
        s.to_string().unwrap_or_default()
    }

    fn fill_file_info_for_directory(info: &mut FileInfo) {
        info.file_attributes = FILE_ATTRIBUTE_DIRECTORY.0;
        info.file_size = 0;
        info.allocation_size = 0;
        let now = Self::windows_time_now();
        info.creation_time = now;
        info.last_access_time = now;
        info.last_write_time = now;
        info.change_time = now;
    }

    fn fill_file_info_for_file(info: &mut FileInfo, entry: &FileEntry) {
        info.file_attributes = FILE_ATTRIBUTE_NORMAL.0;
        info.file_size = entry.size_bytes as u64;
        info.allocation_size = ((entry.size_bytes as u64 + SECTOR_SIZE as u64 - 1)
            / SECTOR_SIZE as u64)
            * SECTOR_SIZE as u64;
        let t = Self::windows_time_now();
        info.creation_time = t;
        info.last_access_time = t;
        info.last_write_time = t;
        info.change_time = t;
    }

    /// Windows FILETIME (100ns intervals since Jan 1 1601).
    fn windows_time_now() -> u64 {
        use std::time::SystemTime;
        let unix = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        // 11644473600 = seconds between 1601 and 1970
        (unix + 11644473600) * 10_000_000
    }

    /// Flush dirty content for an inode to Shelby and update the CID in SQLite.
    fn sync_to_shelby(&self, ino: u64, path: &str) {
        let is_dirty = self.dirty_inodes.lock().unwrap().remove(&ino);
        if !is_dirty {
            return;
        }

        let content = match self.staging.read_all(ino) {
            Some(c) if !c.is_empty() => c,
            _ => return,
        };

        // Safety
        let verdict = safety::scan_image(path, &content);
        if verdict.is_blocked() {
            warn!("Content blocked: {} — {}", path, verdict.reason());
            self.staging.remove(ino);
            let conn = self.db.lock().unwrap();
            let blocked_cid = format!("blocked:{}", verdict.reason());
            let _ = index::update_file_cid(&conn, path, &blocked_cid, 0, &Self::now_iso());
            return;
        }

        // Encrypt with per-folder key
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
        let filename = path.rsplit('/').next().unwrap_or(path);

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
                let local_cid = format!(
                    "local:{}",
                    hex::encode(&sha2::Sha256::digest(path.as_bytes())[..16])
                );
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

// ---------------------------------------------------------------------------
// FileSystemContext trait implementation
// ---------------------------------------------------------------------------

impl FileSystemContext for ShelDriveFS {
    type FileContext = FileContext;

    fn get_security_by_name(
        &self,
        file_name: &U16CStr,
        _security_descriptor: Option<&mut [c_void]>,
        _reparse_point_resolver: impl FnOnce(&U16CStr) -> Option<FileSecurity>,
    ) -> FspResult<FileSecurity> {
        let path = Self::to_internal_path(&Self::u16_to_string(file_name));
        let conn = self.db.lock().unwrap();

        let is_directory = path == "/" || index::get_directory(&conn, &path).is_ok();
        let exists = is_directory || index::get_file_by_path(&conn, &path).is_ok();

        if !exists {
            return Err(FspError::from(STATUS_OBJECT_NAME_NOT_FOUND));
        }

        Ok(FileSecurity {
            reparse: false,
            sz_security_descriptor: 0,
            attributes: if is_directory {
                FILE_ATTRIBUTE_DIRECTORY.0
            } else {
                FILE_ATTRIBUTE_NORMAL.0
            },
        })
    }

    fn open(
        &self,
        file_name: &U16CStr,
        _create_options: u32,
        _granted_access: u32,
        file_info: &mut OpenFileInfo,
    ) -> FspResult<Self::FileContext> {
        let path = Self::to_internal_path(&Self::u16_to_string(file_name));
        debug!("open: {}", path);
        let conn = self.db.lock().unwrap();

        let info = file_info.as_mut();

        // Root or directory?
        if path == "/" {
            Self::fill_file_info_for_directory(info);
            let ino = self.alloc_ino(&path);
            return Ok(FileContext {
                path,
                is_directory: true,
                ino,
            });
        }

        if index::get_directory(&conn, &path).is_ok() {
            Self::fill_file_info_for_directory(info);
            let ino = self.alloc_ino(&path);
            return Ok(FileContext {
                path,
                is_directory: true,
                ino,
            });
        }

        if let Ok(entry) = index::get_file_by_path(&conn, &path) {
            Self::fill_file_info_for_file(info, &entry);
            let ino = self.alloc_ino(&path);
            return Ok(FileContext {
                path,
                is_directory: false,
                ino,
            });
        }

        Err(FspError::from(STATUS_OBJECT_NAME_NOT_FOUND))
    }

    fn close(&self, context: Self::FileContext) {
        debug!("close: {}", context.path);
        // Flush any dirty content on close
        if !context.is_directory {
            self.sync_to_shelby(context.ino, &context.path);
        }
    }

    fn create(
        &self,
        file_name: &U16CStr,
        create_options: u32,
        _granted_access: u32,
        _file_attributes: u32,
        _security_descriptor: Option<&[c_void]>,
        _allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        _extra_buffer_is_reparse_point: bool,
        file_info: &mut OpenFileInfo,
    ) -> FspResult<Self::FileContext> {
        let path = Self::to_internal_path(&Self::u16_to_string(file_name));
        let is_dir = (create_options & FILE_DIRECTORY_FILE.0) != 0;
        debug!("create: {} (dir={})", path, is_dir);

        // Safety: block dangerous file types
        if !is_dir && safety::check_extension(&path).is_blocked() {
            return Err(FspError::from(STATUS_ACCESS_DENIED));
        }

        let conn = self.db.lock().unwrap();

        if is_dir {
            if index::get_directory(&conn, &path).is_ok() {
                return Err(FspError::from(STATUS_OBJECT_NAME_COLLISION));
            }
            index::insert_directory(&conn, &path, &Self::now_iso())
                .map_err(|_| FspError::from(STATUS_ACCESS_DENIED))?;

            // Generate per-folder encryption key
            if let Some(ref pk) = self.encryption_key {
                let folder_key = crypto::generate_folder_key();
                if let Ok(wrapped) = crypto::wrap_folder_key(&folder_key, pk) {
                    let owner_addr = hex::encode(&sha2::Sha256::digest(pk.as_bytes())[..20]);
                    let fk = index::FolderKey {
                        folder_path: path.clone(),
                        encrypted_key: wrapped,
                        owner_address: owner_addr,
                        created_at: Self::now_iso(),
                        rotated_at: None,
                    };
                    let _ = index::insert_folder_key(&conn, &fk);
                }
            }

            Self::fill_file_info_for_directory(file_info.as_mut());
            let ino = self.alloc_ino(&path);
            Ok(FileContext {
                path,
                is_directory: true,
                ino,
            })
        } else {
            if index::get_file_by_path(&conn, &path).is_ok() {
                return Err(FspError::from(STATUS_OBJECT_NAME_COLLISION));
            }
            let entry = FileEntry {
                id: 0,
                path: path.clone(),
                cid: "pending".to_string(),
                size_bytes: 0,
                mime_type: None,
                pinned_at: Self::now_iso(),
                modified_at: Self::now_iso(),
                metadata: None,
            };
            index::insert_file(&conn, &entry).map_err(|_| FspError::from(STATUS_ACCESS_DENIED))?;

            Self::fill_file_info_for_file(file_info.as_mut(), &entry);
            let ino = self.alloc_ino(&path);
            Ok(FileContext {
                path,
                is_directory: false,
                ino,
            })
        }
    }

    fn cleanup(&self, context: &Self::FileContext, _file_name: Option<&U16CStr>, flags: u32) {
        debug!("cleanup: {} (flags={:x})", context.path, flags);
        // WinFSP `FspCleanupDelete` flag = 0x01
        if (flags & 0x01) != 0 && self.pending_delete.lock().unwrap().contains(&context.ino) {
            self.do_delete(context);
        }
    }

    fn flush(
        &self,
        context: Option<&Self::FileContext>,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if let Some(ctx) = context {
            if !ctx.is_directory {
                self.sync_to_shelby(ctx.ino, &ctx.path);
                let conn = self.db.lock().unwrap();
                if let Ok(entry) = index::get_file_by_path(&conn, &ctx.path) {
                    Self::fill_file_info_for_file(file_info, &entry);
                }
            }
        }
        Ok(())
    }

    fn get_file_info(
        &self,
        context: &Self::FileContext,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if context.is_directory {
            Self::fill_file_info_for_directory(file_info);
        } else {
            let conn = self.db.lock().unwrap();
            match index::get_file_by_path(&conn, &context.path) {
                Ok(entry) => Self::fill_file_info_for_file(file_info, &entry),
                Err(_) => return Err(FspError::from(STATUS_OBJECT_NAME_NOT_FOUND)),
            }
        }
        Ok(())
    }

    fn read(
        &self,
        context: &Self::FileContext,
        buffer: &mut [u8],
        offset: u64,
    ) -> FspResult<u32> {
        if context.is_directory {
            return Err(FspError::from(STATUS_FILE_IS_A_DIRECTORY));
        }

        let cid = {
            let conn = self.db.lock().unwrap();
            index::get_file_by_path(&conn, &context.path)
                .map(|e| e.cid)
                .unwrap_or_default()
        };

        let data = self
            .fetch_content(context.ino, &context.path, &cid)
            .unwrap_or_default();

        let start = offset as usize;
        if start >= data.len() {
            return Err(FspError::from(STATUS_END_OF_FILE));
        }

        let end = (start + buffer.len()).min(data.len());
        let n = end - start;
        buffer[..n].copy_from_slice(&data[start..end]);
        Ok(n as u32)
    }

    fn write(
        &self,
        context: &Self::FileContext,
        buffer: &[u8],
        offset: u64,
        write_to_eof: bool,
        _constrained_io: bool,
        file_info: &mut FileInfo,
    ) -> FspResult<u32> {
        if context.is_directory {
            return Err(FspError::from(STATUS_FILE_IS_A_DIRECTORY));
        }

        let actual_offset = if write_to_eof {
            self.staging.size(context.ino).unwrap_or(0)
        } else {
            offset
        };

        // Safety: size limit
        let projected = actual_offset + buffer.len() as u64;
        if safety::check_size(projected).is_blocked() {
            return Err(FspError::from(STATUS_ACCESS_DENIED));
        }

        let n = self
            .staging
            .write(context.ino, actual_offset, buffer)
            .map_err(|_| FspError::from(STATUS_ACCESS_DENIED))? as u32;

        self.dirty_inodes.lock().unwrap().insert(context.ino);

        let new_size = self.staging.size(context.ino).unwrap_or(0) as i64;
        let conn = self.db.lock().unwrap();
        let _ = index::update_file_cid(
            &conn,
            &context.path,
            "pending",
            new_size,
            &Self::now_iso(),
        );
        if let Ok(entry) = index::get_file_by_path(&conn, &context.path) {
            Self::fill_file_info_for_file(file_info, &entry);
        }

        Ok(n)
    }

    fn overwrite(
        &self,
        context: &Self::FileContext,
        _file_attributes: u32,
        _replace_file_attributes: bool,
        allocation_size: u64,
        _extra_buffer: Option<&[u8]>,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if context.is_directory {
            return Err(FspError::from(STATUS_FILE_IS_A_DIRECTORY));
        }
        let _ = self.staging.truncate(context.ino, allocation_size);
        self.dirty_inodes.lock().unwrap().insert(context.ino);

        let conn = self.db.lock().unwrap();
        let _ = index::update_file_cid(
            &conn,
            &context.path,
            "pending",
            allocation_size as i64,
            &Self::now_iso(),
        );
        if let Ok(entry) = index::get_file_by_path(&conn, &context.path) {
            Self::fill_file_info_for_file(file_info, &entry);
        }
        Ok(())
    }

    fn read_directory(
        &self,
        context: &Self::FileContext,
        _pattern: Option<&U16CStr>,
        _marker: DirMarker,
        buffer: &mut [u8],
    ) -> FspResult<u32> {
        if !context.is_directory {
            return Err(FspError::from(STATUS_NOT_A_DIRECTORY));
        }

        let conn = self.db.lock().unwrap();
        let items = index::list_directory(&conn, &context.path)
            .map_err(|_| FspError::from(STATUS_ACCESS_DENIED))?;
        drop(conn);

        let mut cursor = 0u32;
        for item in &items {
            let (name, file_info) = match item {
                DirItem::Directory(d) => {
                    let name = d.path.rsplit('/').next().unwrap_or(&d.path).to_string();
                    let mut info = FileInfo::default();
                    Self::fill_file_info_for_directory(&mut info);
                    (name, info)
                }
                DirItem::File(f) => {
                    let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                    let mut info = FileInfo::default();
                    Self::fill_file_info_for_file(&mut info, f);
                    (name, info)
                }
            };

            let mut dir_info: DirInfo<255> = DirInfo::new();
            *dir_info.file_info_mut() = file_info;
            let name_wide: Vec<u16> = name.encode_utf16().collect();
            let _ = dir_info.set_name_raw(name_wide.as_slice());
            if !dir_info.append_to_buffer(buffer, &mut cursor) {
                break;
            }
        }
        // Signal end of directory
        DirInfo::<255>::finalize_buffer(buffer, &mut cursor);
        Ok(cursor)
    }

    fn rename(
        &self,
        _context: &Self::FileContext,
        file_name: &U16CStr,
        new_file_name: &U16CStr,
        replace_if_exists: bool,
    ) -> FspResult<()> {
        let old_path = Self::to_internal_path(&Self::u16_to_string(file_name));
        let new_path = Self::to_internal_path(&Self::u16_to_string(new_file_name));
        debug!("rename: {} -> {}", old_path, new_path);

        let conn = self.db.lock().unwrap();
        if !replace_if_exists && index::get_file_by_path(&conn, &new_path).is_ok() {
            return Err(FspError::from(STATUS_OBJECT_NAME_COLLISION));
        }

        // Best-effort SQL rename via delete+insert of file entry
        if let Ok(entry) = index::get_file_by_path(&conn, &old_path) {
            let _ = index::delete_file_by_path(&conn, &old_path);
            let new_entry = FileEntry {
                path: new_path.clone(),
                ..entry
            };
            index::insert_file(&conn, &new_entry)
                .map_err(|_| FspError::from(STATUS_ACCESS_DENIED))?;

            let mut map = self.path_to_ino.lock().unwrap();
            if let Some(ino) = map.remove(&old_path) {
                map.insert(new_path, ino);
            }
            Ok(())
        } else {
            Err(FspError::from(STATUS_OBJECT_NAME_NOT_FOUND))
        }
    }

    fn set_basic_info(
        &self,
        context: &Self::FileContext,
        _file_attributes: u32,
        _creation_time: u64,
        _last_access_time: u64,
        _last_write_time: u64,
        _last_change_time: u64,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        self.get_file_info(context, file_info)
    }

    fn set_delete(
        &self,
        context: &Self::FileContext,
        _file_name: &U16CStr,
        delete_file: bool,
    ) -> FspResult<()> {
        if delete_file {
            // For directories, check empty
            if context.is_directory {
                let conn = self.db.lock().unwrap();
                if let Ok(items) = index::list_directory(&conn, &context.path) {
                    if !items.is_empty() {
                        return Err(FspError::from(STATUS_DIRECTORY_NOT_EMPTY));
                    }
                }
            }
            self.pending_delete.lock().unwrap().insert(context.ino);
        } else {
            self.pending_delete.lock().unwrap().remove(&context.ino);
        }
        Ok(())
    }

    fn set_file_size(
        &self,
        context: &Self::FileContext,
        new_size: u64,
        _set_allocation_size: bool,
        file_info: &mut FileInfo,
    ) -> FspResult<()> {
        if context.is_directory {
            return Err(FspError::from(STATUS_FILE_IS_A_DIRECTORY));
        }
        let _ = self.staging.truncate(context.ino, new_size);
        if new_size > 0 {
            self.dirty_inodes.lock().unwrap().insert(context.ino);
        }
        let conn = self.db.lock().unwrap();
        let _ = index::update_file_cid(
            &conn,
            &context.path,
            "pending",
            new_size as i64,
            &Self::now_iso(),
        );
        if let Ok(entry) = index::get_file_by_path(&conn, &context.path) {
            Self::fill_file_info_for_file(file_info, &entry);
        }
        Ok(())
    }

    fn get_volume_info(&self, out_volume_info: &mut VolumeInfo) -> FspResult<()> {
        // 1TB virtual capacity
        let total: u64 = 1024 * 1024 * 1024 * 1024;
        let used: u64 = self.staging.used_bytes().unwrap_or(0);
        out_volume_info.total_size = total;
        out_volume_info.free_size = total.saturating_sub(used);
        out_volume_info.set_volume_label(VOLUME_LABEL);
        Ok(())
    }

    fn set_volume_label(
        &self,
        _volume_label: &U16CStr,
        volume_info: &mut VolumeInfo,
    ) -> FspResult<()> {
        self.get_volume_info(volume_info)
    }

    fn get_security(
        &self,
        _context: &Self::FileContext,
        _security_descriptor: Option<&mut [c_void]>,
    ) -> FspResult<u64> {
        // Minimal: no security descriptor
        Ok(0)
    }

    fn set_security(
        &self,
        _context: &Self::FileContext,
        _security_information: u32,
        _modification_descriptor: ModificationDescriptor,
    ) -> FspResult<()> {
        Ok(())
    }
}

impl ShelDriveFS {
    fn do_delete(&self, context: &FileContext) {
        debug!("delete: {}", context.path);
        let conn = self.db.lock().unwrap();

        if context.is_directory {
            let _ = index::delete_directory(&conn, &context.path);
        } else {
            // Unpin from Shelby (best effort)
            if let Ok(entry) = index::get_file_by_path(&conn, &context.path) {
                if !entry.cid.starts_with("stub:")
                    && !entry.cid.starts_with("local:")
                    && entry.cid != "pending"
                {
                    let _ = self.bridge.unpin(&entry.cid);
                }
            }
            let _ = index::delete_file_by_path(&conn, &context.path);
            self.staging.remove(context.ino);
        }

        self.path_to_ino.lock().unwrap().remove(&context.path);
        self.dirty_inodes.lock().unwrap().remove(&context.ino);
        self.pending_delete.lock().unwrap().remove(&context.ino);
    }
}

// ---------------------------------------------------------------------------
// Mount control
// ---------------------------------------------------------------------------

pub struct MountHandle {
    _fs: FileSystemHost<ShelDriveFS>,
    _init: winfsp::FspInit,
}

pub fn mount(
    mount_point: &str,
    db_path: &str,
    bridge: Arc<ShelbyBridge>,
    encryption_key: Option<String>,
) -> Result<MountHandle, String> {
    // Initialize WinFSP DLL
    let init = winfsp::winfsp_init().map_err(|e| format!("WinFSP init failed: {:?}", e))?;

    let fs = ShelDriveFS::new(db_path, bridge, encryption_key)
        .map_err(|e| format!("Failed to initialize filesystem: {}", e))?;

    let mut volume_params = VolumeParams::new();
    volume_params
        .sector_size(SECTOR_SIZE)
        .sectors_per_allocation_unit(1)
        .max_component_length(255)
        .volume_serial_number(0x5EDD0000)
        .file_info_timeout(1000)
        .case_sensitive_search(false)
        .case_preserved_names(true)
        .unicode_on_disk(true)
        .persistent_acls(false)
        .post_cleanup_when_modified_only(true)
        .pass_query_directory_pattern(true)
        .filesystem_name(FILESYSTEM_NAME);

    let mut host = FileSystemHost::new(volume_params, fs)
        .map_err(|e| format!("Failed to create FileSystemHost: {:?}", e))?;

    host.mount(mount_point)
        .map_err(|e| format!("Failed to mount at {}: {:?}", mount_point, e))?;

    host.start()
        .map_err(|e| format!("Failed to start dispatcher: {:?}", e))?;

    info!("WinFSP mounted at {}", mount_point);

    Ok(MountHandle {
        _fs: host,
        _init: init,
    })
}

impl Drop for MountHandle {
    fn drop(&mut self) {
        self._fs.stop();
        self._fs.unmount();
    }
}
