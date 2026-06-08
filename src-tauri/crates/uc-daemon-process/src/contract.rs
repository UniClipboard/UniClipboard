//! GUI-framework agnostic contract types for daemon process coordination.

use std::process::Command;

use thiserror::Error;

/// 一次健康探测的分类结果。
///
/// ADR-008 P5-L L2: the pure classifier (`ProbeOutcome` +
/// `classify_health_response` + `running_daemon_is_strictly_newer`) was lifted
/// into the iroh/diesel-free `uc-daemon-contract::probe` so `uc-cli` can depend
/// on it without welding the iroh edge into the CLI build. Re-exported here so
/// existing `uc_daemon_local::contract::ProbeOutcome` consumers (uc-desktop)
/// keep compiling unchanged.
pub use uc_daemon_contract::probe::ProbeOutcome;

/// 桌面侧 daemon 拉起 / 监督流程中可能产生的错误。
#[derive(Debug, Error)]
pub enum DaemonBootstrapError {
    #[error("failed to initialize daemon HTTP probe client: {0}")]
    Client(anyhow::Error),
    #[error("failed to probe daemon health: {0}")]
    Probe(anyhow::Error),
    #[error("incompatible daemon is already running: {details}")]
    IncompatibleDaemon { details: String },
    /// ADR-008 P4-7 (OQ-downgrade-rollback): the running daemon is a strictly
    /// newer version than this client. The incumbent (higher version) wins — a
    /// lower-version client must NOT terminate it, as that would silently
    /// downgrade the running daemon. We refuse to take over instead of killing.
    #[error(
        "refusing to downgrade: running daemon {observed} is newer than this client {expected} \
         — restart to converge, or re-upgrade the client"
    )]
    RefusedNewerDaemon { observed: String, expected: String },
    #[error("failed to spawn uniclipboard-daemon: {0}")]
    Spawn(anyhow::Error),
    #[error("daemon startup timed out after {timeout_ms}ms")]
    StartupTimeout { timeout_ms: u64 },
    #[error("failed to load daemon connection info: {0}")]
    ConnectionInfo(anyhow::Error),
}

/// `terminate_local_daemon_pid` 的返回错误，仅承载一个 detail string。
#[derive(Debug)]
pub struct TerminateDaemonError(pub String);

impl std::fmt::Display for TerminateDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TerminateDaemonError {}

/// 通过平台原生命令向指定 PID 发送 SIGTERM（或 Windows 上 taskkill /F）。
///
/// 使用 `std::process::Command`——不依赖任何 GUI 框架或 sidecar 体系，
/// 可以被 daemon 二进制、GUI shell、CLI 工具任意一方消费。
pub fn terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    // Unix `kill -TERM 0` broadcasts to the caller's entire process group,
    // which would take down the GUI/CLI host. A corrupted PID file reading
    // as 0 must never reach `kill`.
    if pid == 0 {
        return Err(TerminateDaemonError(
            "refusing to terminate pid 0".to_string(),
        ));
    }

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

/// Terminate a daemon PID and block until the process has fully exited.
///
/// On Windows, `taskkill /F` sends `TerminateProcess` which is immediate, but
/// the OS may keep the process object (and its file locks) alive briefly while
/// reclaiming resources. This function polls until the PID is no longer present,
/// so callers can safely overwrite the daemon binary afterwards.
///
/// On Unix this is a no-op after sending SIGTERM — the kernel allows overwriting
/// a running binary (the old inode stays alive until the process exits).
pub fn terminate_and_wait(
    pid: u32,
    timeout: std::time::Duration,
) -> Result<(), TerminateDaemonError> {
    terminate_local_daemon_pid(pid)?;

    #[cfg(windows)]
    {
        use std::time::Instant;

        let deadline = Instant::now() + timeout;
        let poll_interval = std::time::Duration::from_millis(100);

        loop {
            if !is_pid_running_win(pid) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(TerminateDaemonError(format!(
                    "daemon pid {pid} did not exit within {}ms after taskkill",
                    timeout.as_millis()
                )));
            }
            std::thread::sleep(poll_interval);
        }
    }

    #[cfg(not(windows))]
    {
        let _ = timeout;
        Ok(())
    }
}

