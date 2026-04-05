use crate::bridge::shelby::ShelbyBridge;
use crate::cache::DiskCache;
use crate::db::index::{
    self, delete_directory, delete_file_by_path, get_directory, get_file_by_path, insert_directory,
    insert_file, list_directory, DirItem, FileEntry,
};
use crate::db::schema;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use fuser::{
    BsdFileFlags, Config, Errno, FileAttr, FileHandle, FileType, Filesystem, FopenFlags,
    Generation, INodeNo, LockOwner, MountOption, OpenFlags, ReplyAttr, ReplyCreate, ReplyData,
    ReplyDirectory, ReplyEmpty, ReplyEntry, ReplyOpen, ReplyWrite, Request, Session, TimeOrNow,
    WriteFlags,
};
use log::{debug, error, info, warn};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

const TTL: Duration = Duration::from_secs(1);
const BLOCK_SIZE: u32 = 512;
const ROOT_INO: u64 = 1;
const FILE_INO_OFFSET: u64 = 0x1_0000_0000;

/// Default cache size: 512 MB
const DEFAULT_CACHE_MAX_BYTES: u64 = 512 * 1024 * 1024;

pub struct ShelDriveFS {
    db: Mutex<Connection>,
    bridge: Arc<ShelbyBridge>,
    disk_cache: DiskCache,
    dir_ino_map: Mutex<HashMap<u64, String>>,
    file_ino_map: Mutex<HashMap<u64, String>>,
    path_ino_map: Mutex<HashMap<String, u64>>,
    next_dir_ino: Mutex<u64>,
    next_file_ino: Mutex<u64>,
    /// In-memory write buffer — content is held here until flushed to Shelby
    file_content: Mutex<HashMap<u64, Vec<u8>>>,
    /// Inodes that have been written to but not yet synced to Shelby
    dirty_inodes: Mutex<HashSet<u64>>,
    next_fh: Mutex<u64>,
}

impl ShelDriveFS {
    pub fn new(db_path: &str, bridge: Arc<ShelbyBridge>) -> rusqlite::Result<Self> {
        let conn = Connection::open(db_path)?;
        schema::initialize(&conn)?;

        let fs = Self {
            db: Mutex::new(conn),
            bridge,
            disk_cache: DiskCache::new(DEFAULT_CACHE_MAX_BYTES),
            dir_ino_map: Mutex::new(HashMap::new()),
            file_ino_map: Mutex::new(HashMap::new()),
            path_ino_map: Mutex::new(HashMap::new()),
            next_dir_ino: Mutex::new(2),
            next_file_ino: Mutex::new(FILE_INO_OFFSET),
            file_content: Mutex::new(HashMap::new()),
            dirty_inodes: Mutex::new(HashSet::new()),
            next_fh: Mutex::new(1),
        };

        fs.dir_ino_map
            .lock()
            .unwrap()
            .insert(ROOT_INO, "/".to_string());
        fs.path_ino_map
            .lock()
            .unwrap()
            .insert("/".to_string(), ROOT_INO);
        fs.load_index_into_maps();

        Ok(fs)
    }

    fn load_index_into_maps(&self) {
        let conn = self.db.lock().unwrap();

        let mut stmt = conn
            .prepare("SELECT path FROM directories WHERE path != '/'")
            .unwrap();
        let dir_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for path in dir_paths {
            self.alloc_dir_ino(&path);
        }

        let mut stmt = conn.prepare("SELECT path FROM files").unwrap();
        let file_paths: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for path in file_paths {
            self.alloc_file_ino(&path);
        }
    }

    fn alloc_dir_ino(&self, path: &str) -> u64 {
        let mut path_map = self.path_ino_map.lock().unwrap();
        if let Some(&ino) = path_map.get(path) {
            return ino;
        }
        let mut next = self.next_dir_ino.lock().unwrap();
        let ino = *next;
        *next += 1;
        self.dir_ino_map
            .lock()
            .unwrap()
            .insert(ino, path.to_string());
        path_map.insert(path.to_string(), ino);
        ino
    }

