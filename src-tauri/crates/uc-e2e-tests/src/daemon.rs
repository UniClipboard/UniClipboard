//! TestDaemon — spawn, health-wait, and kill a `uniclipd` process for testing.

use std::process::{Child, Command};
use std::time::Duration;

use crate::profile::TestProfile;

const PROFILE_HTTP_PORT_START: u16 = 42719;

/// Manages a `uniclipd` daemon process for a single test.
pub struct TestDaemon {
    child: Option<Child>,
    pub profile: TestProfile,
    port: u16,
}

impl TestDaemon {
    /// Locate the `uniclipd` binary. Assumes `cargo build -p uc-daemon` has run.
    fn binary_path() -> String {
        let target_dir = std::env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| {
            let manifest = env!("CARGO_MANIFEST_DIR");
            format!("{}/../../target", manifest)
        });
        let bin_name = if cfg!(windows) {
            "uniclipd.exe"
        } else {
            "uniclipd"
        };
        format!("{}/debug/{}", target_dir, bin_name)
    }

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
        let binary = Self::binary_path();
        let port = Self::port_for_profile(&profile.name);

        profile.cleanup();

        let child = Command::new(&binary)
            .env("UC_PROFILE", &profile.name)
            .env("RUST_LOG", "warn")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;

        Ok(Self {
            child: Some(child),
            profile,
            port,
        })
    }

    /// Spawn and wait until the daemon reports healthy (or timeout).
    pub async fn start(profile: TestProfile) -> Result<Self, String> {
        let mut daemon = Self::spawn(profile).map_err(|e| format!("spawn failed: {e}"))?;
        daemon.wait_healthy(Duration::from_secs(30)).await?;
        Ok(daemon)
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
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.kill();
    }
}