#[cfg(windows)]
fn is_pid_running_win(pid: u32) -> bool {
    let output = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output();
    match output {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // When PID is absent, tasklist prints a line containing "INFO:".
            // When PID is present, it prints the process row (no "INFO:").
            !stdout.contains("INFO:")
        }
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::types::{DaemonResidency, HealthResponse};

    fn sample_health() -> HealthResponse {
        HealthResponse {
            status: "ok".to_string(),
            package_version: "0.6.0".to_string(),
            api_revision: "rev-1".to_string(),
            residency: DaemonResidency::Standalone,
        }
    }

    #[test]
    fn probe_outcome_compatible_carries_health_payload() {
        let health = sample_health();
        let outcome = ProbeOutcome::Compatible(health.clone());

        match outcome {
            ProbeOutcome::Compatible(h) => assert_eq!(h, health),
            other => panic!("expected Compatible, got {other:?}"),
        }
    }

    #[test]
    fn probe_outcome_eq_distinguishes_variants_and_payload() {
        assert_eq!(ProbeOutcome::Absent, ProbeOutcome::Absent);
        assert_ne!(
            ProbeOutcome::Absent,
            ProbeOutcome::Compatible(sample_health())
        );

        let a = ProbeOutcome::Incompatible {
            details: "bad version".into(),
            observed_package_version: Some("0.5.0".into()),
            observed_api_revision: None,
        };
        let b = a.clone();
        assert_eq!(a, b, "Clone must produce an equal value");
    }

    #[test]
    fn daemon_bootstrap_error_messages_include_context() {
        let err = DaemonBootstrapError::IncompatibleDaemon {
            details: "version mismatch".into(),
        };
        assert!(
            err.to_string().contains("version mismatch"),
            "error display must surface details so caller logs are actionable; got: {err}"
        );

        let err = DaemonBootstrapError::StartupTimeout { timeout_ms: 8_000 };
        assert!(
            err.to_string().contains("8000"),
            "timeout display must include the configured timeout in ms; got: {err}"
        );

        // RefusedNewerDaemon must name both versions so logs make the
        // downgrade-refusal actionable (which side is newer, what to do).
        let err = DaemonBootstrapError::RefusedNewerDaemon {
            observed: "0.15.0".into(),
            expected: "0.14.0".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("0.15.0") && msg.contains("0.14.0") && msg.contains("refusing"),
            "refusal display must surface observed + expected versions; got: {err}"
        );
    }

    #[test]
    fn terminate_daemon_error_is_an_error_with_passthrough_display() {
        let err = TerminateDaemonError("kill failed: ESRCH".into());
        assert_eq!(err.to_string(), "kill failed: ESRCH");

        // Confirms it satisfies `std::error::Error` so callers can box it.
        let boxed: Box<dyn std::error::Error> = Box::new(err);
        assert!(boxed.to_string().contains("ESRCH"));
    }

    #[test]
    fn terminate_local_daemon_pid_refuses_pid_zero() {
        // Guard rail: a corrupted PID file reading as 0 must not reach
        // `kill -TERM 0` (which would signal the entire process group).
        let err = terminate_local_daemon_pid(0)
            .expect_err("pid 0 must be rejected before any signal is sent");
        assert!(
            err.to_string().contains("pid 0"),
            "error must explain why pid 0 was refused; got: {err}"
        );
    }

    #[test]
    fn terminate_local_daemon_pid_fails_for_nonexistent_pid() {
        // PID 1 (init) is owned by root and unsignalable from a normal user;
        // PID 0 means "every process in the group" — we don't want that.
        // Use a likely-unused high PID so the kill/taskkill exits non-zero
        // without any chance of hitting our own process.
        let result = terminate_local_daemon_pid(999_999_999);
        let err = result.expect_err("targeting a non-existent pid must fail");
        let msg = err.to_string();
        assert!(
            msg.contains("999999999"),
            "error must name the pid we tried to terminate; got: {msg}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn terminate_local_daemon_pid_signals_a_real_child_process() {
        use std::process::{Command, Stdio};
        use std::thread;
        use std::time::{Duration, Instant};

        // Spawn a long-running child we can prove we killed. `sleep 60` is
        // available on every supported unix; we wait_with_output to avoid
        // leaking the process if the test fails before we reap.
        let mut child = Command::new("sleep")
            .arg("60")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep");
        let pid = child.id();

        terminate_local_daemon_pid(pid).expect("terminate_local_daemon_pid must succeed");

        // Wait for the child to actually exit. Bound it so a stuck test fails
        // loudly instead of hanging CI.
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match child.try_wait().expect("try_wait") {
                Some(_status) => return,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    panic!("child {pid} did not exit after SIGTERM");
                }
                None => thread::sleep(Duration::from_millis(20)),
            }
        }
    }
}
