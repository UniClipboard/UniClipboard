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

/// Escape valve mirroring the GUI's `UC_DISABLE_SINGLE_INSTANCE` (set to `1`).
///
/// When set, the daemon skips the per-profile lock so multiple daemons may run
/// on one profile. Test/dev only — production must keep the singleton invariant
/// (ADR-008 D22); this exists for harnesses that intentionally spin up several
/// daemons against the same data dir.
const DISABLE_ENV: &str = "UC_DISABLE_DAEMON_SINGLE_INSTANCE";

fn single_instance_disabled() -> bool {
    std::env::var(DISABLE_ENV).as_deref() == Ok("1")
}

/// Held for the daemon's lifetime. Dropping it releases the lock.
///
/// `_file` is `None` only when the lock was bypassed via [`DISABLE_ENV`]; the
/// guard is still returned so the call site is unchanged.
#[derive(Debug)]
pub struct DaemonInstanceLock {
    _file: Option<File>,
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

        if single_instance_disabled() {
            tracing::warn!(
                env = DISABLE_ENV,
                "daemon single-instance lock disabled via env — concurrent \
                 daemons on this profile are allowed (test/dev escape valve, \
                 ADR-008 D22)"
            );
            return Ok(Self {
                _file: None,
                path: lock_path,
            });
        }

        let file = File::create(&lock_path).map_err(InstanceLockError::Io)?;

        #[cfg(unix)]
        repair_lock_permissions(&lock_path);

        match file.try_lock_exclusive() {
            Ok(()) => Ok(Self {
                _file: Some(file),
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
    use std::sync::Mutex;

    // `try_acquire` reads the process-global `DISABLE_ENV`; serialise every test
    // here so the disabled-path test cannot leak the env var into a concurrent
    // test that expects the lock to be live.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn acquire_and_release() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        assert!(lock.path().exists());
        drop(lock);
    }

    #[test]
    fn second_acquire_fails_with_already_running() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let _lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();

        match DaemonInstanceLock::try_acquire(dir.path()) {
            Err(InstanceLockError::AlreadyRunning { .. }) => {}
            other => panic!("expected AlreadyRunning, got {other:?}"),
        }
    }

    #[test]
    fn reacquire_after_drop() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        drop(lock);
        let _lock2 = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
    }

    #[test]
    fn disable_env_bypasses_lock_for_concurrent_daemons() {
        let _env = ENV_LOCK.lock().unwrap();
        std::env::set_var(DISABLE_ENV, "1");
        let dir = tempfile::tempdir().unwrap();

        // Both acquisitions succeed because the singleton invariant is disabled.
        let lock_a = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        let lock_b = DaemonInstanceLock::try_acquire(dir.path()).unwrap();

        std::env::remove_var(DISABLE_ENV);
        drop((lock_a, lock_b));
    }
}