    fn alloc_file_ino(&self, path: &str) -> u64 {
        let mut path_map = self.path_ino_map.lock().unwrap();
        if let Some(&ino) = path_map.get(path) {
            return ino;
        }
        let mut next = self.next_file_ino.lock().unwrap();
        let ino = *next;
        *next += 1;
        self.file_ino_map
            .lock()
            .unwrap()
            .insert(ino, path.to_string());
        path_map.insert(path.to_string(), ino);
        ino
    }

    fn resolve_path(&self, parent_ino: u64, name: &OsStr) -> Option<String> {
        let name_str = name.to_str()?;
        let dir_map = self.dir_ino_map.lock().unwrap();
        let parent_path = dir_map.get(&parent_ino)?;
        let child_path = if parent_path == "/" {
            format!("/{}", name_str)
        } else {
            format!("{}/{}", parent_path, name_str)
        };
        Some(child_path)
    }

    fn make_dir_attr(&self, ino: u64) -> FileAttr {
        let now = SystemTime::now();
        FileAttr {
            ino: INodeNo(ino),
            size: 0,
            blocks: 0,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::Directory,
            perm: 0o755,
            nlink: 2,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: BLOCK_SIZE,
            flags: 0,
        }
    }

    fn make_file_attr(&self, ino: u64, entry: &FileEntry) -> FileAttr {
        let now = SystemTime::now();
        let size = entry.size_bytes as u64;
        FileAttr {
            ino: INodeNo(ino),
            size,
            blocks: (size + BLOCK_SIZE as u64 - 1) / BLOCK_SIZE as u64,
            atime: now,
            mtime: now,
            ctime: now,
            crtime: now,
            kind: FileType::RegularFile,
            perm: 0o644,
            nlink: 1,
            uid: unsafe { libc::getuid() },
            gid: unsafe { libc::getgid() },
            rdev: 0,
            blksize: BLOCK_SIZE,
            flags: 0,
        }
    }

    fn now_iso() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// Flush dirty content for an inode to Shelby and update the CID in SQLite.
    fn sync_to_shelby(&self, ino: u64) {
        let is_dirty = self.dirty_inodes.lock().unwrap().remove(&ino);
        if !is_dirty {
            return;
        }

        let path = match self.file_ino_map.lock().unwrap().get(&ino).cloned() {
            Some(p) => p,
            None => return,
        };

        let content = match self.file_content.lock().unwrap().get(&ino).cloned() {
            Some(c) => c,
            None => return,
        };

        if content.is_empty() {
            return;
        }

        let content_b64 = BASE64.encode(&content);
        let filename = path.rsplit('/').next().unwrap_or(&path);

        info!("Pinning {} ({} bytes) to Shelby...", path, content.len());

        match self.bridge.pin(&content_b64, Some(filename)) {
            Ok(result) => {
                info!("Pinned {} → CID: {}", path, result.cid);
                // Persist to disk cache
                self.disk_cache.put(&result.cid, &content);
                let conn = self.db.lock().unwrap();
                let _ = index::update_file_cid(
                    &conn,
                    &path,
                    &result.cid,
                    result.size_bytes,
                    &Self::now_iso(),
                );
            }
            Err(e) => {
                error!("Failed to pin {} to Shelby: {}", path, e);
                // Still save to disk cache with a local CID so content isn't lost
                let local_cid = format!("local:{:x}", fnv_hash(path.as_bytes()));
                self.disk_cache.put(&local_cid, &content);
                let conn = self.db.lock().unwrap();
                let _ = index::update_file_cid(
                    &conn,
                    &path,
                    &local_cid,
                    content.len() as i64,
                    &Self::now_iso(),
                );
                // Mark dirty again so it retries on next flush
                self.dirty_inodes.lock().unwrap().insert(ino);
            }
        }
    }

