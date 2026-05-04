use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use reqwest::Client;
use uc_daemon_contract::api::types::HealthResponse;
use uc_daemon_local::socket::try_resolve_daemon_http_addr;

const HEALTH_PATH: &str = "/health";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDaemonSession {
    pub base_url: String,
    pub spawned: bool,
}

#[derive(Debug)]
pub enum LocalDaemonError {
    ProbeClient(anyhow::Error),
    ResolveAddress(anyhow::Error),
    Probe(anyhow::Error),
    ResolveBinary(anyhow::Error),
    Spawn(anyhow::Error),
    StartupTimeout {
        timeout_ms: u64,
        profile: Option<String>,
        base_url: String,
    },
}

impl fmt::Display for LocalDaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProbeClient(error) => write!(
                f,
                "failed to prepare local daemon probe client for setup: {error}"
            ),
            Self::ResolveAddress(error) => {
                write!(
                    f,
                    "failed to resolve profile-aware local daemon address: {error}"
                )
            }
            Self::Probe(error) => {
                write!(f, "failed to probe local daemon health for setup: {error}")
            }
            Self::ResolveBinary(error) => {
                write!(
                    f,
                    "failed to resolve CLI executable for daemon spawn: {error}"
                )
            }
            Self::Spawn(error) => write!(f, "failed to spawn daemon process: {error}"),
            Self::StartupTimeout {
                timeout_ms,
                profile,
                base_url,
            } => {
                let profile = profile.as_deref().unwrap_or("default");
                write!(
                    f,
                    "local daemon did not become healthy within {timeout_ms}ms for profile {profile} at {base_url}"
                )
            }
        }
    }
}

impl std::error::Error for LocalDaemonError {}

/// Probe-only check: returns Ok(true) if the daemon is already healthy, Ok(false) otherwise.
/// Does NOT spawn a daemon process.
pub async fn probe_running() -> Result<bool, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;
    probe_daemon_health(&client, &base_url).await
}

pub async fn ensure_local_daemon_running() -> Result<LocalDaemonSession, LocalDaemonError> {
    let client = Client::builder()
        .timeout(PROBE_TIMEOUT)
        .build()
        .map_err(|error| LocalDaemonError::ProbeClient(error.into()))?;
    let base_url = resolve_base_url()?;

    // Fast path: daemon is already running.
    if probe_daemon_health(&client, &base_url).await? {
        return Ok(LocalDaemonSession {
            base_url,
            spawned: false,
        });
    }

    // Slow path: spawn + wait for health. Show a spinner so the user sees
    // progress — daemon cold start can take many seconds in debug builds.
    let spinner = crate::ui::spinner("Starting local daemon…");

    if let Err(error) = spawn_daemon_process() {
        crate::ui::spinner_finish_error(&spinner, "Failed to spawn local daemon");
        return Err(error);
    }
    // After `spawn_daemon_process` returns, the daemon is its own session
    // leader / process group — the CLI is no longer holding a wait-able
    // handle. The probe loop below is the only proof of life.

    let mut probe = || probe_daemon_health(&client, &base_url);
    match wait_for_daemon_health(&mut probe, STARTUP_TIMEOUT, POLL_INTERVAL, &base_url).await {
        Ok(()) => {
            crate::ui::spinner_finish_success(&spinner, "Local daemon ready");
            Ok(LocalDaemonSession {
                base_url,
                spawned: true,
            })
        }
        Err(error) => {
            crate::ui::spinner_finish_error(&spinner, "Local daemon failed to start");
            Err(error)
        }
    }
}

