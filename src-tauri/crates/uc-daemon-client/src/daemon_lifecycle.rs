//! GUI-owned daemon process lifecycle management.
//! Handles spawned daemon child tracking, graceful shutdown, and exit cleanup.

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri_plugin_shell::process::CommandChild;
use thiserror::Error;
use tokio::time::{sleep, Instant};

#[derive(Debug)]
pub struct TerminateDaemonError(pub String);

impl std::fmt::Display for TerminateDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TerminateDaemonError {}

/// Terminates a local daemon process by PID using platform-specific commands.
/// Returns `TerminateDaemonError` on failure.
pub fn terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("kill");
        command.arg("-TERM").arg(pid.to_string());
        command
    };

    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("taskkill");
        command.arg("/PID").arg(pid.to_string()).arg("/T").arg("/F");
        command
    };

    let output = command
        .output()
        .map_err(|e| TerminateDaemonError(format!("failed to launch terminator: {e}")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(TerminateDaemonError(format!(
        "failed to terminate pid {pid}: status={} stdout={} stderr={}",
        output.status,
        stdout.trim(),
        stderr.trim()
    )))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    Absent,
    Replacement,
}

#[derive(Debug)]
pub struct OwnedDaemonChild {
    pub pid: u32,
    pub spawn_reason: SpawnReason,
    pub child: CommandChild,
}

#[derive(Default)]
struct GuiOwnedDaemonStateInner {
    child: Mutex<Option<OwnedDaemonChild>>,
    exit_in_progress: AtomicBool,
}

#[derive(Clone, Default)]
pub struct GuiOwnedDaemonState(Arc<GuiOwnedDaemonStateInner>);

#[derive(Debug, Error)]
pub enum DaemonExitCleanupError {
    #[error("failed to terminate GUI-owned daemon pid {pid}: {details}")]
    Terminate { pid: u32, details: String },
    #[error("failed to observe GUI-owned daemon pid {pid} exit: {source}")]
    Observe {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("timed out waiting {timeout_ms}ms for GUI-owned daemon pid {pid} to exit")]
    Timeout { pid: u32, timeout_ms: u64 },
    #[error("failed to force kill GUI-owned daemon pid {pid}: {source}")]
    ForceKill {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to reap GUI-owned daemon pid {pid}: {source}")]
    Wait {
        pid: u32,
        #[source]
        source: std::io::Error,
    },
}

impl GuiOwnedDaemonState {
    /// Record a newly spawned daemon child. `pid` is passed separately since
    /// `CommandChild::pid()` must be called before moving the child.
    pub fn record_spawned(&self, child: CommandChild, pid: u32, spawn_reason: SpawnReason) {
        let owned_child = OwnedDaemonChild {
            pid,
            spawn_reason,
            child,
        };

        match self.0.child.lock() {
            Ok(mut guard) => {
                *guard = Some(owned_child);
            }
            Err(poisoned) => {
                tracing::error!(
                    "Mutex poisoned in GuiOwnedDaemonState::record_spawned, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                *guard = Some(owned_child);
            }
        }
    }

    pub fn clear(&self) -> Option<OwnedDaemonChild> {
        match self.0.child.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => {
                tracing::error!(
                    "Mutex poisoned in GuiOwnedDaemonState::clear, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.take()
            }
        }
    }

    pub fn snapshot_pid(&self) -> Option<u32> {
        match self.0.child.lock() {
            Ok(guard) => guard.as_ref().map(|owned_child| owned_child.pid),
            Err(poisoned) => {
                tracing::error!(
                    "Mutex poisoned in GuiOwnedDaemonState::snapshot_pid, recovering from poisoned state"
                );
                let guard = poisoned.into_inner();
                guard.as_ref().map(|owned_child| owned_child.pid)
            }
        }
    }

    pub fn begin_exit_cleanup(&self) -> bool {
        !self.0.exit_in_progress.swap(true, Ordering::SeqCst)
    }

    pub fn exit_cleanup_in_progress(&self) -> bool {
        self.0.exit_in_progress.load(Ordering::SeqCst)
    }

    pub fn finish_exit_cleanup(&self) {
        self.0.exit_in_progress.store(false, Ordering::SeqCst);
    }

    pub async fn shutdown_owned_daemon(
        &self,
        timeout: Duration,
        poll_interval: Duration,
    ) -> Result<bool, DaemonExitCleanupError> {
        let owned_child = match self.take_owned_child() {
            Some(c) => c,
            None => return Ok(false),
        };

        let daemon_pid = owned_child.pid;
        let spawn_reason = owned_child.spawn_reason;

        tracing::info!(
            daemon_pid,
            ?spawn_reason,
            timeout_ms = timeout.as_millis() as u64,
            poll_interval_ms = poll_interval.as_millis() as u64,
            "Starting GUI-owned daemon exit cleanup (sidecar)"
        );

        // Step 1: Send SIGTERM via PID
        if let Err(error) = terminate_local_daemon_pid(daemon_pid) {
            tracing::warn!(
                daemon_pid,
                error = %error,
                "SIGTERM failed; dropping CommandChild to close stdin tether (D-06)"
            );
            // Drop child to close stdin (daemon should exit on stdin EOF per D-06)
            drop(owned_child.child);
            // Wait briefly for stdin-EOF-triggered exit
            sleep(poll_interval).await;
            return Ok(true);
        }

        // Step 2: Poll for process exit
        let deadline = Instant::now() + timeout;
        let child = owned_child.child;
        loop {
            sleep(poll_interval).await;

            // Check if process is gone by trying to signal with 0 (Unix only)
            #[cfg(unix)]
            {
                let alive = unsafe { libc::kill(daemon_pid as libc::pid_t, 0) } == 0;
                if !alive {
                    tracing::info!(
                        daemon_pid,
                        ?spawn_reason,
                        "GUI-owned daemon exited after SIGTERM"
                    );
                    drop(child);
                    return Ok(true);
                }
            }

            if Instant::now() >= deadline {
                tracing::warn!(
                    daemon_pid,
                    ?spawn_reason,
                    timeout_ms = timeout.as_millis() as u64,
                    "GUI-owned daemon did not exit after SIGTERM; forcing kill via CommandChild"
                );
                // CommandChild::kill() consumes self
                if let Err(e) = child.kill() {
                    tracing::error!(daemon_pid, error = %e, "CommandChild::kill() failed");
                    return Err(DaemonExitCleanupError::ForceKill {
                        pid: daemon_pid,
                        source: std::io::Error::new(std::io::ErrorKind::Other, e.to_string()),
                    });
                }
                tracing::info!(
                    daemon_pid,
                    ?spawn_reason,
                    "GUI-owned daemon force-killed via CommandChild"
                );
                return Ok(true);
            }
        }
    }

    fn take_owned_child(&self) -> Option<OwnedDaemonChild> {
        match self.0.child.lock() {
            Ok(mut guard) => guard.take(),
            Err(poisoned) => {
                tracing::error!(
                    "Mutex poisoned in GuiOwnedDaemonState::take_owned_child, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                guard.take()
            }
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn begin_exit_cleanup_is_idempotent_until_finished() {
        let state = GuiOwnedDaemonState::default();

        assert!(state.begin_exit_cleanup());
        assert!(!state.begin_exit_cleanup());

        state.finish_exit_cleanup();

        assert!(state.begin_exit_cleanup());
        state.finish_exit_cleanup();
    }
}
