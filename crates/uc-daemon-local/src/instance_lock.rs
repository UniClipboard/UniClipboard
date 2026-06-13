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
use std::time::{Duration, Instant};

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

    /// Blocking acquire — parks the calling thread in `flock(2)` until the
    /// current holder releases. Only called from [`acquire_with_deadline`]'s
    /// dedicated blocking thread; the `DISABLE_ENV` escape valve is handled
    /// by the `try_acquire` fast path before this runs.
    fn acquire_blocking(data_dir: &Path) -> Result<Self, InstanceLockError> {
        let lock_path = data_dir.join(".uniclipd.lock");
        let file = File::create(&lock_path).map_err(InstanceLockError::Io)?;

        #[cfg(unix)]
        repair_lock_permissions(&lock_path);

        file.lock_exclusive().map_err(InstanceLockError::Io)?;
        Ok(Self {
            _file: Some(file),
            path: lock_path,
        })
    }
}

/// Acquire the per-profile instance lock, blocking (event-driven) until an
/// exiting holder releases it, bounded by `deadline`.
///
/// Used by the daemon host on EVERY start to ride out the gap where an exiting
/// predecessor still holds the lock during iroh teardown: the predecessor's
/// `/health` endpoint goes absent (HTTP server cancelled) BEFORE the instance
/// lock is released (iroh `endpoint.close()` then guard drop), so a freshly
/// spawned daemon can observe the lock still held even though the predecessor
/// is already exiting. This race is not specific to controlled restarts — any
/// health-probing spawner (CLI/GUI) can hit it after a plain stop/start cycle
/// (observed in production, 2026-06-12: spawner saw `/health` absent at T+2s,
/// predecessor released the lock at T+5.4s, replacement exited
/// `AlreadyRunning`).
///
/// The wait is a blocking `flock(2)` on a dedicated DETACHED `std::thread`
/// (NOT `spawn_blocking`: tokio joins its blocking pool on runtime drop, so a
/// thread still parked in `flock` after a deadline expiry would deadlock the
/// daemon's exit path): the kernel wakes the waiter the instant the holder
/// releases — no polling, no budget tuned to the predecessor's internal
/// teardown phases. `deadline` is therefore pure protection against a holder
/// that never exits, not a timing estimate; expiring returns
/// [`InstanceLockError::AlreadyRunning`].
///
/// Failure semantics: a fast-path I/O error is terminal immediately; a
/// deadline expiry leaves the detached thread parked in `flock(2)` — if the
/// holder later exits, the thread wins the lock, fails to send it (receiver
/// dropped), and releases it immediately; process exit reclaims the thread
/// and any held lock either way, so the orphan is harmless.
pub async fn acquire_with_deadline(
    data_dir: &Path,
    deadline: Duration,
) -> Result<DaemonInstanceLock, InstanceLockError> {
    // Fast path — also covers the `DISABLE_ENV` escape valve and terminal
    // I/O errors (bad permissions, disk full) without spawning a thread.
    let lock_path = match DaemonInstanceLock::try_acquire(data_dir) {
        Ok(lock) => return Ok(lock),
        Err(InstanceLockError::AlreadyRunning { lock_path }) => lock_path,
        Err(other) => return Err(other),
    };

    // Single-instance arbitration (the "evicted after update" fix): before
    // patiently waiting out the holder, decide whether THIS daemon out-ranks it
    // (`should_evict_holder`). A strictly-older or stuck/orphaned holder is
    // SIGTERM'd — escalating to SIGKILL — so the newcomer takes over
    // deterministically, instead of waiting out `deadline` and exiting
    // `AlreadyRunning` (the symptom users hit after an update: had to kill the
    // leftover uniclipd by hand). An equal-or-newer holder is left untouched
    // (downgrade protection) and we fall through to the cooperative wait below.
    // Eviction (below) may itself spend up to EVICT_SIGTERM_GRACE +
    // EVICT_SIGKILL_GRACE before falling through to the cooperative wait. Start
    // the clock BEFORE it and charge that time against `deadline`, so the total
    // wait still honors the caller's budget — the spawner-side
    // DAEMON_STARTUP_TIMEOUT is derived assuming this lock wait stays ≤ deadline
    // (the coupling `timing` exists to keep load-bearing).
    let started = Instant::now();

    if let Some(lock) = try_evict_outranked_holder(data_dir).await? {
        return Ok(lock);
    }

    let remaining = deadline.saturating_sub(started.elapsed());

    tracing::warn!(
        lock_path = %lock_path.display(),
        deadline_ms = remaining.as_millis() as u64,
        "daemon instance lock busy — blocking until the holder releases it"
    );

    match block_acquire_within(data_dir, remaining).await? {
        Some(lock) => {
            tracing::info!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "daemon instance lock acquired after holder release"
            );
            Ok(lock)
        }
        // Deadline expired: the holder never released within the bound.
        None => Err(InstanceLockError::AlreadyRunning { lock_path }),
    }
}

