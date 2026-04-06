use log::{info, warn};

/// Maximum file size in bytes (2 GB default)
pub const MAX_FILE_SIZE: u64 = 2 * 1024 * 1024 * 1024;

/// Blocked file extensions — dangerous executables and scripts
const BLOCKED_EXTENSIONS: &[&str] = &[
    "exe", "bat", "cmd", "scr", "dll", "com", "msi", "ps1", "vbs", "wsf", "cpl", "hta", "inf",
    "reg", "rgs", "sct", "shb", "sys", "pif", "application", "gadget", "msp", "mst", "csh",
    "ksh", "lnk", "ws", "wsc", "wsh",
];

/// Image extensions that get NSFW scanned
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "gif", "webp", "bmp", "tiff", "tif"];

#[derive(Debug, Clone)]
pub enum ContentVerdict {
    Safe,
    BlockedExtension(String),
    BlockedSize(u64),
    BlockedNsfw(f32),
}

impl ContentVerdict {
    pub fn is_blocked(&self) -> bool {
        !matches!(self, ContentVerdict::Safe)
    }

    pub fn reason(&self) -> String {
        match self {
            ContentVerdict::Safe => "safe".to_string(),
            ContentVerdict::BlockedExtension(ext) => format!("blocked file type: .{}", ext),
            ContentVerdict::BlockedSize(size) => {
                format!("file too large: {} MB (max {} MB)", size / (1024 * 1024), MAX_FILE_SIZE / (1024 * 1024))
            }
            ContentVerdict::BlockedNsfw(score) => format!("NSFW content detected (score: {:.2})", score),
        }
    }
}

/// Check if a file extension is blocked.
pub fn check_extension(path: &str) -> ContentVerdict {
    let ext = path
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_lowercase();

    if BLOCKED_EXTENSIONS.contains(&ext.as_str()) {
        warn!("Blocked file type: .{} ({})", ext, path);
        ContentVerdict::BlockedExtension(ext)
    } else {
        ContentVerdict::Safe
    }
}

/// Check if file size exceeds the limit.
pub fn check_size(size: u64) -> ContentVerdict {
    if size > MAX_FILE_SIZE {
        warn!("File too large: {} bytes (max {})", size, MAX_FILE_SIZE);
        ContentVerdict::BlockedSize(size)
    } else {
        ContentVerdict::Safe
    }
}

/// Check if a file is an image that needs NSFW scanning.
pub fn is_image(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_lowercase();
    IMAGE_EXTENSIONS.contains(&ext.as_str())
}

/// Scan image content for NSFW material.
/// Uses a simple heuristic check for now — ONNX model integration is Phase 2.
/// Returns Safe for non-image files.
pub fn scan_image(path: &str, data: &[u8]) -> ContentVerdict {
    if !is_image(path) || data.len() < 100 {
        return ContentVerdict::Safe;
    }

    // Phase 1: Header-based validation — ensure the file is actually an image
    if !is_valid_image_header(data) {
        warn!("File {} claims to be an image but has invalid header", path);
        return ContentVerdict::BlockedExtension("fake-image".to_string());
    }

    // Phase 2: ONNX NSFW model — placeholder for now
    // When the ort crate and model are integrated, this will run the classifier.
    // For now, all valid images pass.
    info!("Image scanned: {} ({} bytes) — passed", path, data.len());
    ContentVerdict::Safe
}

/// Validate image file magic bytes.
fn is_valid_image_header(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // JPEG: FF D8 FF
    if data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF {
        return true;
    }
    // PNG: 89 50 4E 47
    if data[0] == 0x89 && data[1] == 0x50 && data[2] == 0x4E && data[3] == 0x47 {
        return true;
    }
    // GIF: GIF8
    if data[0] == b'G' && data[1] == b'I' && data[2] == b'F' && data[3] == b'8' {
        return true;
    }
    // BMP: BM
    if data[0] == b'B' && data[1] == b'M' {
        return true;
    }
    // WebP: RIFF....WEBP
    if data.len() >= 12
        && data[0] == b'R'
        && data[1] == b'I'
        && data[2] == b'F'
        && data[3] == b'F'
        && data[8] == b'W'
        && data[9] == b'E'
        && data[10] == b'B'
        && data[11] == b'P'
    {
        return true;
    }
    // TIFF: II or MM
    if (data[0] == b'I' && data[1] == b'I') || (data[0] == b'M' && data[1] == b'M') {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blocked_extensions() {
        assert!(check_extension("/test.exe").is_blocked());
        assert!(check_extension("/path/to/virus.bat").is_blocked());
        assert!(check_extension("/script.ps1").is_blocked());
        assert!(check_extension("/malware.DLL").is_blocked()); // case insensitive
        assert!(!check_extension("/photo.jpg").is_blocked());
        assert!(!check_extension("/document.pdf").is_blocked());
        assert!(!check_extension("/video.mp4").is_blocked());
    }

    #[test]
    fn test_size_limits() {
        assert!(!check_size(1024).is_blocked());
        assert!(!check_size(MAX_FILE_SIZE).is_blocked());
        assert!(check_size(MAX_FILE_SIZE + 1).is_blocked());
    }

    #[test]
    fn test_image_detection() {
        assert!(is_image("photo.jpg"));
        assert!(is_image("image.PNG"));
        assert!(is_image("pic.webp"));
        assert!(!is_image("doc.pdf"));
        assert!(!is_image("video.mp4"));
    }

    #[test]
    fn test_valid_image_headers() {
        assert!(is_valid_image_header(&[0xFF, 0xD8, 0xFF, 0xE0])); // JPEG
        assert!(is_valid_image_header(&[0x89, 0x50, 0x4E, 0x47])); // PNG
        assert!(is_valid_image_header(&[b'G', b'I', b'F', b'8'])); // GIF
        assert!(is_valid_image_header(&[b'B', b'M', 0, 0]));       // BMP
        assert!(!is_valid_image_header(&[0x00, 0x00, 0x00, 0x00])); // Random
    }
}