async fn wait_for_daemon_health<Probe, ProbeFuture>(
    probe: &mut Probe,
    startup_timeout: Duration,
    poll_interval: Duration,
    base_url: &str,
) -> Result<(), LocalDaemonError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<bool, LocalDaemonError>>,
{
    let deadline = tokio::time::Instant::now() + startup_timeout;
    loop {
        if probe().await? {
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(LocalDaemonError::StartupTimeout {
                timeout_ms: startup_timeout.as_millis() as u64,
                profile: std::env::var("UC_PROFILE").ok(),
                base_url: base_url.to_string(),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

async fn probe_daemon_health(client: &Client, base_url: &str) -> Result<bool, LocalDaemonError> {
    let url = format!("{base_url}{HEALTH_PATH}");
    let response = match client.get(url).send().await {
        Ok(response) => response,
        Err(error) if error.is_connect() || error.is_timeout() => return Ok(false),
        Err(error) => {
            return Err(LocalDaemonError::Probe(
                anyhow::Error::new(error).context("daemon health probe request failed"),
            ))
        }
    };

    if !response.status().is_success() {
        return Ok(false);
    }

    let health = response.json::<HealthResponse>().await.map_err(|error| {
        LocalDaemonError::Probe(
            anyhow::Error::new(error).context("failed to decode daemon health response"),
        )
    })?;

    Ok(health.status == "ok")
}

fn resolve_base_url() -> Result<String, LocalDaemonError> {
    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        LocalDaemonError::ResolveAddress(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    Ok(format!("http://{}:{}", addr.ip(), addr.port()))
}

/// Spawn `uniclip daemon` as a **detached** background process.
///
/// "Detached" means the new process survives the CLI exiting — that's the
/// whole point of `uniclip start`. We rely on three pieces:
///
/// 1. `Stdio::null()` on all three streams so the daemon never inherits the
///    terminal — closing the controlling tty must not propagate SIGHUP to it.
/// 2. **Unix**: `setsid()` in a `pre_exec` hook. The child becomes a new
///    session leader detached from the parent's controlling terminal, so
///    `Ctrl+C` / shell exit doesn't reach it. Since the daemon is now session
///    leader of its own session, signals to the *CLI's* process group don't
///    hit it either.
/// 3. **Windows**: `DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP` flags. The
///    daemon gets no console of its own and is a separate process group, so
///    `Ctrl+C` on the parent console doesn't deliver `CTRL_C_EVENT` to it.
///
/// The returned `Child` is intentionally dropped: under Unix that does *not*
/// reap the process (we made it a session leader and cut stdio, the kernel
/// will reap it when it exits — its parent is now PID 1 once the CLI returns).
/// Under Windows the handle just closes; the process keeps running.
fn spawn_daemon_process() -> Result<(), LocalDaemonError> {
    let cli_exe = resolve_cli_exe_path()?;

    let mut command = Command::new(&cli_exe);
    command
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    configure_detached(&mut command);

    let child = command.spawn().map_err(|error| {
        LocalDaemonError::Spawn(anyhow::Error::new(error).context(format!(
            "failed to spawn daemon via `{} daemon`",
            cli_exe.display()
        )))
    })?;

    // Drop the handle deliberately — see fn doc. The detached child runs on
    // its own; the CLI's responsibility ends here. Polling for health is the
    // caller's job.
    drop(child);
    Ok(())
}

#[cfg(unix)]
fn configure_detached(command: &mut Command) {
    use std::os::unix::process::CommandExt;

    // SAFETY: `setsid` is async-signal-safe and only touches process group
    // / session ids. It's the documented way to detach a child from the
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
    // No detachment configured for unknown platforms — the daemon will still
    // be spawned but may receive parent signals. Acceptable as a degraded
    // fallback; uc-cli's main targets (macOS / Linux / Windows) all hit the
    // platform-specific paths above.
}

/// Resolve the path to the current CLI executable (used to spawn itself with `daemon` subcommand).
pub(crate) fn resolve_cli_exe_path() -> Result<PathBuf, LocalDaemonError> {
    std::env::current_exe().map_err(|error| {
        LocalDaemonError::ResolveBinary(
            anyhow::Error::new(error).context("failed to resolve current CLI executable"),
        )
    })
}
