//! Implementation of [`MarkHiddenPort`] over each platform's notion of a
//! hidden path.
//!
//! The two platforms disagree on what makes a path hidden, and the difference
//! is not cosmetic trivia — it decides whether a working area shows up in the
//! middle of the user's own folder:
//!
//! - Unix: a leading dot is the whole convention, and file browsers honor it.
//!   Callers already name their working areas that way, so there is nothing
//!   left to do here.
//! - Windows: the dot means nothing. Hiding is an explicit file attribute,
//!   `FILE_ATTRIBUTE_HIDDEN`, and without setting it a dot-prefixed directory
//!   is as visible as any other.

use std::path::Path;

use async_trait::async_trait;
use uc_core::ports::hidden_path::MarkHiddenPort;

pub struct FsHiddenPathMarker;

impl FsHiddenPathMarker {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(Self)
    }
}

impl Default for FsHiddenPathMarker {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl MarkHiddenPort for FsHiddenPathMarker {
    async fn mark_hidden(&self, path: &Path) {
        let path = path.to_path_buf();
        // A join error means the attribute was not set; the port promises
        // nothing about the outcome, so there is nothing to report.
        let _ = tokio::task::spawn_blocking(move || set_hidden(&path)).await;
    }
}

/// The dot prefix already did this.
#[cfg(not(windows))]
fn set_hidden(_path: &Path) {}

#[cfg(windows)]
fn set_hidden(path: &Path) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileAttributesW, SetFileAttributesW, FILE_ATTRIBUTE_HIDDEN, INVALID_FILE_ATTRIBUTES,
    };

    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    if wide.contains(&0) {
        return;
    }
    wide.push(0);

    // Read-modify-write: overwriting the attribute set wholesale would clear
    // whatever else the path carries (FILE_ATTRIBUTE_DIRECTORY above all).
    let current = unsafe { GetFileAttributesW(wide.as_ptr()) };
    if current == INVALID_FILE_ATTRIBUTES {
        tracing::debug!("could not read attributes while hiding a path");
        return;
    }
    if current & FILE_ATTRIBUTE_HIDDEN != 0 {
        return;
    }
    if unsafe { SetFileAttributesW(wide.as_ptr(), current | FILE_ATTRIBUTE_HIDDEN) } == 0 {
        tracing::debug!("could not hide a path; it stays visible");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn marking_leaves_the_directory_in_place_and_readable() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".uniclip-probe");
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), b"hi").unwrap();

        FsHiddenPathMarker.mark_hidden(&dir).await;

        // Whatever the platform did with visibility, the directory must still
        // be a directory the caller can keep working in.
        assert!(dir.is_dir());
        assert_eq!(std::fs::read(dir.join("a.txt")).unwrap(), b"hi");
        std::fs::write(dir.join("b.txt"), b"more").unwrap();
    }

    #[tokio::test]
    async fn marking_a_missing_path_is_silent() {
        let tmp = TempDir::new().unwrap();
        FsHiddenPathMarker
            .mark_hidden(&tmp.path().join("absent"))
            .await;
    }

    #[tokio::test]
    async fn marking_twice_is_idempotent() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".uniclip-probe");
        std::fs::create_dir(&dir).unwrap();

        FsHiddenPathMarker.mark_hidden(&dir).await;
        FsHiddenPathMarker.mark_hidden(&dir).await;

        assert!(dir.is_dir());
    }
}