    /// Retrieve file content — checks memory cache, disk cache, then Shelby network.
    fn fetch_content(&self, ino: u64, path: &str, cid: &str) -> Option<Vec<u8>> {
        if cid == "pending" {
            return None;
        }

        // 1. Disk cache
        if let Some(data) = self.disk_cache.get(cid) {
            debug!("Disk cache hit for {} (CID: {})", path, cid);
            self.file_content.lock().unwrap().insert(ino, data.clone());
            return Some(data);
        }

        // 2. Local-only CIDs can't be fetched from network
        if cid.starts_with("stub:") || cid.starts_with("local:") {
            return None;
        }

        // 3. Shelby network
        info!("Retrieving {} (CID: {}) from Shelby...", path, cid);
        match self.bridge.retrieve(cid) {
            Ok(result) => {
                let data = BASE64.decode(&result.content).unwrap_or_default();
                info!("Retrieved {} ({} bytes)", path, data.len());
                // Cache in memory and on disk
                self.file_content.lock().unwrap().insert(ino, data.clone());
                self.disk_cache.put(cid, &data);
                Some(data)
            }
            Err(e) => {
                warn!("Failed to retrieve {} from Shelby: {}", path, e);
                None
            }
        }
    }
}

impl Filesystem for ShelDriveFS {
    fn lookup(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEntry) {
        let parent_raw = u64::from(parent);
        debug!("lookup: parent={}, name={:?}", parent_raw, name);

        let child_path = match self.resolve_path(parent_raw, name) {
            Some(p) => p,
            None => {
                reply.error(Errno::ENOENT);
                return;
            }
        };

        let conn = self.db.lock().unwrap();

        if get_directory(&conn, &child_path).is_ok() {
            let ino = self.alloc_dir_ino(&child_path);
            reply.entry(&TTL, &self.make_dir_attr(ino), Generation(0));
            return;
        }

        if let Ok(entry) = get_file_by_path(&conn, &child_path) {
            let ino = self.alloc_file_ino(&child_path);
            reply.entry(&TTL, &self.make_file_attr(ino, &entry), Generation(0));
            return;
        }

        reply.error(Errno::ENOENT);
    }

    fn getattr(&self, _req: &Request, ino: INodeNo, _fh: Option<FileHandle>, reply: ReplyAttr) {
        let ino_raw = u64::from(ino);

        if self.dir_ino_map.lock().unwrap().contains_key(&ino_raw) {
            reply.attr(&TTL, &self.make_dir_attr(ino_raw));
            return;
        }

        if let Some(path) = self.file_ino_map.lock().unwrap().get(&ino_raw).cloned() {
            let conn = self.db.lock().unwrap();
            if let Ok(entry) = get_file_by_path(&conn, &path) {
                reply.attr(&TTL, &self.make_file_attr(ino_raw, &entry));
                return;
            }
        }

        reply.error(Errno::ENOENT);
    }

    fn readdir(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        mut reply: ReplyDirectory,
    ) {
        let ino_raw = u64::from(ino);

        let dir_path = match self.dir_ino_map.lock().unwrap().get(&ino_raw).cloned() {
            Some(p) => p,
            None => {
                reply.error(Errno::ENOENT);
                return;
            }
        };

        let conn = self.db.lock().unwrap();
        let items = match list_directory(&conn, &dir_path) {
            Ok(items) => items,
            Err(e) => {
                error!("readdir failed: {}", e);
                reply.error(Errno::EIO);
                return;
            }
        };

        let mut entries: Vec<(u64, FileType, String)> = Vec::new();

        entries.push((ino_raw, FileType::Directory, ".".to_string()));
        let parent_ino = if ino_raw == ROOT_INO {
            ROOT_INO
        } else {
            let parent_path = {
                let parts: Vec<&str> = dir_path.rsplitn(2, '/').collect();
                if parts.len() == 2 && !parts[1].is_empty() {
                    parts[1].to_string()
                } else {
                    "/".to_string()
                }
            };
            *self
                .path_ino_map
                .lock()
                .unwrap()
                .get(&parent_path)
                .unwrap_or(&ROOT_INO)
        };
        entries.push((parent_ino, FileType::Directory, "..".to_string()));

        for item in items {
            match item {
                DirItem::Directory(d) => {
                    let child_ino = self.alloc_dir_ino(&d.path);
                    let name = d.path.rsplit('/').next().unwrap_or(&d.path).to_string();
                    entries.push((child_ino, FileType::Directory, name));
                }
                DirItem::File(f) => {
                    let child_ino = self.alloc_file_ino(&f.path);
                    let name = f.path.rsplit('/').next().unwrap_or(&f.path).to_string();
                    entries.push((child_ino, FileType::RegularFile, name));
                }
            }
        }

        for (i, (child_ino, kind, name)) in entries.iter().enumerate().skip(offset as usize) {
            if reply.add(INodeNo(*child_ino), (i + 1) as u64, *kind, name) {
                break;
            }
        }

        reply.ok();
    }

