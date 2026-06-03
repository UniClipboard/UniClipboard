use std::fmt;
use std::future::Future;
use std::time::Duration;

use reqwest::Client;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::types::HealthResponse;
use uc_daemon_local::process_metadata::DaemonSpawnOrigin;
use uc_daemon_local::socket::try_resolve_daemon_http_addr;
use uc_daemon_local::spawn::{spawn_detached_daemon, SpawnDaemonError};

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

impl From<SpawnDaemonError> for LocalDaemonError {
    fn from(error: SpawnDaemonError) -> Self {
        match error {
            SpawnDaemonError::ResolveBinary(error) => Self::ResolveBinary(error),
            SpawnDaemonError::Spawn(error) => Self::Spawn(error),
        }
    }
}

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

    if let Err(error) =
        spawn_detached_daemon(DaemonSpawnOrigin::Cli).map_err(LocalDaemonError::from)
    {
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

    // Wire shape (ADR-008 §H): `/health` is now enveloped as
    // `{ data: HealthResponse, ts }`. Decode the envelope and read `data.status`.
    let envelope = response
        .json::<ApiEnvelope<HealthResponse>>()
        .await
        .map_err(|error| {
            LocalDaemonError::Probe(
                anyhow::Error::new(error).context("failed to decode daemon health response"),
            )
        })?;

    Ok(envelope.data.status == "ok")
}

fn resolve_base_url() -> Result<String, LocalDaemonError> {
    let addr = try_resolve_daemon_http_addr().map_err(|error| {
        LocalDaemonError::ResolveAddress(
            error.context("failed to resolve profile-aware daemon HTTP address"),
        )
    })?;
    Ok(format!("http://{}:{}", addr.ip(), addr.port()))
}

/// Detached daemon spawn + binary resolution now live in the shared
/// [`uc_daemon_local::spawn`] module so GUI shells (ADR-008 P3) reuse the exact
/// same `setsid` / `DETACHED_PROCESS` detach semantics. The CLI keeps only the
/// probe→spawn→wait-health orchestration above.

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

    // `resolve_daemon_exe_path` moved to `uc_daemon_local::spawn`; its
    // no-panic test lives there now.
}
