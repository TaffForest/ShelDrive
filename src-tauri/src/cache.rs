use log::{debug, info, warn};
use std::fs;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

/// On-disk LRU file cache at ~/.sheldrive/cache/
pub struct DiskCache {
    dir: PathBuf,
    max_bytes: u64,
}

impl DiskCache {
    pub fn new(max_bytes: u64) -> Self {
        let dir = dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".sheldrive")
            .join("cache");

        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("Failed to create cache dir {:?}: {}", dir, e);
        }

        Self { dir, max_bytes }
    }

    pub fn get(&self, cid: &str) -> Option<Vec<u8>> {
        let path = self.cid_path(cid);
        if path.exists() {
            match fs::read(&path) {
                Ok(data) => {
                    debug!("Cache hit: {}", cid);
                    let _ = filetime::set_file_mtime(&path, filetime::FileTime::now());
                    Some(data)
                }
                Err(e) => {
                    warn!("Cache read failed for {}: {}", cid, e);
                    None
                }
            }
        } else {
            None
        }
    }

    pub fn put(&self, cid: &str, data: &[u8]) {
        let path = self.cid_path(cid);
        match fs::write(&path, data) {
            Ok(()) => {
                debug!("Cached {} ({} bytes)", cid, data.len());
                self.evict_if_needed();
            }
            Err(e) => {
                warn!("Cache write failed for {}: {}", cid, e);
            }
        }
    }

    pub fn remove(&self, cid: &str) {
        let path = self.cid_path(cid);
        let _ = fs::remove_file(&path);
    }

    fn evict_if_needed(&self) {
        let mut entries = self.list_entries();
        let total: u64 = entries.iter().map(|(_, size, _)| *size).sum();

        if total <= self.max_bytes {
            return;
        }

        entries.sort_by_key(|(_, _, mtime)| *mtime);

        let mut current = total;
        for (path, size, _) in &entries {
            if current <= self.max_bytes {
                break;
            }
            if fs::remove_file(path).is_ok() {
                info!("Evicted cached file {:?} ({} bytes)", path, size);
                current -= size;
            }
        }
    }

    fn cid_path(&self, cid: &str) -> PathBuf {
        let safe_name: String = cid
            .chars()
            .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
            .collect();
        self.dir.join(safe_name)
    }

    fn list_entries(&self) -> Vec<(PathBuf, u64, u64)> {
        let mut entries = Vec::new();
        if let Ok(dir) = fs::read_dir(&self.dir) {
            for entry in dir.flatten() {
                if let Ok(meta) = entry.metadata() {
                    if meta.is_file() {
                        let mtime = meta
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                            .unwrap_or(0);
                        entries.push((entry.path(), meta.len(), mtime));
                    }
                }
            }
        }
        entries
    }
}

/// Disk-backed staging area for file writes.
/// Files are written here during FUSE write() calls, then moved to cache
/// or uploaded to Shelby on flush/release.
pub struct StagingArea {
    dir: PathBuf,
}

impl StagingArea {
    pub fn new() -> Self {
        let dir = dirs::home_dir()
            .expect("Could not determine home directory")
            .join(".sheldrive")
            .join("staging");

        if let Err(e) = fs::create_dir_all(&dir) {
            warn!("Failed to create staging dir {:?}: {}", dir, e);
        }

        Self { dir }
    }

    /// Write data at a given offset into the staging file for an inode.
    pub fn write(&self, ino: u64, offset: u64, data: &[u8]) -> std::io::Result<usize> {
        let path = self.ino_path(ino);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)?;
        file.seek(SeekFrom::Start(offset))?;
        file.write_all(data)?;
        Ok(data.len())
    }

    /// Read data from the staging file at a given offset.
    pub fn read(&self, ino: u64, offset: u64, size: u32) -> Option<Vec<u8>> {
        let path = self.ino_path(ino);
        if !path.exists() {
            return None;
        }
        let mut file = fs::File::open(&path).ok()?;
        let file_len = file.metadata().ok()?.len();
        if offset >= file_len {
            return Some(vec![]);
        }
        file.seek(SeekFrom::Start(offset)).ok()?;
        let read_size = size.min((file_len - offset) as u32) as usize;
        let mut buf = vec![0u8; read_size];
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// Read the entire staging file content.
    pub fn read_all(&self, ino: u64) -> Option<Vec<u8>> {
        let path = self.ino_path(ino);
        fs::read(&path).ok()
    }

    /// Get the size of the staging file.
    pub fn size(&self, ino: u64) -> Option<u64> {
        let path = self.ino_path(ino);
        fs::metadata(&path).ok().map(|m| m.len())
    }

    /// Check if a staging file exists for this inode.
    pub fn exists(&self, ino: u64) -> bool {
        self.ino_path(ino).exists()
    }

    /// Truncate a staging file to a given size.
    pub fn truncate(&self, ino: u64, size: u64) -> std::io::Result<()> {
        let path = self.ino_path(ino);
        let file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .open(&path)?;
        file.set_len(size)?;
        Ok(())
    }

    /// Remove the staging file for an inode.
    pub fn remove(&self, ino: u64) {
        let _ = fs::remove_file(self.ino_path(ino));
    }

    fn ino_path(&self, ino: u64) -> PathBuf {
        self.dir.join(format!("{:x}", ino))
    }
}
