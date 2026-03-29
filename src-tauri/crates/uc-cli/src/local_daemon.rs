use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use reqwest::Client;
use uc_daemon::api::types::HealthResponse;
use uc_daemon::socket::try_resolve_daemon_http_addr;

const HEALTH_PATH: &str = "/health";
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(8);
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
    let probe_base_url = base_url.clone();

    ensure_local_daemon_running_with(
        || probe_daemon_health(&client, &probe_base_url),
        || spawn_daemon_process().map(|_| ()),
        base_url,
        STARTUP_TIMEOUT,
        POLL_INTERVAL,
    )
    .await
}

async fn ensure_local_daemon_running_with<Probe, ProbeFuture, Spawn>(
    mut probe: Probe,
    spawn: Spawn,
    base_url: String,
    startup_timeout: Duration,
    poll_interval: Duration,
) -> Result<LocalDaemonSession, LocalDaemonError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<bool, LocalDaemonError>>,
    Spawn: FnOnce() -> Result<(), LocalDaemonError>,
{
    if probe().await? {
        return Ok(LocalDaemonSession {
            base_url,
            spawned: false,
        });
    }

    spawn()?;
    wait_for_daemon_health(&mut probe, startup_timeout, poll_interval, &base_url).await?;

    Ok(LocalDaemonSession {
        base_url,
        spawned: true,
    })
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

fn spawn_daemon_process() -> Result<Child, LocalDaemonError> {
    let cli_exe = resolve_cli_exe_path()?;

    Command::new(&cli_exe)
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| {
            LocalDaemonError::Spawn(anyhow::Error::new(error).context(format!(
                "failed to spawn daemon via `{} daemon`",
                cli_exe.display()
            )))
        })
}

/// Resolve the path to the current CLI executable (used to spawn itself with `daemon` subcommand).
pub(crate) fn resolve_cli_exe_path() -> Result<PathBuf, LocalDaemonError> {
    std::env::current_exe().map_err(|error| {
        LocalDaemonError::ResolveBinary(
            anyhow::Error::new(error).context("failed to resolve current CLI executable"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn ensure_local_daemon_running_returns_without_spawn_when_probe_is_healthy() {
        let spawns = Arc::new(AtomicUsize::new(0));
        let session = ensure_local_daemon_running_with(
            || async { Ok(true) },
            {
                let spawns = Arc::clone(&spawns);
                move || {
                    spawns.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
            "http://127.0.0.1:42716".to_string(),
            Duration::from_millis(10),
            Duration::from_millis(1),
        )
        .await
        .expect("healthy daemon should not require spawn");

        assert_eq!(session.base_url, "http://127.0.0.1:42716");
        assert!(!session.spawned);
        assert_eq!(spawns.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn ensure_local_daemon_running_spawns_and_polls_until_healthy() {
        let spawns = Arc::new(AtomicUsize::new(0));
        let probes = Arc::new(AtomicUsize::new(0));
        let session = ensure_local_daemon_running_with(
            {
                let probes = Arc::clone(&probes);
                move || {
                    let probes = Arc::clone(&probes);
                    async move {
                        let attempt = probes.fetch_add(1, Ordering::SeqCst);
                        Ok(attempt >= 2)
                    }
                }
            },
            {
                let spawns = Arc::clone(&spawns);
                move || {
                    spawns.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
            "http://127.0.0.1:42717".to_string(),
            Duration::from_millis(50),
            Duration::from_millis(1),
        )
        .await
        .expect("spawned daemon should become healthy during polling");

        assert!(session.spawned);
        assert_eq!(spawns.load(Ordering::SeqCst), 1);
        assert!(probes.load(Ordering::SeqCst) >= 3);
    }

    #[tokio::test]
    async fn probe_daemon_health_accepts_healthy_response() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let addr = listener
            .local_addr()
            .expect("listener address should resolve");
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.expect("connection should arrive");
            let mut buffer = [0_u8; 1024];
            let _ = stream
                .read(&mut buffer)
                .await
                .expect("request should be readable");
            let body = r#"{"status":"ok","packageVersion":"0.1.0","apiRevision":"v1"}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("response should be written");
        });

        let client = Client::builder()
            .timeout(PROBE_TIMEOUT)
            .build()
            .expect("probe client should build");
        let is_healthy = probe_daemon_health(&client, &format!("http://{addr}"))
            .await
            .expect("probe should succeed");

        assert!(is_healthy);
        server.await.expect("server should finish");
    }
}