/// Park a detached thread in blocking `flock(2)`; resolve to `Ok(Some(lock))` if
/// acquired within `budget`, `Ok(None)` if the budget expired with the holder
/// still in place, `Err` on I/O / wait-thread failure.
///
/// A DETACHED `std::thread` (NOT `spawn_blocking`: tokio joins its blocking pool
/// on runtime drop, so a thread still parked in `flock` after a budget expiry
/// would deadlock the daemon's exit path). On expiry the thread is harmless: if
/// the holder later exits, the thread wins the lock, fails to send it (receiver
/// dropped), and releases it immediately; process exit reclaims it regardless.
async fn block_acquire_within(
    data_dir: &Path,
    budget: Duration,
) -> Result<Option<DaemonInstanceLock>, InstanceLockError> {
    let dir = data_dir.to_path_buf();
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("uniclipd-lock-wait".into())
        .spawn(move || {
            let _ = tx.send(DaemonInstanceLock::acquire_blocking(&dir));
        })
        .map_err(InstanceLockError::Io)?;

    match tokio::time::timeout(budget, rx).await {
        Ok(Ok(Ok(lock))) => Ok(Some(lock)),
        Ok(Ok(Err(error))) => Err(error),
        // Sender dropped without sending — the wait thread panicked.
        Ok(Err(_recv_error)) => Err(InstanceLockError::Io(std::io::Error::other(
            "instance lock wait thread terminated unexpectedly",
        ))),
        // Budget expired: the holder never released within the bound.
        Err(_elapsed) => Ok(None),
    }
}

/// Evict the current lock holder iff THIS daemon strictly out-ranks it, then try
/// to take the freed lock. Returns `Ok(Some(lock))` if eviction won the lock,
/// `Ok(None)` if no eviction was warranted or it did not free the lock in time
/// (caller falls through to the cooperative deadline wait), `Err` on terminal
/// I/O.
///
/// Best-effort and conservative: an unreadable/absent holder PID file, a
/// stale/foreign PID (D22 — not a live `uniclipd`), or an equal-or-newer holder
/// all yield `Ok(None)` WITHOUT signaling anyone.
async fn try_evict_outranked_holder(
    data_dir: &Path,
) -> Result<Option<DaemonInstanceLock>, InstanceLockError> {
    use uc_daemon_process::contract::{
        force_terminate_local_daemon_pid, terminate_local_daemon_pid,
    };
    use uc_daemon_process::process_metadata::{
        read_pid_metadata_in, should_evict_holder, verify_pid_identity, PidVerification,
        SELF_PACKAGE_VERSION,
    };
    use uc_daemon_process::timing::{EVICT_SIGKILL_GRACE, EVICT_SIGTERM_GRACE};

    // Best-effort read of the holder's PID metadata; on any problem fall back to
    // the plain cooperative wait.
    let holder = match read_pid_metadata_in(data_dir) {
        Ok(Some(metadata)) => metadata,
        Ok(None) => return Ok(None),
        Err(error) => {
            tracing::warn!(%error, "could not read lock holder PID metadata — skipping eviction");
            return Ok(None);
        }
    };

    let self_pid = std::process::id();

    // Never signal our own PID. If a stale PID file's recorded pid was recycled
    // to THIS very process, `verify_pid_identity` would see it "alive" (it is us)
    // and we would SIGTERM ourselves — the real lock holder is someone else, so
    // fall back to the cooperative wait instead.
    if holder.pid == self_pid {
        return Ok(None);
    }

    // D22: never signal a PID that is not a live `uniclipd`. A stale PID means
    // the real holder already exited (the lock will free on its own); a foreign
    // PID is someone else's process. Either way, do not signal.
    if !matches!(verify_pid_identity(&holder), PidVerification::Active) {
        return Ok(None);
    }

    let self_started_at_ms = now_ms();
    if !should_evict_holder(SELF_PACKAGE_VERSION, self_started_at_ms, self_pid, &holder) {
        tracing::info!(
            holder_pid = holder.pid,
            holder_version = %holder.package_version,
            self_version = SELF_PACKAGE_VERSION,
            "instance lock held by an equal-or-newer daemon — not evicting (downgrade protection)"
        );
        return Ok(None);
    }

    tracing::warn!(
        holder_pid = holder.pid,
        holder_version = %holder.package_version,
        self_version = SELF_PACKAGE_VERSION,
        "instance lock held by an out-ranked daemon — evicting it (SIGTERM, then SIGKILL)"
    );

    if let Err(error) = terminate_local_daemon_pid(holder.pid) {
        tracing::warn!(holder_pid = holder.pid, %error, "SIGTERM to lock holder failed");
    }
    if let Some(lock) = block_acquire_within(data_dir, EVICT_SIGTERM_GRACE).await? {
        tracing::info!(
            holder_pid = holder.pid,
            "instance lock acquired after holder SIGTERM"
        );
        return Ok(Some(lock));
    }

    // Holder did not release within the graceful window. Re-verify identity (a
    // recycled PID must not be SIGKILL'd) and escalate.
    if matches!(verify_pid_identity(&holder), PidVerification::Active) {
        tracing::warn!(
            holder_pid = holder.pid,
            "lock holder still alive after SIGTERM grace — escalating to SIGKILL"
        );
        if let Err(error) = force_terminate_local_daemon_pid(holder.pid) {
            tracing::warn!(holder_pid = holder.pid, %error, "SIGKILL to lock holder failed");
        }
        if let Some(lock) = block_acquire_within(data_dir, EVICT_SIGKILL_GRACE).await? {
            tracing::info!(
                holder_pid = holder.pid,
                "instance lock acquired after holder SIGKILL"
            );
            return Ok(Some(lock));
        }
    }

    // Even SIGKILL did not free it in time (D-state, or a PID that went stale
    // between checks). Fall through to the cooperative deadline wait.
    Ok(None)
}

