//! Detached spawn of the `uniclipd` daemon binary.
//!
//! Shared between every desktop-side process that needs to bring a local
//! daemon up out-of-process:
//!
//! - `uc-cli` (`uniclip start`) — the historical caller.
//! - GUI shells (`uc-tauri`, future native) — ADR-008 P3: the GUI becomes a
//!   pure client and spawns the daemon as an independent `uniclipd` process
//!   instead of hosting it in-process.
//!
//! This module only knows how to *spawn detached* + *resolve the binary*. The
//! probe→spawn→wait-health orchestration (spinners, timeouts) stays with each
//! caller, layered on [`crate::health_wait`]. Keeping the spawn primitive here
//! — rather than duplicated per shell — is the whole point: there is exactly
//! one place that gets the Unix `setsid` / Windows `DETACHED_PROCESS` detach
//! semantics right.

use std::fmt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use crate::process_metadata::{DaemonSpawnOrigin, SPAWN_ORIGIN_ENV};

/// Failure modes of [`spawn_detached_daemon`] / [`resolve_daemon_exe_path`].
///
/// Deliberately uses a hand-rolled `Display` (no `thiserror`) to keep this thin
/// crate dependency-light and buildable on every target, including Windows.
#[derive(Debug)]
pub enum SpawnDaemonError {
    /// The `uniclipd` binary could not be located (neither as a sibling of the
    /// current executable nor on `PATH`).
    ResolveBinary(anyhow::Error),
    /// `Command::spawn` failed for the resolved binary.
    Spawn(anyhow::Error),
}

impl fmt::Display for SpawnDaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ResolveBinary(error) => {
                write!(f, "failed to resolve `uniclipd` binary for spawn: {error}")
            }
            Self::Spawn(error) => write!(f, "failed to spawn daemon process: {error}"),
        }
    }
}

impl std::error::Error for SpawnDaemonError {}

/// Spawn `uniclipd` as a **detached** background process.
///
/// "Detached" means the new process survives the spawning process exiting —
/// that's the whole point of bringing up a daemon. We rely on three pieces:
///
/// 1. `Stdio::null()` on all three streams so the daemon never inherits the
///    terminal — closing the controlling tty must not propagate SIGHUP to it.
/// 2. **Unix**: `setsid()` in a `pre_exec` hook. The child becomes a new
///    session leader detached from the parent's controlling terminal, so
///    `Ctrl+C` / shell exit doesn't reach it. As session leader of its own
///    session, signals to the *parent's* process group don't hit it either.
/// 3. **Windows**: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` flags. The
///    daemon gets no console of its own and is a separate process group, so
///    `Ctrl+C` on the parent console doesn't deliver `CTRL_C_EVENT` to it.
///
/// The spawned `Child` is intentionally dropped: under Unix that does *not*
/// reap the process (it's a session leader with cut stdio; the kernel reaps it
/// when it exits, its parent reparents to PID 1 once the spawner returns).
/// Under Windows the handle just closes; the process keeps running. Proving the
/// daemon actually came up is the caller's job (poll `/health`).
///
/// `origin` records who is bringing the daemon up (ADR-008 D3). It is passed to
/// the child via [`SPAWN_ORIGIN_ENV`]; the daemon reads it back when writing its
/// PID file ([`DaemonSpawnOrigin::from_env`]), so a GUI can later tell whether
/// the daemon it attached to is one a GUI spawned (stoppable on full quit).
///
/// `handover_dir_override` is a test/override hook so the handover read aligns
/// with a redirected data dir; production passes `None` (resolves the system
/// `app_data_root`, byte-identical).
pub fn spawn_detached_daemon(
    origin: DaemonSpawnOrigin,
    handover_dir_override: Option<std::path::PathBuf>,
) -> Result<(), SpawnDaemonError> {
    let daemon_exe = resolve_daemon_exe_path()?;

    let mut command = Command::new(&daemon_exe);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .env(SPAWN_ORIGIN_ENV, origin.as_env_str());

    configure_detached(&mut command);

    // ADR-008 P5-L L7: honour a pending cross-process handover. A controlled
    // restart (L8) writes the target run mode to the lock dir while holding the
    // OLD daemon's instance lock; the spawner reads it here as a HINT and launches
    // the new daemon in that mode via RUN_MODE_ENV. Best-effort: in production
    // nothing writes a record (read → None), so the spawn is unchanged.
    //
    // ADR-008 P5-L L8a: resolve the handover dir via the override when present
    // (so a redirected-data-dir test reads the same dir the daemon resolves),
    // else fall back to the system app_data_root — production passes None, so
    // the resolved path is byte-identical to before.
    let handover_root = resolve_handover_dir(handover_dir_override);
    if let Some(app_data_root) = handover_root {
        if let Some(record) = crate::handover::read(&app_data_root) {
            command.env(crate::spawn_contract::RUN_MODE_ENV, &record.target_mode);
            tracing::info!(
                target_mode = %record.target_mode,
                generation = record.generation,
                "spawning daemon to fulfil a pending handover",
            );
        }
    }

    let child = command.spawn().map_err(|error| {
        SpawnDaemonError::Spawn(anyhow::Error::new(error).context(format!(
            "failed to spawn daemon via `{}`",
            daemon_exe.display()
        )))
    })?;

    // Drop the handle deliberately — see fn doc. The detached child runs on its
    // own; the spawner's responsibility ends here.
    drop(child);
    Ok(())
}

