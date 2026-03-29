//! Start command -- launches the daemon in background or foreground mode.

use std::fmt;
use std::process::Stdio;

use serde::Serialize;

use crate::exit_codes;
use crate::local_daemon;
use crate::output;

#[derive(Serialize)]
pub struct StartOutput {
    pub status: &'static str,
    pub pid: Option<u32>,
}

impl fmt::Display for StartOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.status, self.pid) {
            ("started", Some(pid)) => write!(f, "Daemon started (pid {})", pid),
            ("already_running", Some(pid)) => write!(f, "Daemon already running (pid {})", pid),
            ("started", None) => write!(f, "Daemon started"),
            ("already_running", None) => write!(f, "Daemon already running"),
            (status, Some(pid)) => write!(f, "Daemon {} (pid {})", status, pid),
            (status, None) => write!(f, "Daemon {}", status),
        }
    }
}

/// Run the start command.
pub async fn run(foreground: bool, json: bool, verbose: bool) -> i32 {
    if let Some(code) = check_setup_complete(json, verbose) {
        return code;
    }

    if foreground {
        run_foreground(json, verbose).await
    } else {
        run_background(json).await
    }
}

/// Check if setup (encryption initialization) is complete before starting the daemon.
///
/// Uses a lightweight file-existence check on `.initialized_encryption` marker
/// instead of building the full CLI runtime, which would trigger keychain
/// access popups on macOS.
///
/// Returns `Some(exit_code)` if start should be blocked, `None` if ok to proceed.
fn check_setup_complete(json: bool, _verbose: bool) -> Option<i32> {
    let paths = match resolve_lightweight_app_paths() {
        Some(p) => p,
        None => return None, // Can't resolve — let daemon handle it
    };

    let marker_file = paths.encryption_marker_path();
    if marker_file.exists() {
        return None; // Setup complete — proceed
    }

    if json {
        let _ = output::print_result(
            &StartOutput {
                status: "setup_required",
                pid: None,
            },
            true,
        );
    } else {
        eprintln!(
            "Error: setup not complete. Run `uniclipboard setup` first to create or join a Space."
        );
    }
    Some(exit_codes::EXIT_ERROR)
}

/// Resolve application paths without building a full runtime.
///
/// Uses `resolve_app_config()` which reads config.toml (dev mode) or falls
/// back to platform-specific app data directory, then builds `AppPaths`
/// for consistent path resolution.
fn resolve_lightweight_app_paths() -> Option<uc_app::app_paths::AppPaths> {
    let config = uc_bootstrap::config_resolution::resolve_app_config().ok()?;
    let platform_dirs = uc_bootstrap::assembly::get_default_app_dirs().ok()?;
    uc_bootstrap::assembly::resolve_app_paths(&platform_dirs, &config).ok()
}

async fn run_background(json: bool) -> i32 {
    run_start_background_with(
        || local_daemon::ensure_local_daemon_running(),
        || uc_daemon::process_metadata::read_pid_file(),
    )
    .await
    .map_or_else(
        |msg| {
            eprintln!("Error: {}", msg);
            exit_codes::EXIT_ERROR
        },
        |output| {
            if let Err(e) = crate::output::print_result(&output, json) {
                eprintln!("Error: {}", e);
                return exit_codes::EXIT_ERROR;
            }
            exit_codes::EXIT_SUCCESS
        },
    )
}

