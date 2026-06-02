//! Per-profile daemon instance lock using OS advisory file locks.
//!
//! The lock **must be acquired before** binding the HTTP port or iroh endpoint.
//! A second daemon attempting to start on the same profile will fail cleanly
//! with [`InstanceLockError::AlreadyRunning`] instead of crashing with
//! `AddrInUse`.
//!
//! The lock is released automatically when the process exits (including
//! crashes and OOM kills) because the OS reclaims advisory locks on
//! file-descriptor close.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use fs2::FileExt;

/// Held for the daemon's lifetime. Dropping it releases the lock.
#[derive(Debug)]
pub struct DaemonInstanceLock {
    _file: File,
    path: PathBuf,
}

#[derive(Debug)]
pub enum InstanceLockError {
    /// Another daemon instance already holds the lock for this profile.
    AlreadyRunning { lock_path: PathBuf },
    /// I/O error creating or locking the file.
    Io(std::io::Error),
}

impl std::fmt::Display for InstanceLockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyRunning { lock_path } => write!(
                f,
                "another daemon instance is already running for this profile \
                 (lock held on {})",
                lock_path.display()
            ),
            Self::Io(error) => write!(f, "failed to acquire daemon instance lock: {error}"),
        }
    }
}

impl std::error::Error for InstanceLockError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl DaemonInstanceLock {
    /// Try to acquire the per-profile instance lock.
    ///
    /// `data_dir` is the profile's `app_data_root_dir` (same directory that
    /// holds `.daemon-pid` and `.daemon-token`).
    ///
    /// Returns `Err(AlreadyRunning)` immediately (non-blocking) if another
    /// process already holds the lock.
    pub fn try_acquire(data_dir: &Path) -> Result<Self, InstanceLockError> {
        fs::create_dir_all(data_dir).map_err(InstanceLockError::Io)?;

        let lock_path = data_dir.join(".uniclipd.lock");
        let file = File::create(&lock_path).map_err(InstanceLockError::Io)?;

        #[cfg(unix)]
        repair_lock_permissions(&lock_path);

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self {
                _file: file,
                path: lock_path,
            }),
            Err(error) if is_would_block(&error) => {
                Err(InstanceLockError::AlreadyRunning { lock_path })
            }
            Err(error) => Err(InstanceLockError::Io(error)),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn is_would_block(error: &std::io::Error) -> bool {
    // fs2 returns WouldBlock when the lock is held by another process.
    // On some platforms it may also surface as a raw OS error.
    error.kind() == std::io::ErrorKind::WouldBlock || error.raw_os_error() == Some(libc_eagain())
}

#[cfg(unix)]
fn libc_eagain() -> i32 {
    libc::EAGAIN
}

#[cfg(not(unix))]
fn libc_eagain() -> i32 {
    -1 // sentinel — WouldBlock kind check covers Windows
}

#[cfg(unix)]
fn repair_lock_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn acquire_and_release() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        assert!(lock.path().exists());
        drop(lock);
    }

    #[test]
    fn second_acquire_fails_with_already_running() {
        let dir = tempfile::tempdir().unwrap();
        let _lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();

        match DaemonInstanceLock::try_acquire(dir.path()) {
            Err(InstanceLockError::AlreadyRunning { .. }) => {}
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn reacquire_after_drop() {
        let dir = tempfile::tempdir().unwrap();
        let lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        drop(lock);
        let _lock2 = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
    }
}
