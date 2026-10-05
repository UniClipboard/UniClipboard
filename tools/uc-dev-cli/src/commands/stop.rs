//! Stop command -- terminates the running daemon gracefully via SIGTERM.

use std::fmt;
use std::time::Duration;

use serde::Serialize;
use uc_daemon_process::process_metadata::{DaemonPidMetadata, DaemonProcessMode};

use crate::exit_codes;
use crate::output;

const STOP_TIMEOUT: Duration = Duration::from_secs(10);
const STOP_POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Serialize)]
pub struct StopOutput {
    pub status: &'static str,
    /// Populated when `status == "managed_by_gui"` so the JSON consumer can
    /// surface "the daemon is owned by GUI process N" without having to
    /// re-read the PID file.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

impl fmt::Display for StopOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status, self.pid) {
            ("stopped", _) => write!(f, "Daemon stopped"),
            ("not_running", _) => write!(f, "Daemon is not running"),
            ("managed_by_gui", Some(pid)) => write!(
                f,
                "Daemon (pid {pid}) is running inside the UniClipboard GUI. \
                 Quit the GUI from its tray menu instead — `uniclip stop` \
                 won't kill it."
            ),
            ("managed_by_gui", None) => write!(
                f,
                "Daemon is running inside the UniClipboard GUI. \
                 Quit the GUI from its tray menu instead."
            ),
            (status, Some(pid)) => write!(f, "Daemon {} (pid {})", status, pid),
            (status, None) => write!(f, "Daemon {}", status),
        }
    }
}

/// Run the stop command.
pub async fn run(json: bool, _verbose: bool) -> i32 {
    run_stop_with(
        || uc_daemon_process::process_metadata::read_pid_metadata(),
        |meta| verify_identity(meta),
        |pid| send_sigterm(pid),
        json,
    )
    .await
}

/// D22 identity-aware liveness check: verify the PID is alive AND belongs
/// to a daemon binary. Returns `true` only if both conditions hold.
fn verify_identity(metadata: &DaemonPidMetadata) -> bool {
    use uc_daemon_process::process_metadata::{verify_pid_identity, PidVerification};
    matches!(verify_pid_identity(metadata), PidVerification::Active)
}

