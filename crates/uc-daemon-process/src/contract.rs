//! GUI-framework agnostic contract types for daemon process coordination.

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
    /// The daemon binary launched but its process exited BEFORE it could serve
    /// `/health` — a hard launch failure, NOT a slow start. On Windows this is
    /// how a missing dependency surfaces: `CreateProcess` (hence
    /// `Command::spawn`) returns `Ok`, then the image loader fails and the
    /// process dies within milliseconds with an NTSTATUS exit code (e.g.
    /// `0xC0000135` STATUS_DLL_NOT_FOUND when `vcruntime140.dll` is absent). A
    /// naive `/health` timeout cannot tell this apart from "the daemon is just
    /// slow to start"; watching the child's exit lets us fail fast with an
    /// actionable message. `code` is the process exit code if the OS reported
    /// one (issue #1259).
    #[error("daemon process exited immediately after spawn{}", describe_spawn_exit(.code))]
    SpawnExitedEarly { code: Option<i32> },
    #[error("daemon startup timed out after {timeout_ms}ms")]
    StartupTimeout { timeout_ms: u64 },
    #[error("failed to load daemon connection info: {0}")]
    ConnectionInfo(anyhow::Error),
}

/// Render the actionable suffix for [`DaemonBootstrapError::SpawnExitedEarly`].
///
/// A handful of Windows loader NTSTATUS codes are recognizable enough to name
/// the likely fix (a missing/incompatible runtime library). Everything else
/// just reports the raw exit code. NTSTATUS error codes have the high bit set,
/// so they read as negative `i32`s — those are rendered as unsigned hex
/// (`0xC0000135`, the conventional form); ordinary small exit codes (a Rust
/// panic is 101, a clean-ish crash may be 1) render as decimal.
fn describe_spawn_exit(code: &Option<i32>) -> String {
    let Some(code) = *code else {
        return String::new();
    };
    if code >= 0 {
        return format!(" (exit code {code})");
    }
    let ntstatus = code as u32;
    let hint = match ntstatus {
        // STATUS_DLL_NOT_FOUND: a required DLL could not be located. On a clean
        // Windows install the usual culprit is the absent Visual C++ Runtime
        // (`vcruntime140.dll`) — the proximate trigger in issue #1259.
        0xC000_0135 => {
            " - a required system library is missing (for example the Visual C++ Runtime). \
             Install the Microsoft Visual C++ 2015-2022 Redistributable (x64) and try again."
        }
        // STATUS_ENTRYPOINT_NOT_FOUND: a DLL was found but lacks an expected
        // entry point — typically a version mismatch of that same runtime.
        0xC000_0139 => {
            " - a required system library is present but incompatible. \
             Reinstall the Microsoft Visual C++ 2015-2022 Redistributable (x64) and try again."
        }
        // STATUS_DLL_INIT_FAILED: a dependent DLL's initialization routine
        // failed. Not always the VC++ runtime, so keep the advice generic.
        0xC000_0142 => {
            " - a required system library failed to initialize. \
             Reinstall the app's runtime dependencies and try again."
        }
        _ => "",
    };
    format!(" (exit code {ntstatus:#010X}){hint}")
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

/// 向指定 PID 发送终止信号：Unix 上 `kill(2)` 系统调用发 SIGTERM，Windows
/// 上 Win32 `TerminateProcess`（经 [`crate::win_process`]）。
///
/// 不依赖任何 GUI 框架或 sidecar 体系，可以被 daemon 二进制、GUI shell、
/// CLI 工具任意一方消费。两侧都曾 shell-out 到平台命令行工具
/// (`kill`/`taskkill`)——除了多一次 fork+exec 和对 PATH 的依赖，shell-out
/// 把 errno 折叠成了 locale 相关的 stderr 文本；直接系统调用没有这些问题。
pub fn terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    // Unix `kill(0, SIGTERM)` broadcasts to the caller's entire process
    // group, which would take down the GUI/CLI host. A corrupted PID file
    // reading as 0 must never reach the syscall.
    if pid == 0 {
        return Err(TerminateDaemonError(
            "refusing to terminate pid 0".to_string(),
        ));
    }

    #[cfg(unix)]
    {
        send_sigterm(pid)
            .map_err(|e| TerminateDaemonError(format!("failed to terminate pid {pid}: {e}")))
    }

    #[cfg(windows)]
    {
        crate::win_process::terminate_pid(pid).map_err(TerminateDaemonError)
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(TerminateDaemonError(format!(
            "process termination unsupported on this platform (pid {pid})"
        )))
    }
}

