//! TestDaemon — spawn, health-wait, and kill a `uniclipd` process for testing.

use std::fs::OpenOptions;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::Duration;

use crate::{NodeBinarySet, TestProfile};

const PROFILE_HTTP_PORT_START: u16 = 42719;

/// Manages a `uniclipd` daemon process for a single test.
pub struct TestDaemon {
    child: Option<Child>,
    pub profile: TestProfile,
    port: u16,
    binary: PathBuf,
    rendezvous_base_url: Option<String>,
}

impl TestDaemon {
    /// Derive the deterministic HTTP port for a profile name (mirrors
    /// `uc-daemon-process/src/socket.rs` resolve logic).
    fn port_for_profile(profile: &str) -> u16 {
        let slot_count = u32::from(u16::MAX) - u32::from(PROFILE_HTTP_PORT_START) + 1;
        let hash = Self::fnv1a(profile);
        let offset = (hash % u64::from(slot_count)) as u16;
        PROFILE_HTTP_PORT_START + offset
    }

    fn fnv1a(s: &str) -> u64 {
        const OFFSET: u64 = 0xcbf29ce484222325;
        const PRIME: u64 = 0x100000001b3;
        s.as_bytes()
            .iter()
            .fold(OFFSET, |h, b| (h ^ u64::from(*b)).wrapping_mul(PRIME))
    }

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
        let port = Self::port_for_profile(&profile.name);
        profile.cleanup();
        let binary = binaries.daemon.clone();
        let rendezvous_base_url = rendezvous_base_url.map(str::to_string);
        let mut command = Self::command(&profile, &binary, rendezvous_base_url.as_deref())?;
        configure(&mut command);
        let child = command.spawn()?;

        Ok(Self {
            child: Some(child),
            profile,
            port,
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

    /// Spawn and wait until the daemon reports healthy AND the `.daemon-token`
    /// file is written (or timeout).
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
        daemon.wait_healthy(Duration::from_secs(30)).await?;
        daemon.wait_for_token(Duration::from_secs(10)).await?;
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
        self.wait_healthy(Duration::from_secs(30)).await?;
        self.wait_for_token(Duration::from_secs(10)).await
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

    /// Poll until the `.daemon-token` file exists and is non-empty.
    pub async fn wait_for_token(&self, timeout: Duration) -> Result<(), String> {
        let token_path = self.profile.data_dir().join(".daemon-token");
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Ok(token) = std::fs::read_to_string(&token_path) {
                if !token.trim().is_empty() {
                    return Ok(());
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(format!(
                    "daemon token not written within {}s at {:?}",
                    timeout.as_secs(),
                    token_path
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    /// The base URL for daemon HTTP API.
    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// The HTTP port this daemon is expected to bind.
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