/// Testable inner implementation that accepts injectable closures.
///
/// `read_metadata` returns `Result<Option<DaemonPidMetadata>>` — the daemon
/// PID + process mode if a daemon is registered.
/// `is_daemon_active` checks whether the metadata points to a live daemon
/// (D22: liveness + executable identity verification).
/// `send_sigterm` sends SIGTERM to the given PID; returns `true` on success.
pub(crate) async fn run_stop_with<ReadMetadata, IsActive, SendSignal>(
    read_metadata: ReadMetadata,
    is_daemon_active: IsActive,
    send_sigterm: SendSignal,
    json: bool,
) -> i32
where
    ReadMetadata: FnOnce() -> anyhow::Result<Option<DaemonPidMetadata>>,
    IsActive: Fn(&DaemonPidMetadata) -> bool,
    SendSignal: FnOnce(u32) -> bool,
{
    // Step 1: Read PID metadata.
    let metadata = match read_metadata() {
        Ok(None) => {
            let out = StopOutput {
                status: "not_running",
                pid: None,
            };
            if let Err(e) = output::print_result(&out, json) {
                eprintln!("Error: {}", e);
                return exit_codes::EXIT_ERROR;
            }
            return exit_codes::EXIT_SUCCESS;
        }
        Ok(Some(metadata)) => metadata,
        Err(e) => {
            eprintln!("Error: failed to read daemon PID file: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    let pid = metadata.pid;

    // Step 2: Stale-PID guard — daemon registered itself but the OS process
    // is gone or the PID has been recycled by a non-daemon process (D22:
    // liveness + exe identity verification). Treat as not running.
    //
    // Must run BEFORE the InProcess check below: a crashed GUI leaves stale
    // InProcess metadata behind, and reporting "managed_by_gui" for a dead
    // process would tell the user to "quit the GUI from its tray menu" when
    // there's no GUI to quit.
    if !is_daemon_active(&metadata) {
        let out = StopOutput {
            status: "not_running",
            pid: None,
        };
        if let Err(e) = output::print_result(&out, json) {
            eprintln!("Error: {}", e);
            return exit_codes::EXIT_ERROR;
        }
        return exit_codes::EXIT_SUCCESS;
    }

    // Step 3: If the daemon is alive AND running inside a GUI shell (in-process
    // mode), refuse to SIGTERM it. Killing it would tear down the entire GUI
    // process and is almost never what the user wants — the GUI's own quit path
    // is the correct shutdown route.
    if matches!(metadata.mode, DaemonProcessMode::InProcess) {
        let out = StopOutput {
            status: "managed_by_gui",
            pid: Some(pid),
        };
        if let Err(e) = output::print_result(&out, json) {
            eprintln!("Error: {}", e);
            return exit_codes::EXIT_ERROR;
        }
        return exit_codes::EXIT_ERROR;
    }

    // Step 4: Send SIGTERM.
    if !send_sigterm(pid) {
        eprintln!("Error: failed to send stop signal to daemon (pid {})", pid);
        return exit_codes::EXIT_ERROR;
    }

    // Step 5: Poll until process exits or timeout.
    let deadline = std::time::Instant::now() + STOP_TIMEOUT;
    loop {
        tokio::time::sleep(STOP_POLL_INTERVAL).await;

        if !is_daemon_active(&metadata) {
            break;
        }

        if std::time::Instant::now() >= deadline {
            eprintln!(
                "Warning: daemon (pid {}) did not stop within {}s. You may need to terminate it manually.",
                pid,
                STOP_TIMEOUT.as_secs()
            );
            return exit_codes::EXIT_ERROR;
        }
    }

    // Step 6: Report success.
    let out = StopOutput {
        status: "stopped",
        pid: Some(pid),
    };
    if let Err(e) = output::print_result(&out, json) {
        eprintln!("Error: {}", e);
        return exit_codes::EXIT_ERROR;
    }

    exit_codes::EXIT_SUCCESS
}

#[cfg(unix)]
pub(crate) fn send_sigterm(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM) == 0 }
}

#[cfg(windows)]
pub(crate) fn send_sigterm(pid: u32) -> bool {
    std::process::Command::new("taskkill")
        .args(["/PID", &pid.to_string()])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn metadata(pid: u32, mode: DaemonProcessMode) -> DaemonPidMetadata {
        DaemonPidMetadata {
            pid,
            mode,
            started_at_ms: 0,
            spawned_by: uc_daemon_process::process_metadata::DaemonSpawnOrigin::Unknown,
            package_version: String::new(),
        }
    }

    #[tokio::test]
    async fn refuses_to_sigterm_in_process_daemon() {
        // Killing the GUI's hosted daemon would also kill the GUI, so `stop`
        // must refuse and exit with EXIT_ERROR (forces shell scripts to
        // notice rather than silently doing nothing).
        let signal_sent = Arc::new(AtomicBool::new(false));
        let signal_sent_for_closure = signal_sent.clone();

        let exit = run_stop_with(
            || Ok(Some(metadata(4242, DaemonProcessMode::InProcess))),
            |_meta| true,
            move |_pid| {
                signal_sent_for_closure.store(true, Ordering::SeqCst);
                true
            },
            true, // json output (silenced from terminal)
        )
        .await;

        assert_eq!(exit, exit_codes::EXIT_ERROR);
        assert!(
            !signal_sent.load(Ordering::SeqCst),
            "SIGTERM must not be sent for an in-process daemon"
        );
    }

    #[tokio::test]
    async fn standalone_daemon_receives_sigterm() {
        let signal_sent = Arc::new(AtomicBool::new(false));
        let signal_sent_for_closure = signal_sent.clone();
        // Process is gone immediately after SIGTERM — keeps the test fast.
        let process_alive = Arc::new(AtomicBool::new(true));
        let process_alive_check = process_alive.clone();

        let exit = run_stop_with(
            || Ok(Some(metadata(7777, DaemonProcessMode::Standalone))),
            move |_meta| process_alive_check.load(Ordering::SeqCst),
            move |_pid| {
                signal_sent_for_closure.store(true, Ordering::SeqCst);
                process_alive.store(false, Ordering::SeqCst);
                true
            },
            true,
        )
        .await;

        assert_eq!(exit, exit_codes::EXIT_SUCCESS);
        assert!(
            signal_sent.load(Ordering::SeqCst),
            "standalone daemon must receive SIGTERM"
        );
    }

    #[tokio::test]
    async fn no_daemon_running_returns_success() {
        let exit = run_stop_with(|| Ok(None), |_pid| false, |_pid| false, true).await;
        assert_eq!(exit, exit_codes::EXIT_SUCCESS);
    }

    #[tokio::test]
    async fn stale_in_process_metadata_reports_not_running_not_managed_by_gui() {
        // A crashed GUI leaves InProcess PID metadata behind. Without a
        // liveness check before the InProcess gate, `stop` would tell the
        // user to "quit the GUI from its tray menu" when there is no GUI
        // to quit. The fix is to check `is_process_running` first.
        let signal_sent = Arc::new(AtomicBool::new(false));
        let signal_sent_for_closure = signal_sent.clone();

        let exit = run_stop_with(
            || Ok(Some(metadata(4242, DaemonProcessMode::InProcess))),
            |_meta| false, // GUI crashed — process is gone, metadata stale
            move |_pid| {
                signal_sent_for_closure.store(true, Ordering::SeqCst);
                true
            },
            true,
        )
        .await;

        assert_eq!(
            exit,
            exit_codes::EXIT_SUCCESS,
            "stale metadata for a dead InProcess daemon must report success — \
             nothing to stop, not an error"
        );
        assert!(
            !signal_sent.load(Ordering::SeqCst),
            "no SIGTERM should fly when the process is already gone"
        );
    }

    #[tokio::test]
    async fn stale_standalone_metadata_reports_not_running() {
        // Same scenario for Standalone mode — daemon binary crashed and left
        // its PID file behind. Should be reported as not_running, not as a
        // SIGTERM target (kill of a dead pid would error out the spawn).
        let signal_sent = Arc::new(AtomicBool::new(false));
        let signal_sent_for_closure = signal_sent.clone();

        let exit = run_stop_with(
            || Ok(Some(metadata(4242, DaemonProcessMode::Standalone))),
            |_meta| false,
            move |_pid| {
                signal_sent_for_closure.store(true, Ordering::SeqCst);
                true
            },
            true,
        )
        .await;

        assert_eq!(exit, exit_codes::EXIT_SUCCESS);
        assert!(!signal_sent.load(Ordering::SeqCst));
    }
}
