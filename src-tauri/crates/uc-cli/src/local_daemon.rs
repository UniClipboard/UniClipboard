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

/// Spawn `uniclipd` as a **detached** background process.
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
    let daemon_exe = resolve_daemon_exe_path()?;

    let mut command = Command::new(&daemon_exe);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    configure_detached(&mut command);

    let child = command.spawn().map_err(|error| {
        LocalDaemonError::Spawn(anyhow::Error::new(error).context(format!(
            "failed to spawn daemon via `{}`",
            daemon_exe.display()
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

/// Resolve the path to the `uniclipd` daemon binary.
///
/// Strategy:
/// 1. Look for `uniclipd` (or `uniclipd.exe` on Windows) as a sibling of
///    the current CLI executable. This covers Tauri sidecar bundles, `cargo
///    build` output directories, and Docker images where both binaries sit
///    in the same directory.
/// 2. Fall back to a PATH lookup so that system-wide installs work.
pub(crate) fn resolve_daemon_exe_path() -> Result<PathBuf, LocalDaemonError> {
    let daemon_name = if cfg!(windows) {
        "uniclipd.exe"
    } else {
        "uniclipd"
    };

    // Strategy 1: sibling of current executable.
    if let Ok(cli_exe) = std::env::current_exe() {
        if let Some(dir) = cli_exe.parent() {
            let candidate = dir.join(daemon_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    // Strategy 2: PATH lookup.
    which::which(daemon_name).map_err(|error| {
        LocalDaemonError::ResolveBinary(anyhow::Error::new(error).context(format!(
            "`{daemon_name}` not found as sibling of the CLI binary or in PATH"
        )))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // ---------- Display impl ----------

    #[test]
    fn display_startup_timeout_includes_profile_and_url() {
        let err = LocalDaemonError::StartupTimeout {
            timeout_ms: 30_000,
            profile: Some("dev".into()),
            base_url: "http://127.0.0.1:7321".into(),
        };
        let s = err.to_string();
        assert!(s.contains("30000"), "must include timeout in ms: {s}");
        assert!(s.contains("dev"), "must include profile name: {s}");
        assert!(
            s.contains("http://127.0.0.1:7321"),
            "must include base URL: {s}"
        );
    }

    #[test]
    fn display_startup_timeout_falls_back_to_default_profile_label() {
        let err = LocalDaemonError::StartupTimeout {
            timeout_ms: 1_000,
            profile: None,
            base_url: "http://localhost:9".into(),
        };
        let s = err.to_string();
        assert!(
            s.contains("default"),
            "missing profile must surface as 'default' label, not blank: {s}"
        );
    }

    #[test]
    fn display_passthrough_for_anyhow_wrapped_variants() {
        let err = LocalDaemonError::Probe(anyhow::anyhow!("connection refused"));
        let s = err.to_string();
        assert!(s.contains("connection refused"));
        assert!(s.contains("probe"), "Probe variant must self-identify: {s}");
    }

    // ---------- LocalDaemonSession PartialEq ----------

    #[test]
    fn session_partial_eq_distinguishes_spawned_flag() {
        let a = LocalDaemonSession {
            base_url: "http://1.2.3.4:5".into(),
            spawned: true,
        };
        let b = LocalDaemonSession {
            base_url: "http://1.2.3.4:5".into(),
            spawned: false,
        };
        assert_ne!(a, b, "spawned=true vs spawned=false must compare unequal");
    }

    // ---------- wait_for_daemon_health ----------

    #[tokio::test]
    async fn wait_returns_immediately_on_first_healthy_probe() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok::<bool, LocalDaemonError>(true)
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
        )
        .await
        .expect("first probe true must resolve as Ok");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "must short-circuit on first healthy probe — wastes startup time otherwise"
        );
    }

    #[tokio::test]
    async fn wait_polls_until_probe_turns_healthy() {
        // Simulate cold start: first 2 probes false (daemon still spawning),
        // then true.
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                let n = calls.fetch_add(1, Ordering::SeqCst);
                Ok::<bool, LocalDaemonError>(n >= 2)
            }
        };

        wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
        )
        .await
        .expect("eventually-healthy probe must resolve");
        assert!(calls.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test(start_paused = true)]
    async fn wait_times_out_with_full_diagnostic_context() {
        // start_paused freezes wall clock so the test doesn't actually wait.
        // env var UC_PROFILE is captured into the timeout error — set it to
        // assert it propagates.
        // SAFETY: tests run with `--test-threads=1` for `set_var`/`remove_var` to be safe.
        // In Rust 2024 edition, std::env::set_var is unsafe; this crate is on edition 2021.
        std::env::set_var("UC_PROFILE", "ci-profile");
        let mut probe = || async { Ok::<bool, LocalDaemonError>(false) };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_millis(500),
            Duration::from_millis(50),
            "http://example:1234",
        )
        .await
        .expect_err("never-healthy probe must produce StartupTimeout");

        std::env::remove_var("UC_PROFILE");

        match err {
            LocalDaemonError::StartupTimeout {
                timeout_ms,
                profile,
                base_url,
            } => {
                assert_eq!(timeout_ms, 500);
                assert_eq!(profile.as_deref(), Some("ci-profile"));
                assert_eq!(base_url, "http://example:1234");
            }
            other => panic!("expected StartupTimeout, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wait_propagates_probe_error_without_retry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_closure = calls.clone();
        let mut probe = move || {
            let calls = calls_for_closure.clone();
            async move {
                calls.fetch_add(1, Ordering::SeqCst);
                Err::<bool, _>(LocalDaemonError::Probe(anyhow::anyhow!("network down")))
            }
        };

        let err = wait_for_daemon_health(
            &mut probe,
            Duration::from_secs(5),
            Duration::from_millis(1),
            "http://test",
        )
        .await
        .expect_err("probe error must propagate, not be retried");

        assert!(matches!(err, LocalDaemonError::Probe(_)));
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "transport-level errors are not transient — wait must not retry"
        );
    }

    // ---------- resolve_daemon_exe_path ----------

    #[test]
    fn resolve_daemon_exe_path_finds_sibling_or_path() {
        // In a cargo test environment `uniclipd` may or may not be built.
        // We only assert the function doesn't panic — the actual resolution
        // depends on the build layout.
        let _result = resolve_daemon_exe_path();
    }
}