/// Resolve the directory to read the pending handover record from.
///
/// The override wins when present (ADR-008 P5-L L8a: a redirected-data-dir test
/// passes the dir the daemon resolves, so the spawner's read targets the same
/// place the daemon writes/clears it). Otherwise fall back to the system
/// `app_data_root`. Production passes `None`, so this is byte-identical to
/// resolving `uc_app_paths::app_data_root()` directly.
fn resolve_handover_dir(
    handover_dir_override: Option<std::path::PathBuf>,
) -> Option<std::path::PathBuf> {
    handover_dir_override.or_else(uc_app_paths::app_data_root)
}

#[cfg(unix)]
fn configure_detached(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: `setsid` is async-signal-safe and only touches process group /
    // session ids. It's the documented way to detach a child from the
    // controlling terminal between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(windows)]
fn configure_detached(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    // CreateProcess flags. Combined: no console + own process group, so
    // `Ctrl+C` to the parent's console does not propagate to the daemon.
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

#[cfg(not(any(unix, windows)))]
fn configure_detached(_command: &mut Command) {
    // No detachment configured for unknown platforms — the daemon will still be
    // spawned but may receive parent signals. Acceptable as a degraded fallback;
    // our real targets (macOS / Linux / Windows) all hit the paths above.
}

/// Resolve the path to the `uniclipd` daemon binary.
///
/// Strategy:
/// 1. Look for `uniclipd` (or `uniclipd.exe` on Windows) as a sibling of the
///    current executable. This covers Tauri sidecar bundles, `cargo build`
///    output directories, and Docker images where both binaries sit in the
///    same directory.
/// 2. Fall back to a `PATH` lookup so system-wide installs work.
pub fn resolve_daemon_exe_path() -> Result<PathBuf, SpawnDaemonError> {
    let daemon_name = if cfg!(windows) {
        "uniclipd.exe"
    } else {
        "uniclipd"
    };

    // Strategy 1: sibling of current executable.
    if let Ok(self_exe) = std::env::current_exe() {
        if let Some(dir) = self_exe.parent() {
            let candidate = dir.join(daemon_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    // Strategy 2: PATH lookup.
    which::which(daemon_name).map_err(|error| {
        SpawnDaemonError::ResolveBinary(anyhow::Error::new(error).context(format!(
            "`{daemon_name}` not found as sibling of the spawning binary or in PATH"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_resolve_binary_self_identifies() {
        let err = SpawnDaemonError::ResolveBinary(anyhow::anyhow!("not on PATH"));
        let s = err.to_string();
        assert!(s.contains("uniclipd"), "must name the binary: {s}");
        assert!(s.contains("not on PATH"), "must surface the cause: {s}");
    }

    #[test]
    fn display_spawn_self_identifies() {
        let err = SpawnDaemonError::Spawn(anyhow::anyhow!("permission denied"));
        let s = err.to_string();
        assert!(s.contains("spawn"), "Spawn variant must self-identify: {s}");
        assert!(s.contains("permission denied"));
    }

    #[test]
    fn resolve_daemon_exe_path_does_not_panic() {
        // In a cargo test environment `uniclipd` may or may not be built. We
        // only assert the resolver doesn't panic — the actual resolution
        // depends on the build layout.
        let _result = resolve_daemon_exe_path();
    }

    #[test]
    fn resolve_handover_dir_prefers_the_override() {
        // ADR-008 P5-L L8a: an explicit override dir wins over the system path,
        // so a redirected-data-dir test (L8d) reads the handover from the same
        // dir the daemon writes/clears it.
        let dir = std::path::PathBuf::from("/tmp/uc-l8a-handover-override-probe");
        assert_eq!(resolve_handover_dir(Some(dir.clone())), Some(dir));
    }

    #[test]
    fn resolve_handover_dir_falls_back_to_system_when_none() {
        // Production passes None → byte-identical to the pre-L8a behaviour of
        // resolving `uc_app_paths::app_data_root()` directly.
        assert_eq!(resolve_handover_dir(None), uc_app_paths::app_data_root());
    }
}
