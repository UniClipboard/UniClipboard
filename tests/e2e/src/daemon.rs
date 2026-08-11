//! TestDaemon — spawn, health-wait, and kill a `uniclipd` process for testing.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use serde::Deserialize;
use uc_daemon_process::socket::DAEMON_CONN_FORMAT;

use crate::{NodeBinarySet, TestProfile};

/// The `daemon.conn` payload the daemon publishes (ADR-011) — the single
/// source of truth for the ephemeral port and bearer token.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DaemonConn {
    format: u32,
    #[allow(dead_code)]
    host: String,
    port: u16,
    token: String,
    pid: u32,
    #[allow(dead_code)]
    started_at_ms: u64,
}

/// Manages a `uniclipd` daemon process for a single test.
pub struct TestDaemon {
    child: Option<Child>,
    pub profile: TestProfile,
    port: u16,
    binary: PathBuf,
    rendezvous_base_url: Option<String>,
}

impl TestDaemon {
    /// Spawn a new daemon with the given profile. Does NOT wait for health.
    pub fn spawn(profile: TestProfile) -> std::io::Result<Self> {
        Self::spawn_with(profile, |_| {})
    }

    /// Like [`Self::spawn`], but lets the test adjust the `Command` before it
    /// runs (e.g. `env_remove("DISPLAY")` to simulate a headless session).
    pub fn spawn_with(
        profile: TestProfile,
        configure: impl FnOnce(&mut Command),
    ) -> std::io::Result<Self> {
        let binaries = NodeBinarySet::current();
        Self::spawn_clean_with(profile, &binaries, None, configure)
    }

    pub fn spawn_clean_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
        configure: impl FnOnce(&mut Command),
    ) -> std::io::Result<Self> {
        profile.cleanup();
        let binary = binaries.daemon.clone();
        let rendezvous_base_url = rendezvous_base_url.map(str::to_string);
        let mut command = Self::command(&profile, &binary, rendezvous_base_url.as_deref())?;
        configure(&mut command);
        let child = command.spawn()?;

        Ok(Self {
            child: Some(child),
            profile,
            // Populated by `wait_for_conn`; unknown until the daemon publishes
            // `daemon.conn` (ADR-011 ephemeral port).
            port: 0,
            binary,
            rendezvous_base_url,
        })
    }

    fn command(
        profile: &TestProfile,
        binary: &PathBuf,
        rendezvous_base_url: Option<&str>,
    ) -> std::io::Result<Command> {
        std::fs::create_dir_all(profile.data_dir())?;
        let log = OpenOptions::new()
            .create(true)
            .append(true)
            .open(profile.process_log_path())?;
        let stderr = log.try_clone()?;
        let rust_log = std::env::var("UC_E2E_RUST_LOG").unwrap_or_else(|_| "warn".to_string());
        let mut command = Command::new(binary);
        command
            .env("UC_PROFILE", &profile.name)
            .env("UNICLIPBOARD_ENV", "development")
            .env("UC_DAEMON_RUN_MODE", "server")
            .env("RUST_LOG", rust_log)
            .stdout(log)
            .stderr(stderr);
        if let Some(base_url) = rendezvous_base_url {
            command.env("UC_E2E_RENDEZVOUS_BASE_URL", base_url);
        }
        Ok(command)
    }

    /// Spawn and wait until the daemon publishes `daemon.conn` and reports
    /// healthy (or timeout).
    pub async fn start(profile: TestProfile) -> Result<Self, String> {
        Self::start_clean_with(profile, &NodeBinarySet::current(), None).await
    }

    pub async fn start_clean_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
    ) -> Result<Self, String> {
        let mut daemon = Self::spawn_clean_with(profile, binaries, rendezvous_base_url, |_| {})
            .map_err(|e| format!("spawn failed: {e}"))?;
        daemon.wait_for_conn(Duration::from_secs(30)).await?;
        daemon.wait_healthy(Duration::from_secs(30)).await?;
        Ok(daemon)
    }

    pub async fn restart_preserving(&mut self) -> Result<(), String> {
        self.kill();
        self.start_configured().await
    }

    pub async fn restart_preserving_with(
        &mut self,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
    ) -> Result<(), String> {
        self.kill();
        self.binary = binaries.daemon.clone();
        self.rendezvous_base_url = rendezvous_base_url.map(str::to_string);
        self.start_configured().await
    }

    async fn start_configured(&mut self) -> Result<(), String> {
        let mut command = Self::command(
            &self.profile,
            &self.binary,
            self.rendezvous_base_url.as_deref(),
        )
        .map_err(|e| format!("prepare restart failed: {e}"))?;
        self.child = Some(
            command
                .spawn()
                .map_err(|e| format!("restart spawn failed: {e}"))?,
        );
        self.wait_for_conn(Duration::from_secs(30)).await?;
        self.wait_healthy(Duration::from_secs(30)).await
    }

    /// Poll until the daemon has published `daemon.conn` (ADR-011) and record
    /// its port.
    ///
    /// The connection file is accepted only when its recorded PID matches the
    /// spawned child process — a leftover `daemon.conn` from a previously
    /// killed daemon (same profile) must not be mistaken for the new daemon's
    /// publish.
    pub async fn wait_for_conn(&mut self, timeout: Duration) -> Result<(), String> {
        let conn_path = self.profile.data_dir().join("daemon.conn");
        let expected_pid = self.child.as_ref().map(|child| child.id());
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Ok(content) = std::fs::read_to_string(&conn_path) {
                if let Ok(conn) = serde_json::from_str::<DaemonConn>(&content) {
                    let pid_matches = expected_pid.is_none_or(|pid| conn.pid == pid);
                    if pid_matches
                        && conn.format == DAEMON_CONN_FORMAT
                        && !conn.token.is_empty()
                    {
                        self.port = conn.port;
                        return Ok(());
                    }
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "daemon.conn not published within {}s at {:?}",
                    timeout.as_secs(),
                    conn_path
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// Poll the daemon's health endpoint until it responds 200, or timeout.
    pub async fn wait_healthy(&mut self, timeout: Duration) -> Result<(), String> {
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|e| format!("http client: {e}"))?;

        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(ref mut child) = self.child {
                if let Ok(Some(status)) = child.try_wait() {
                    return Err(format!("daemon exited early with {status}"));
                }
            }

            match client.get(&url).send().await {
                Ok(resp) if resp.status().is_success() => return Ok(()),
                _ => {}
            }

            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "daemon did not become healthy within {}s (port {})",
                    timeout.as_secs(),
                    self.port
                ));
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    /// The base URL for daemon HTTP API.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The HTTP port this daemon is bound to (from `daemon.conn`).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Kill the daemon process.
    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }

    /// Check if the daemon process is still running.
    pub fn is_running(&mut self) -> bool {
        match &mut self.child {
            Some(child) => child.try_wait().ok().flatten().is_none(),
            None => false,
        }
    }

    pub fn diagnostic_log(&self) -> String {
        std::fs::read_to_string(self.profile.process_log_path())
            .unwrap_or_else(|error| format!("<failed to read daemon process log: {error}>"))
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.kill();
    }
}
