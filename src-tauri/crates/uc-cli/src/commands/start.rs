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
    if let Some(code) = check_setup_complete(json, verbose).await {
        return code;
    }

    if foreground {
        run_foreground(json, verbose).await
    } else {
        run_background(json).await
    }
}

/// Block `start` if Space setup hasn't completed for the active
/// profile. Delegates the actual check to
/// [`uc_bootstrap::is_setup_complete`] so the file paths + JSON schema
/// stay encoded in `uc-infra::FileSetupStatusRepository`, not duplicated
/// here.
///
/// Returns `Some(exit_code)` to block, `None` to proceed.
async fn check_setup_complete(json: bool, _verbose: bool) -> Option<i32> {
    // Resolution failure (e.g. missing app dirs) → let daemon surface
    // the underlying error rather than masking it here.
    if uc_bootstrap::is_setup_complete().await.unwrap_or(true) {
        return None;
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
            "Error: setup not complete. Run `uniclip init` (new Space) or \
             `uniclip join` (existing Space) first, then retry `start`."
        );
    }
    Some(exit_codes::EXIT_ERROR)
}

async fn run_background(json: bool) -> i32 {
    run_start_background_with(
        || local_daemon::ensure_local_daemon_running(),
        || uc_daemon_local::process_metadata::read_pid_metadata().map(|opt| opt.map(|m| m.pid)),
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
        let pid = uc_daemon_local::process_metadata::read_pid_metadata()
            .ok()
            .flatten()
            .map(|m| m.pid);
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