    fn open(&self, _req: &Request, ino: INodeNo, _flags: OpenFlags, reply: ReplyOpen) {
        let ino_raw = u64::from(ino);

        if !self.file_ino_map.lock().unwrap().contains_key(&ino_raw) {
            reply.error(Errno::ENOENT);
            return;
        }

        let mut fh = self.next_fh.lock().unwrap();
        let handle = *fh;
        *fh += 1;

        reply.opened(FileHandle(handle), FopenFlags::empty());
    }

    fn read(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        size: u32,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyData,
    ) {
        let ino_raw = u64::from(ino);

        // 1. Check in-memory cache
        {
            let content = self.file_content.lock().unwrap();
            if let Some(data) = content.get(&ino_raw) {
                let start = offset as usize;
                if start >= data.len() {
                    reply.data(&[]);
                } else {
                    let end = (start + size as usize).min(data.len());
                    reply.data(&data[start..end]);
                }
                return;
            }
        }

        // 2. Lookup CID from SQLite and retrieve from Shelby
        let path = self.file_ino_map.lock().unwrap().get(&ino_raw).cloned();
        if let Some(path) = path {
            let cid = {
                let conn = self.db.lock().unwrap();
                get_file_by_path(&conn, &path).ok().map(|e| e.cid)
            };

            if let Some(cid) = cid {
                if let Some(data) = self.fetch_content(ino_raw, &path, &cid) {
                    let start = offset as usize;
                    if start >= data.len() {
                        reply.data(&[]);
                    } else {
                        let end = (start + size as usize).min(data.len());
                        reply.data(&data[start..end]);
                    }
                    return;
                }

                // Fallback: file exists in index but content unavailable
                let placeholder = format!(
                    "[ShelDrive] Content unavailable — CID: {}\n",
                    cid
                );
                let data = placeholder.as_bytes();
                let start = offset as usize;
                if start >= data.len() {
                    reply.data(&[]);
                } else {
                    let end = (start + size as usize).min(data.len());
                    reply.data(&data[start..end]);
                }
                return;
            }
        }

        reply.error(Errno::ENOENT);
    }

    fn write(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        offset: u64,
        data: &[u8],
        _write_flags: WriteFlags,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        reply: ReplyWrite,
    ) {
        let ino_raw = u64::from(ino);

        let path = match self.file_ino_map.lock().unwrap().get(&ino_raw).cloned() {
            Some(p) => p,
            None => {
                reply.error(Errno::ENOENT);
                return;
            }
        };

        // Buffer writes in memory
        let mut content = self.file_content.lock().unwrap();
        let buf = content.entry(ino_raw).or_insert_with(Vec::new);
        let start = offset as usize;

        if start > buf.len() {
            buf.resize(start, 0);
        }
        let end = start + data.len();
        if end > buf.len() {
            buf.resize(end, 0);
        }
        buf[start..end].copy_from_slice(data);

        let new_size = buf.len() as i64;
        drop(content);

        // Mark dirty — will be synced to Shelby on release/flush
        self.dirty_inodes.lock().unwrap().insert(ino_raw);

        // Update size in SQLite (CID stays as "pending" until flush)
        let conn = self.db.lock().unwrap();
        let _ = index::update_file_cid(&conn, &path, "pending", new_size, &Self::now_iso());

        reply.written(data.len() as u32);
    }

    fn flush(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _lock_owner: LockOwner,
        reply: ReplyEmpty,
    ) {
        let ino_raw = u64::from(ino);
        debug!("flush: ino={}", ino_raw);
        self.sync_to_shelby(ino_raw);
        reply.ok();
    }