/// Force-kill `pid`: Unix `SIGKILL` (uncatchable, immediate), Windows
/// `TerminateProcess` (already forceful). Used by single-instance eviction as
/// the escalation when a `SIGTERM`'d holder does not release the instance lock
/// within the grace window. The flock is reclaimed by the kernel the moment the
/// process dies, so a contender parked in `flock(2)` wakes immediately after.
pub fn force_terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    // Same pid-0 guard as `terminate_local_daemon_pid`: a corrupted PID file
    // reading as 0 would broadcast to the caller's whole process group.
    if pid == 0 {
        return Err(TerminateDaemonError(
            "refusing to force-terminate pid 0".to_string(),
        ));
    }

    #[cfg(unix)]
    {
        send_sigkill(pid)
            .map_err(|e| TerminateDaemonError(format!("failed to force-terminate pid {pid}: {e}")))
    }

    #[cfg(windows)]
    {
        crate::win_process::terminate_pid(pid).map_err(TerminateDaemonError)
    }

    #[cfg(not(any(unix, windows)))]
    {
        Err(TerminateDaemonError(format!(
            "process termination unsupported on this platform (pid {pid})"
        )))
    }
}

/// Send SIGTERM to `pid` via the raw `kill(2)` syscall. `Err` carries the OS
/// error (`ESRCH` = no such process, `EPERM` = exists but unsignalable).
#[cfg(unix)]
fn send_sigterm(pid: u32) -> Result<(), std::io::Error> {
    // A pid above pid_t::MAX can only come from a corrupted PID file; the
    // `as` cast would wrap it negative, and kill(2) on a negative pid
    // signals the process *group* |pid| — reject before the cast.
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "pid exceeds pid_t"))?;
    // SAFETY: kill(2) has no preconditions; callers guard pid != 0.
    if unsafe { libc::kill(pid, libc::SIGTERM) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Send SIGKILL to `pid` via the raw `kill(2)` syscall. `Err` carries the OS
/// error (`ESRCH` = no such process, `EPERM` = exists but unsignalable).
#[cfg(unix)]
fn send_sigkill(pid: u32) -> Result<(), std::io::Error> {
    // A pid above pid_t::MAX can only come from a corrupted PID file; the `as`
    // cast would wrap it negative, and kill(2) on a negative pid signals the
    // process *group* |pid| — reject before the cast.
    let pid = libc::pid_t::try_from(pid)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidInput, "pid exceeds pid_t"))?;
    // SAFETY: kill(2) has no preconditions; callers guard pid != 0.
    if unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Terminate a daemon PID and block until the process has fully exited.