async fn run_foreground(json: bool, _verbose: bool) -> i32 {
    // Check if daemon is already running using probe-only (no spawn).
    // We must NOT use ensure_local_daemon_running() here because it would
    // spawn a background daemon, conflicting with the foreground spawn below.
    if let Ok(true) = local_daemon::probe_running().await {
        let pid = uc_daemon::process_metadata::read_pid_file().ok().flatten();
        let out = StartOutput {
            status: "already_running",
            pid,
        };
        if let Err(e) = output::print_result(&out, json) {
            eprintln!("Error: {}", e);
            return exit_codes::EXIT_ERROR;
        }
        return exit_codes::EXIT_SUCCESS;
    }

    let cli_exe = match local_daemon::resolve_cli_exe_path() {
        Ok(path) => path,
        Err(e) => {
            eprintln!("Error: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    if !json {
        println!("Starting daemon in foreground... (press Ctrl+C to stop)");
    }

    let mut child = match std::process::Command::new(&cli_exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Error: failed to spawn daemon: {}", e);
            return exit_codes::EXIT_ERROR;
        }
    };

    match child.wait() {
        Ok(_) => exit_codes::EXIT_SUCCESS,
        Err(e) => {
            eprintln!("Error: failed to wait for daemon process: {}", e);
            exit_codes::EXIT_ERROR
        }
    }
}

/// Testable inner implementation that accepts injectable closures.
///
/// `ensure_daemon` should probe and/or spawn the daemon, returning a session.
/// `read_pid` should return the daemon PID from the PID file.
pub(crate) async fn run_start_background_with<EnsureDaemon, EnsureFuture, ReadPid>(
    ensure_daemon: EnsureDaemon,
    read_pid: ReadPid,
) -> Result<StartOutput, String>
where
    EnsureDaemon: FnOnce() -> EnsureFuture,
    EnsureFuture: std::future::Future<
        Output = Result<local_daemon::LocalDaemonSession, local_daemon::LocalDaemonError>,
    >,
    ReadPid: FnOnce() -> anyhow::Result<Option<u32>>,
{
    let session = ensure_daemon().await.map_err(|e| e.to_string())?;

    let pid = read_pid().ok().flatten();

    let status = if session.spawned {
        "started"
    } else {
        "already_running"
    };

    Ok(StartOutput { status, pid })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_daemon::{LocalDaemonError, LocalDaemonSession};

    fn healthy_session(spawned: bool) -> Result<LocalDaemonSession, LocalDaemonError> {
        Ok(LocalDaemonSession {
            base_url: "http://127.0.0.1:12345".to_string(),
            spawned,
        })
    }

    #[tokio::test]
    async fn start_background_already_running() {
        let result =
            run_start_background_with(|| async { healthy_session(false) }, || Ok(Some(12345_u32)))
                .await;

        let output = result.expect("should succeed when daemon already running");
        assert_eq!(output.status, "already_running");
        assert_eq!(output.pid, Some(12345));
    }

    #[tokio::test]
    async fn start_background_spawned() {
        let result =
            run_start_background_with(|| async { healthy_session(true) }, || Ok(Some(99999_u32)))
                .await;

        let output = result.expect("should succeed after spawning daemon");
        assert_eq!(output.status, "started");
        assert_eq!(output.pid, Some(99999));
    }

    #[tokio::test]
    async fn start_background_spawn_failure() {
        let result = run_start_background_with(
            || async {
                Err::<LocalDaemonSession, LocalDaemonError>(LocalDaemonError::Spawn(
                    anyhow::anyhow!("binary not found"),
                ))
            },
            || Ok(None),
        )
        .await;

        assert!(result.is_err(), "should return error on spawn failure");
    }

    #[test]
    fn json_output_start_already_running() {
        let out = StartOutput {
            status: "already_running",
            pid: Some(42),
        };
        let json = serde_json::to_string(&out).expect("should serialize");
        assert!(json.contains("\"status\""));
        assert!(json.contains("\"already_running\""));
        assert!(json.contains("\"pid\""));
        assert!(json.contains("42"));
    }

    #[test]
    fn json_output_start_started() {
        let out = StartOutput {
            status: "started",
            pid: Some(1001),
        };
        let json = serde_json::to_string(&out).expect("should serialize");
        assert!(json.contains("\"started\""));
        assert!(json.contains("1001"));
    }

    #[test]
    fn display_output_start() {
        let started = StartOutput {
            status: "started",
            pid: Some(12345),
        };
        assert_eq!(format!("{}", started), "Daemon started (pid 12345)");

        let already_running = StartOutput {
            status: "already_running",
            pid: Some(9876),
        };
        assert_eq!(
            format!("{}", already_running),
            "Daemon already running (pid 9876)"
        );
    }
}
