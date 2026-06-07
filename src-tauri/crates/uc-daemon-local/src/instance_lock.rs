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

/// Whether the per-profile single-instance lock is disabled via [`DISABLE_ENV`].
///
/// Exposed for ADR-008 P5-L L8c: the controlled-restart path refuses to operate
/// when the single-instance lock is disabled (the handover safety model relies
/// on lock mutual exclusion).
pub fn single_instance_disabled() -> bool {
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

/// Retry an instance-lock acquisition while it keeps returning
/// [`InstanceLockError::AlreadyRunning`].
///
/// Used by the daemon host to ride out the gap where a controlled-restart
/// predecessor still holds the lock during iroh teardown: the predecessor's
/// `/health` endpoint goes absent (HTTP server cancelled) BEFORE the instance
/// lock is released (iroh `endpoint.close()` then guard drop), so a freshly
/// spawned promotion daemon can briefly observe `AlreadyRunning` even though the
/// predecessor is already exiting. Only [`InstanceLockError::AlreadyRunning`] is
/// retried; any other error (e.g. [`InstanceLockError::Io`]) is returned
/// immediately. If `max_attempts` is exhausted while still `AlreadyRunning`, the
/// last `AlreadyRunning` is returned.
///
/// `max_attempts` caps attempts, not sleeps: the closure is always called at
/// least once (even with `max_attempts == 0`), and a sleep happens only BETWEEN
/// attempts, so `n` attempts incur at most `n - 1` sleeps.
///
/// Generic over `T` so it is unit-testable with a fake closure (no real fs2
/// lock needed).
pub async fn retry_while_already_running<T, F>(
    mut attempt: F,
    max_attempts: usize,
    interval: std::time::Duration,
) -> Result<T, InstanceLockError>
where
    F: FnMut() -> Result<T, InstanceLockError>,
{
    let mut remaining = max_attempts;
    loop {
        match attempt() {
            Ok(value) => return Ok(value),
            Err(InstanceLockError::AlreadyRunning { lock_path }) => {
                remaining = remaining.saturating_sub(1);
                if remaining == 0 {
                    return Err(InstanceLockError::AlreadyRunning { lock_path });
                }
                tokio::time::sleep(interval).await;
            }
            // Any non-AlreadyRunning error is terminal — do not retry.
            Err(other) => return Err(other),
        }
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

    /// ADR-008 P5-L L8a: prove the lock-reuse ordering the controlled-restart
    /// path relies on — modelled on `uc-webserver`'s port-reuse proxy
    /// (`tests/graceful_shutdown_port_reuse.rs`). Sync test: the fs2 lock is
    /// synchronous, so no tokio runtime is needed.
    ///
    /// 1. Acquire succeeds on a fresh profile dir.
    /// 2. While the first guard is HELD, a second acquire returns
    ///    `AlreadyRunning` (mutual exclusion — a new daemon cannot grab the
    ///    lock before the old one releases it).
    /// 3. Dropping the guard releases the lock; an acquire IMMEDIATELY after
    ///    drop succeeds (release-on-drop, no lingering OS lock).
    #[test]
    fn lock_releases_on_drop_and_is_immediately_reacquirable() {
        let _env = ENV_LOCK.lock().unwrap();
        // Guard against the disabled escape valve leaking from another test: the
        // mutual-exclusion assertion below only holds when the lock is live.
        assert!(
            !single_instance_disabled(),
            "{DISABLE_ENV} must NOT be set for this test"
        );

        let dir = tempfile::tempdir().unwrap();

        // (1) Fresh acquire succeeds.
        let lock = DaemonInstanceLock::try_acquire(dir.path()).unwrap();

        // (2) While HELD, a second acquire is rejected with AlreadyRunning.
        match DaemonInstanceLock::try_acquire(dir.path()) {
            Err(InstanceLockError::AlreadyRunning { .. }) => {}
            other => panic!("expected AlreadyRunning while held, got {other:?}"),
        }

        // (3) Drop releases the lock; the very next acquire succeeds.
        drop(lock);
        let _lock2 = DaemonInstanceLock::try_acquire(dir.path())
            .expect("lock must be re-acquirable immediately after the prior guard drops");
    }

    // ---------- retry_while_already_running ----------

    /// A fake lock path so `AlreadyRunning` can be constructed without touching
    /// the filesystem — `retry_while_already_running` never inspects the path.
    fn fake_lock_path() -> PathBuf {
        PathBuf::from("/tmp/fake.uniclipd.lock")
    }

    /// ADR-008 P5-L L8d-2: (a) N `AlreadyRunning` then `Ok` → succeeds after the
    /// predecessor releases the lock, before exhausting the attempt budget.
    #[tokio::test(start_paused = true)]
    async fn retry_succeeds_after_some_already_running() {
        let calls = std::cell::Cell::new(0usize);
        let attempt = || {
            let n = calls.get();
            calls.set(n + 1);
            if n < 3 {
                Err(InstanceLockError::AlreadyRunning {
                    lock_path: fake_lock_path(),
                })
            } else {
                Ok::<u32, InstanceLockError>(42)
            }
        };

        let value = retry_while_already_running(attempt, 10, std::time::Duration::from_millis(200))
            .await
            .expect("must succeed once the predecessor releases the lock");

        assert_eq!(value, 42);
        assert_eq!(calls.get(), 4, "3 AlreadyRunning + 1 Ok = 4 attempts total");
    }

    /// (b) Always `AlreadyRunning` → returns `AlreadyRunning` after exhausting
    /// `max_attempts`; assert the attempt count is exactly the budget.
    #[tokio::test(start_paused = true)]
    async fn retry_exhausts_budget_then_returns_already_running() {
        let calls = std::cell::Cell::new(0usize);
        let attempt = || {
            calls.set(calls.get() + 1);
            Err::<u32, _>(InstanceLockError::AlreadyRunning {
                lock_path: fake_lock_path(),
            })
        };

        let err = retry_while_already_running(attempt, 5, std::time::Duration::from_millis(200))
            .await
            .expect_err("a predecessor that never releases must surface AlreadyRunning");

        assert!(matches!(err, InstanceLockError::AlreadyRunning { .. }));
        assert_eq!(
            calls.get(),
            5,
            "must attempt exactly max_attempts times before giving up"
        );
    }

    /// (c) An `Io` error on the first call propagates immediately without retry —
    /// only `AlreadyRunning` is transient.
    #[tokio::test(start_paused = true)]
    async fn retry_propagates_io_error_without_retry() {
        let calls = std::cell::Cell::new(0usize);
        let attempt = || {
            calls.set(calls.get() + 1);
            Err::<u32, _>(InstanceLockError::Io(std::io::Error::other("disk full")))
        };

        let err = retry_while_already_running(attempt, 10, std::time::Duration::from_millis(200))
            .await
            .expect_err("Io is terminal — must propagate");

        assert!(matches!(err, InstanceLockError::Io(_)));
        assert_eq!(
            calls.get(),
            1,
            "non-AlreadyRunning errors must not be retried"
        );
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