///
/// On Windows this terminates via Win32 and then waits on the **process
/// handle** (`WaitForSingleObject`), which signals at true kernel teardown —
/// after that the executable's file lock is released and an installer can
/// safely overwrite the daemon binary. Two generations of shell-out
/// implementations got this wrong: polling `tasklist` flashed a console
/// window per 100ms poll from the no-console GUI host, and parsing its
/// locale-dependent no-match banner misread dead PIDs as alive on non-English
/// Windows, aborting every update install.
///
/// On Unix this does not wait after sending SIGTERM — the kernel allows
/// overwriting a running binary (the old inode stays alive until the process
/// exits). On both platforms an already-exited PID is `Ok` (the goal state),
/// unlike the signal-only [`terminate_local_daemon_pid`], which reports `Err`
/// for an unknown PID.
pub fn terminate_and_wait(
    pid: u32,
    timeout: std::time::Duration,
) -> Result<(), TerminateDaemonError> {
    if pid == 0 {
        return Err(TerminateDaemonError(
            "refusing to terminate pid 0".to_string(),
        ));
    }

    #[cfg(windows)]
    {
        // Combined terminate+wait over one handle: no TOCTOU between signal
        // and wait.
        crate::win_process::terminate_and_wait_pid(pid, timeout).map_err(TerminateDaemonError)
    }

    #[cfg(unix)]
    {
        let _ = timeout;
        match send_sigterm(pid) {
            Ok(()) => Ok(()),
            // ESRCH: the process is already gone — the goal state of a
            // kill-and-wait, matching the Windows arm's Ok when OpenProcess
            // finds no such PID.
            Err(e) if e.raw_os_error() == Some(libc::ESRCH) => Ok(()),
            Err(e) => Err(TerminateDaemonError(format!(
                "failed to terminate pid {pid}: {e}"
            ))),
        }
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = timeout;
        terminate_local_daemon_pid(pid)
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
    fn spawn_exited_early_names_missing_dll_and_the_fix() {
        // The signature case (issue #1259): a Windows missing-DLL crash exits
        // with STATUS_DLL_NOT_FOUND (0xC0000135, negative as i32). The message
        // must render the code as unsigned hex AND point at the likely fix so
        // the frontend error screen is actionable, not just "it timed out".
        let err = DaemonBootstrapError::SpawnExitedEarly {
            code: Some(0xC000_0135u32 as i32),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("0xC0000135"),
            "must render the NTSTATUS as unsigned hex; got: {msg}"
        );
        assert!(
            msg.contains("Visual C++"),
            "must name the actionable fix for a missing runtime; got: {msg}"
        );
        assert!(
            msg.contains("exited immediately"),
            "must distinguish an immediate crash from a slow start; got: {msg}"
        );
    }

    #[test]
    fn spawn_exited_early_renders_plain_codes_in_decimal() {
        // A non-NTSTATUS exit (e.g. a Rust panic aborts with 101) has no
        // loader-specific hint — report the raw code in decimal, not hex.
        let err = DaemonBootstrapError::SpawnExitedEarly { code: Some(101) };
        let msg = err.to_string();
        assert!(
            msg.contains("101"),
            "plain exit code must appear; got: {msg}"
        );
        assert!(
            !msg.contains("0x"),
            "a plain positive code must not be hex-rendered; got: {msg}"
        );
    }

    #[test]
    fn spawn_exited_early_tolerates_unknown_exit_code() {
        // No OS-reported code: still a distinct, non-panicking message.
        let err = DaemonBootstrapError::SpawnExitedEarly { code: None };
        assert!(err.to_string().contains("exited immediately"));

        // An unrecognized NTSTATUS still renders as hex, just without a hint.
        let err = DaemonBootstrapError::SpawnExitedEarly {
            code: Some(0xC000_00FDu32 as i32), // STATUS_STACK_OVERFLOW
        };
        assert!(err.to_string().contains("0xC00000FD"));
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

    #[test]
    fn terminate_and_wait_treats_missing_pid_as_already_exited() {
        // Kill-and-wait's goal state is "the process is gone" — a PID that
        // never existed already satisfies it. Unix maps ESRCH to Ok; Windows
        // maps a failed OpenProcess to Ok. The signal-only function keeps
        // reporting Err for the same input (covered above).
        terminate_and_wait(999_999_999, std::time::Duration::from_millis(100))
            .expect("a non-existent pid must read as already exited");
    }

    #[cfg(unix)]
    #[test]
    fn terminate_local_daemon_pid_rejects_pid_beyond_pid_t() {
        // u32::MAX would wrap negative through an `as pid_t` cast, turning
        // kill(2) into a process-*group* signal. The guard must reject it
        // before the syscall.
        let err = terminate_local_daemon_pid(u32::MAX)
            .expect_err("a pid beyond pid_t::MAX must be rejected");
        assert!(
            err.to_string().contains("pid_t"),
            "error must explain the overflow rejection; got: {err}"
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