    fn release(
        &self,
        _req: &Request,
        ino: INodeNo,
        _fh: FileHandle,
        _flags: OpenFlags,
        _lock_owner: Option<LockOwner>,
        _flush: bool,
        reply: ReplyEmpty,
    ) {
        let ino_raw = u64::from(ino);
        debug!("release: ino={}", ino_raw);
        // Ensure any dirty content is flushed to Shelby on file close
        self.sync_to_shelby(ino_raw);
        reply.ok();
    }

    fn create(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        _flags: i32,
        reply: ReplyCreate,
    ) {
        let parent_raw = u64::from(parent);

        let child_path = match self.resolve_path(parent_raw, name) {
            Some(p) => p,
            None => {
                reply.error(Errno::EINVAL);
                return;
            }
        };

        let conn = self.db.lock().unwrap();
        if get_file_by_path(&conn, &child_path).is_ok() {
            reply.error(Errno::EEXIST);
            return;
        }

        let entry = FileEntry {
            id: 0,
            path: child_path.clone(),
            cid: "pending".to_string(),
            size_bytes: 0,
            mime_type: None,
            pinned_at: Self::now_iso(),
            modified_at: Self::now_iso(),
            metadata: None,
        };

        match insert_file(&conn, &entry) {
            Ok(_) => {
                let ino = self.alloc_file_ino(&child_path);
                let attr = self.make_file_attr(ino, &entry);

                let mut fh = self.next_fh.lock().unwrap();
                let handle = *fh;
                *fh += 1;

                reply.created(&TTL, &attr, Generation(0), FileHandle(handle), FopenFlags::empty());
            }
            Err(e) => {
                error!("create failed: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn mkdir(
        &self,
        _req: &Request,
        parent: INodeNo,
        name: &OsStr,
        _mode: u32,
        _umask: u32,
        reply: ReplyEntry,
    ) {
        let parent_raw = u64::from(parent);

        let child_path = match self.resolve_path(parent_raw, name) {
            Some(p) => p,
            None => {
                reply.error(Errno::EINVAL);
                return;
            }
        };

        let conn = self.db.lock().unwrap();
        if get_directory(&conn, &child_path).is_ok() {
            reply.error(Errno::EEXIST);
            return;
        }

        match insert_directory(&conn, &child_path, &Self::now_iso()) {
            Ok(_) => {
                let ino = self.alloc_dir_ino(&child_path);
                reply.entry(&TTL, &self.make_dir_attr(ino), Generation(0));
            }
            Err(e) => {
                error!("mkdir failed: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn unlink(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let parent_raw = u64::from(parent);

        let child_path = match self.resolve_path(parent_raw, name) {
            Some(p) => p,
            None => {
                reply.error(Errno::ENOENT);
                return;
            }
        };

        // Get CID before deleting, so we can unpin from Shelby
        let cid = {
            let conn = self.db.lock().unwrap();
            get_file_by_path(&conn, &child_path).ok().map(|e| e.cid)
        };

        // Unpin from Shelby (non-blocking — don't fail the delete if unpin fails)
        if let Some(ref cid) = cid {
            if !cid.starts_with("stub:") && cid != "pending" {
                info!("Unpinning {} (CID: {}) from Shelby...", child_path, cid);
                if let Err(e) = self.bridge.unpin(cid) {
                    warn!("Unpin failed for {}: {} — continuing with local delete", child_path, e);
                }
            }
        }

        let conn = self.db.lock().unwrap();
        match delete_file_by_path(&conn, &child_path) {
            Ok(1) => {
                if let Some(ino) = self.path_ino_map.lock().unwrap().remove(&child_path) {
                    self.file_ino_map.lock().unwrap().remove(&ino);
                    self.file_content.lock().unwrap().remove(&ino);
                    self.dirty_inodes.lock().unwrap().remove(&ino);
                }
                reply.ok();
            }
            Ok(_) => reply.error(Errno::ENOENT),
            Err(e) => {
                error!("unlink failed: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn rmdir(&self, _req: &Request, parent: INodeNo, name: &OsStr, reply: ReplyEmpty) {
        let parent_raw = u64::from(parent);

        let child_path = match self.resolve_path(parent_raw, name) {
            Some(p) => p,
            None => {
                reply.error(Errno::ENOENT);
                return;
            }
        };

        let conn = self.db.lock().unwrap();
        match list_directory(&conn, &child_path) {
            Ok(items) if !items.is_empty() => {
                reply.error(Errno::ENOTEMPTY);
                return;
            }
            Err(e) => {
                error!("rmdir list failed: {}", e);
                reply.error(Errno::EIO);
                return;
            }
            _ => {}
        }

        match delete_directory(&conn, &child_path) {
            Ok(1) => {
                if let Some(ino) = self.path_ino_map.lock().unwrap().remove(&child_path) {
                    self.dir_ino_map.lock().unwrap().remove(&ino);
                }
                reply.ok();
            }
            Ok(_) => reply.error(Errno::ENOENT),
            Err(e) => {
                error!("rmdir failed: {}", e);
                reply.error(Errno::EIO);
            }
        }
    }

    fn setattr(
        &self,
        _req: &Request,
        ino: INodeNo,
        _mode: Option<u32>,
        _uid: Option<u32>,
        _gid: Option<u32>,
        size: Option<u64>,
        _atime: Option<TimeOrNow>,
        _mtime: Option<TimeOrNow>,
        _ctime: Option<SystemTime>,
        _fh: Option<FileHandle>,
        _crtime: Option<SystemTime>,
        _chgtime: Option<SystemTime>,
        _bkuptime: Option<SystemTime>,
        _flags: Option<BsdFileFlags>,
        reply: ReplyAttr,
    ) {
        let ino_raw = u64::from(ino);

        if let Some(new_size) = size {
            if let Some(path) = self.file_ino_map.lock().unwrap().get(&ino_raw).cloned() {
                let mut content = self.file_content.lock().unwrap();
                let buf = content.entry(ino_raw).or_insert_with(Vec::new);
                buf.resize(new_size as usize, 0);
                let sz = buf.len() as i64;
                drop(content);

                if new_size > 0 {
                    self.dirty_inodes.lock().unwrap().insert(ino_raw);
                }

                let conn = self.db.lock().unwrap();
                let _ = index::update_file_cid(&conn, &path, "pending", sz, &Self::now_iso());

                if let Ok(entry) = get_file_by_path(&conn, &path) {
                    reply.attr(&TTL, &self.make_file_attr(ino_raw, &entry));
                    return;
                }
            }
        }

        if self.dir_ino_map.lock().unwrap().contains_key(&ino_raw) {
            reply.attr(&TTL, &self.make_dir_attr(ino_raw));
            return;
        }

        if let Some(path) = self.file_ino_map.lock().unwrap().get(&ino_raw).cloned() {
            let conn = self.db.lock().unwrap();
            if let Ok(entry) = get_file_by_path(&conn, &path) {
                reply.attr(&TTL, &self.make_file_attr(ino_raw, &entry));
                return;
            }
        }

        reply.error(Errno::ENOENT);
    }
}

fn fnv_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

// ---------------------------------------------------------------------------
// Mount/unmount
// ---------------------------------------------------------------------------

pub struct MountHandle {
    session: Session<ShelDriveFS>,
}

impl MountHandle {
    pub fn unmount(self) {
        drop(self.session);
    }
}

pub fn mount(
    mount_point: &str,
    db_path: &str,
    bridge: Arc<ShelbyBridge>,
) -> Result<MountHandle, String> {
    let mount_path = PathBuf::from(mount_point);

    if !mount_path.exists() {
        std::fs::create_dir_all(&mount_path)
            .map_err(|e| format!("Failed to create mount point: {}", e))?;
    }

    let fs = ShelDriveFS::new(db_path, bridge)
        .map_err(|e| format!("Failed to initialize filesystem: {}", e))?;

    let mut config = Config::default();
    config.mount_options = vec![
        MountOption::FSName("ShelDrive".to_string()),
        MountOption::AutoUnmount,
        MountOption::DefaultPermissions,
    ];

    let session = Session::new(fs, &mount_path, &config)
        .map_err(|e| format!("Failed to create FUSE session: {}", e))?;

    Ok(MountHandle { session })
}
