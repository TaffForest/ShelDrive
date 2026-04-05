use log::{debug, info, warn};
use std::fs;
use std::path::{Path, PathBuf};

/// On-disk LRU file cache at ~/.sheldrive/cache/
/// Files are stored by CID hash to avoid filesystem-unsafe characters.
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

    /// Get cached content by CID.
    pub fn get(&self, cid: &str) -> Option<Vec<u8>> {
        let path = self.cid_path(cid);
        if path.exists() {
            match fs::read(&path) {
                Ok(data) => {
                    debug!("Cache hit: {}", cid);
                    // Touch the file to update mtime for LRU
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

    /// Store content in cache.
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

    /// Remove a CID from cache.
    pub fn remove(&self, cid: &str) {
        let path = self.cid_path(cid);
        let _ = fs::remove_file(&path);
    }

    /// Total size of cache directory in bytes.
    pub fn size_bytes(&self) -> u64 {
        self.list_entries()
            .iter()
            .map(|(_, size, _)| *size)
            .sum()
    }

    /// Evict oldest files until under max_bytes.
    fn evict_if_needed(&self) {
        let mut entries = self.list_entries();
        let total: u64 = entries.iter().map(|(_, size, _)| *size).sum();

        if total <= self.max_bytes {
            return;
        }

        // Sort by mtime ascending (oldest first)
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
        // Replace unsafe chars in CID for filesystem use
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
