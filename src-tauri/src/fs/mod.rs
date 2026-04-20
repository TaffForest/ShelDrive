// Platform-gated filesystem driver
// - Unix (macOS/Linux): FUSE via fuser crate + FUSE-T / libfuse3
// - Windows: WinFSP

#[cfg(unix)]
pub mod fuse_driver;

#[cfg(windows)]
pub mod winfsp_driver;

// Re-export the active driver as `driver` so callers don't need cfg checks
#[cfg(unix)]
pub use fuse_driver as driver;

#[cfg(windows)]
pub use winfsp_driver as driver;