/// Unix-epoch milliseconds for "now" — this process's start instant for
/// single-instance ranking (`should_evict_holder`). Matches how
/// `DaemonPidMetadata::now` stamps `started_at_ms`, so the value compared here
/// is on the same clock as a holder's persisted timestamp.
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
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

    // ---------- acquire_with_deadline ----------
    //
    // These tests use real `flock(2)` blocking and real time, so they build
    // their own multi-thread runtime inside a sync `#[test]` body: the
    // `ENV_LOCK` guard (a std MutexGuard, !Send) can then be held across the
    // whole test without making the test future non-Send.

    fn blocking_test_runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .unwrap()
    }

    /// Free lock → the fast path acquires immediately, no blocking thread.
    #[test]
    fn acquire_with_deadline_succeeds_immediately_when_free() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let lock = blocking_test_runtime()
            .block_on(acquire_with_deadline(dir.path(), Duration::from_secs(1)))
            .expect("free lock must be acquired on the fast path");
        assert!(lock.path().exists());
    }

    /// Held lock released mid-wait → the blocked waiter wakes and acquires
    /// well before the deadline (event-driven, not deadline-driven).
    #[test]
    fn acquire_with_deadline_wakes_when_holder_releases() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let holder = DaemonInstanceLock::try_acquire(dir.path()).unwrap();
        let release_after = Duration::from_millis(300);
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(release_after);
            drop(holder);
        });

        let started = Instant::now();
        let lock = blocking_test_runtime()
            .block_on(acquire_with_deadline(dir.path(), Duration::from_secs(10)))
            .expect("waiter must acquire once the holder releases");
        let elapsed = started.elapsed();

        assert!(lock.path().exists());
        assert!(
            elapsed >= Duration::from_millis(250),
            "must actually have waited for the holder (elapsed {elapsed:?})"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "must wake on release, long before the deadline (elapsed {elapsed:?})"
        );
        releaser.join().unwrap();
    }

    /// Holder never releases → deadline expires with `AlreadyRunning`.
    #[test]
    fn acquire_with_deadline_times_out_when_holder_never_releases() {
        let _env = ENV_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let _holder = DaemonInstanceLock::try_acquire(dir.path()).unwrap();

        let err = blocking_test_runtime()
            .block_on(acquire_with_deadline(
                dir.path(),
                Duration::from_millis(300),
            ))
            .expect_err("a holder that never releases must surface AlreadyRunning");
        assert!(matches!(err, InstanceLockError::AlreadyRunning { .. }));
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
