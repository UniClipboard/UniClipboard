//! TestDaemon — spawn, health-wait, and kill a `uniclipd` process for testing.

use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus};
use std::time::Duration;

use serde::Deserialize;
use uc_daemon_process::process_metadata::{DaemonSpawnOrigin, SPAWN_ORIGIN_ENV};
use uc_daemon_process::socket::DAEMON_CONN_FORMAT;

use crate::{DaemonEndpointDiscovery, NodeBinarySet, TestProfile};

const LEGACY_PROFILE_HTTP_PORT_START: u16 = 42719;

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
    uses_legacy_fixed_port: bool,
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
            uses_legacy_fixed_port: binaries.endpoint_discovery
                == DaemonEndpointDiscovery::FixedProfilePort,
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
        Self::start_clean_configured_with(profile, binaries, rendezvous_base_url, |_| {}).await
    }

    /// Start from userdata that was already restored into this exact profile.
    pub async fn start_preserving_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
    ) -> Result<Self, String> {
        Self::start_preserving_configured_with(profile, binaries, rendezvous_base_url, |_| {}).await
    }

    /// Spawn on userdata already restored into this profile without waiting
    /// for health, so a test can inspect a daemon whose startup failed.
    pub fn spawn_preserving_configured_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
        configure: impl FnOnce(&mut Command),
    ) -> Result<Self, String> {
        let binary = binaries.daemon.clone();
        let rendezvous_base_url = rendezvous_base_url.map(str::to_string);
        let mut command = Self::command(&profile, &binary, rendezvous_base_url.as_deref())
            .map_err(|error| format!("prepare restored profile failed: {error}"))?;
        configure(&mut command);
        let child = command
            .spawn()
            .map_err(|error| format!("spawn restored profile failed: {error}"))?;
        Ok(Self {
            child: Some(child),
            profile,
            port: 0,
            binary,
            uses_legacy_fixed_port: binaries.endpoint_discovery
                == DaemonEndpointDiscovery::FixedProfilePort,
            rendezvous_base_url,
        })
    }

    pub async fn start_preserving_configured_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
        configure: impl FnOnce(&mut Command),
    ) -> Result<Self, String> {
        let mut daemon = Self::spawn_preserving_configured_with(
            profile,
            binaries,
            rendezvous_base_url,
            configure,
        )?;
        daemon.wait_ready().await?;
        Ok(daemon)
    }

    pub async fn start_clean_configured_with(
        profile: TestProfile,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
        configure: impl FnOnce(&mut Command),
    ) -> Result<Self, String> {
        let mut daemon = Self::spawn_clean_with(profile, binaries, rendezvous_base_url, configure)
            .map_err(|e| format!("spawn failed: {e}"))?;
        daemon.wait_ready().await?;
        Ok(daemon)
    }

    pub async fn restart_preserving(&mut self) -> Result<(), String> {
        self.kill();
        self.start_configured().await
    }

    pub async fn restart_preserving_configured_with(
        &mut self,
        configure: impl FnOnce(&mut Command),
    ) -> Result<(), String> {
        self.kill();
        let mut command = Self::command(
            &self.profile,
            &self.binary,
            self.rendezvous_base_url.as_deref(),
        )
        .map_err(|e| format!("prepare configured restart failed: {e}"))?;
        configure(&mut command);
        self.child = Some(
            command
                .spawn()
                .map_err(|e| format!("configured restart spawn failed: {e}"))?,
        );
        self.wait_ready().await
    }

    /// Restart on the same userdata without waiting for health, so a test can
    /// inspect a restart whose startup fails.
    pub fn respawn_preserving_configured_with(
        &mut self,
        configure: impl FnOnce(&mut Command),
    ) -> Result<(), String> {
        self.kill();
        let mut command = Self::command(
            &self.profile,
            &self.binary,
            self.rendezvous_base_url.as_deref(),
        )
        .map_err(|e| format!("prepare configured respawn failed: {e}"))?;
        configure(&mut command);
        self.child = Some(
            command
                .spawn()
                .map_err(|e| format!("configured respawn failed: {e}"))?,
        );
        Ok(())
    }

    pub async fn restart_preserving_with_system_clipboard(&mut self) -> Result<(), String> {
        self.kill();
        let mut command = Self::command(
            &self.profile,
            &self.binary,
            self.rendezvous_base_url.as_deref(),
        )
        .map_err(|e| format!("prepare desktop restart failed: {e}"))?;
        command.env_remove("UC_DAEMON_RUN_MODE");
        self.child = Some(
            command
                .spawn()
                .map_err(|e| format!("desktop restart spawn failed: {e}"))?,
        );
        self.wait_ready().await
    }

    pub async fn restart_preserving_with(
        &mut self,
        binaries: &NodeBinarySet,
        rendezvous_base_url: Option<&str>,
    ) -> Result<(), String> {
        self.kill();
        self.binary = binaries.daemon.clone();
        self.uses_legacy_fixed_port =
            binaries.endpoint_discovery == DaemonEndpointDiscovery::FixedProfilePort;
        self.rendezvous_base_url = rendezvous_base_url.map(str::to_string);
        self.start_configured().await
    }

    pub async fn restart_preserving_as_gui_with(
        &mut self,
        binaries: &NodeBinarySet,
    ) -> Result<(), String> {
        self.kill();
        self.binary = binaries.daemon.clone();
        self.uses_legacy_fixed_port =
            binaries.endpoint_discovery == DaemonEndpointDiscovery::FixedProfilePort;
        self.rendezvous_base_url = None;
        let mut command = Self::command(&self.profile, &self.binary, None)
            .map_err(|e| format!("prepare GUI-origin restart failed: {e}"))?;
        command
            .env_remove("UC_DAEMON_RUN_MODE")
            .env(SPAWN_ORIGIN_ENV, DaemonSpawnOrigin::Gui.as_env_str());
        self.child = Some(
            command
                .spawn()
                .map_err(|e| format!("GUI-origin restart spawn failed: {e}"))?,
        );
        self.wait_ready().await
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
        self.wait_ready().await
    }

    /// Wait until `daemon.conn` is published and `/health` returns success.
    ///
    /// Both failures include the daemon process log. Conn-publish failures
    /// already carry the child exit status and a log tail.
    async fn wait_ready(&mut self) -> Result<(), String> {
        if let Err(error) = self.wait_for_endpoint(Duration::from_secs(30)).await {
            return Err(format!("{error}\n{}", self.diagnostic_log()));
        }
        if let Err(error) = self.wait_healthy(Duration::from_secs(30)).await {
            return Err(format!("{error}\n{}", self.diagnostic_log()));
        }
        Ok(())
    }

    async fn wait_for_endpoint(&mut self, timeout: Duration) -> Result<(), String> {
        if self.uses_legacy_fixed_port {
            self.port = legacy_port_for_profile(&self.profile.name);
            return Ok(());
        }
        self.wait_for_conn(timeout).await
    }

    /// Poll until the daemon has published `daemon.conn` (ADR-011) and record
    /// its port.
    ///
    /// The connection file is accepted only when its recorded PID matches the
    /// spawned child process — a leftover `daemon.conn` from a previously
    /// killed daemon (same profile) must not be mistaken for the new daemon's
    /// publish.
    ///
    /// If the child exits first, this returns immediately. A dead process will
    /// not publish the file, and the exit status is the evidence.
    pub async fn wait_for_conn(&mut self, timeout: Duration) -> Result<(), String> {
        let conn_path = self.profile.data_dir().join("daemon.conn");
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let expected_pid = self.child.as_ref().map(Child::id);
            if let Some(port) = accepted_conn_port(&conn_path, expected_pid) {
                self.port = port;
                return Ok(());
            }
            let child = observe_child(self.child.as_mut());
            if child.stopped() {
                return Err(self.conn_failure(child.headline(), &conn_path, expected_pid, &child));
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(self.conn_failure(
                    &format!(
                        "daemon.conn not published within {}s at {}",
                        timeout.as_secs(),
                        conn_path.display()
                    ),
                    &conn_path,
                    expected_pid,
                    &child,
                ));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    fn conn_failure(
        &self,
        headline: &str,
        conn_path: &Path,
        expected_pid: Option<u32>,
        child: &ChildObservation,
    ) -> String {
        render_conn_failure(
            headline,
            conn_path,
            expected_pid,
            child,
            &process_log_tail(&self.profile.process_log_path()),
        )
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

    /// Suspend the daemon process owned by this harness instance.
    #[cfg(unix)]
    pub fn suspend(&mut self) -> std::io::Result<()> {
        let child = self.child.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "daemon is not running")
        })?;
        if unsafe { libc::kill(child.id() as i32, libc::SIGSTOP) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// Resume a daemon previously suspended with [`Self::suspend`].
    #[cfg(unix)]
    pub fn resume(&mut self) -> std::io::Result<()> {
        let child = self.child.as_mut().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "daemon is not running")
        })?;
        if unsafe { libc::kill(child.id() as i32, libc::SIGCONT) } == 0 {
            Ok(())
        } else {
            Err(std::io::Error::last_os_error())
        }
    }

    /// Kill the daemon process.
    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }

    /// Ask the daemon to shut down cleanly so SQLite can checkpoint before a
    /// profile is snapshotted. A timeout is an error and never falls through
    /// to packaging a potentially incomplete profile.
    pub async fn stop_gracefully(&mut self) -> Result<(), String> {
        let Some(child) = self.child.as_mut() else {
            return Ok(());
        };
        #[cfg(unix)]
        if unsafe { libc::kill(child.id() as i32, libc::SIGTERM) } != 0 {
            return Err(format!(
                "send daemon SIGTERM failed: {}",
                std::io::Error::last_os_error()
            ));
        }
        #[cfg(not(unix))]
        child
            .kill()
            .map_err(|error| format!("stop daemon failed: {error}"))?;

        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            let exited = self
                .child
                .as_mut()
                .expect("daemon child disappeared during graceful stop")
                .try_wait()
                .map_err(|error| format!("wait for daemon shutdown failed: {error}"))?
                .is_some();
            if exited {
                self.child = None;
                return Ok(());
            }
            if tokio::time::Instant::now() >= deadline {
                self.kill();
                return Err("daemon did not shut down cleanly within 10s".to_string());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
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

const PROCESS_LOG_TAIL_BYTES: u64 = 8 * 1024;

enum ChildObservation {
    Missing,
    Running { pid: u32 },
    Exited { pid: u32, status: ExitStatus },
    WaitFailed { pid: u32, error: String },
}

impl ChildObservation {
    fn stopped(&self) -> bool {
        matches!(self, Self::Exited { .. } | Self::WaitFailed { .. })
    }

    fn headline(&self) -> &'static str {
        match self {
            Self::Exited { .. } => "daemon exited before publishing daemon.conn",
            Self::WaitFailed { .. } => {
                "failed to observe the daemon process before daemon.conn was published"
            }
            Self::Missing | Self::Running { .. } => "daemon.conn not published",
        }
    }

    /// `(exited, pid, exit detail)` for the failure message.
    fn lines(&self) -> (&'static str, String, String) {
        match self {
            Self::Missing => ("unknown", "<none>".to_string(), "<no child>".to_string()),
            Self::Running { pid } => ("no", pid.to_string(), "<still running>".to_string()),
            Self::Exited { pid, status } => ("yes", pid.to_string(), exit_label(status)),
            Self::WaitFailed { pid, error } => (
                "unknown",
                pid.to_string(),
                format!("<wait failed: {error}>"),
            ),
        }
    }
}

fn exit_label(status: &ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => status.to_string(),
    }
}

fn observe_child(child: Option<&mut Child>) -> ChildObservation {
    let Some(child) = child else {
        return ChildObservation::Missing;
    };
    let pid = child.id();
    match child.try_wait() {
        Ok(Some(status)) => ChildObservation::Exited { pid, status },
        Ok(None) => ChildObservation::Running { pid },
        Err(error) => ChildObservation::WaitFailed {
            pid,
            error: error.to_string(),
        },
    }
}

enum ConnLoad {
    Missing,
    Unreadable(String),
    Parsed(DaemonConn),
}

fn load_conn(path: &Path) -> ConnLoad {
    match std::fs::read_to_string(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ConnLoad::Missing,
        Err(error) => ConnLoad::Unreadable(error.to_string()),
        Ok(content) => match serde_json::from_str::<DaemonConn>(&content) {
            Ok(conn) => ConnLoad::Parsed(conn),
            Err(error) => ConnLoad::Unreadable(error.to_string()),
        },
    }
}

fn accepted_conn_port(path: &Path, expected_pid: Option<u32>) -> Option<u16> {
    let ConnLoad::Parsed(conn) = load_conn(path) else {
        return None;
    };
    if conn.format == DAEMON_CONN_FORMAT
        && !conn.token.is_empty()
        && expected_pid.is_none_or(|pid| conn.pid == pid)
    {
        Some(conn.port)
    } else {
        None
    }
}

fn rejection_note(conn: &DaemonConn, expected_pid: Option<u32>) -> String {
    let mut reasons = Vec::new();
    if conn.format != DAEMON_CONN_FORMAT {
        reasons.push(format!("format {}", conn.format));
    }
    if conn.token.is_empty() {
        reasons.push("empty token".to_string());
    }
    if expected_pid.is_some_and(|pid| conn.pid != pid) {
        reasons.push("pid mismatch".to_string());
    }
    if reasons.is_empty() {
        String::new()
    } else {
        format!("conn rejected: {}", reasons.join(", "))
    }
}

fn render_conn_failure(
    headline: &str,
    conn_path: &Path,
    expected_pid: Option<u32>,
    child: &ChildObservation,
    log_tail: &str,
) -> String {
    let (exited, child_pid, child_exit) = child.lines();
    let expected = match expected_pid {
        Some(pid) => pid.to_string(),
        None => "<none>".to_string(),
    };
    let (exists, conn_pid, note) = match load_conn(conn_path) {
        ConnLoad::Missing => ("no", "<absent>".to_string(), String::new()),
        ConnLoad::Unreadable(error) => ("unknown", format!("<unreadable: {error}>"), String::new()),
        ConnLoad::Parsed(conn) => {
            let token = if conn.token.is_empty() { "no" } else { "yes" };
            let mut note = format!("conn format: {}\nconn token present: {token}", conn.format);
            let rejected = rejection_note(&conn, expected_pid);
            if !rejected.is_empty() {
                note.push('\n');
                note.push_str(&rejected);
            }
            ("yes", conn.pid.to_string(), note)
        }
    };
    let mut message = format!(
        "{headline}\n\
         conn path: {}\n\
         child exited: {exited}\n\
         child pid: {child_pid}\n\
         child exit: {child_exit}\n\
         expected pid: {expected}\n\
         conn file exists: {exists}\n\
         conn pid: {conn_pid}\n",
        conn_path.display()
    );
    if !note.is_empty() {
        message.push_str(&note);
        message.push('\n');
    }
    message.push_str("daemon process log tail:\n");
    message.push_str(log_tail);
    if !log_tail.ends_with('\n') {
        message.push('\n');
    }
    message
}

fn process_log_tail(path: &Path) -> String {
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(error) => return format!("<failed to read daemon process log: {error}>"),
    };
    let length = match file.metadata() {
        Ok(metadata) => metadata.len(),
        Err(error) => return format!("<failed to read daemon process log: {error}>"),
    };
    let truncated = length > PROCESS_LOG_TAIL_BYTES;
    if truncated {
        let start = length - PROCESS_LOG_TAIL_BYTES;
        if file.seek(SeekFrom::Start(start)).is_err() {
            return "<failed to read daemon process log tail>".to_string();
        }
    }
    let mut bytes = Vec::new();
    if file.read_to_end(&mut bytes).is_err() {
        return "<failed to read daemon process log tail>".to_string();
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if truncated {
        if let Some(index) = text.find('\n') {
            text.drain(..=index);
        }
        format!("<truncated to last {PROCESS_LOG_TAIL_BYTES} bytes>\n{text}")
    } else {
        text
    }
}

fn legacy_port_for_profile(profile: &str) -> u16 {
    let slot_count = u32::from(u16::MAX) - u32::from(LEGACY_PROFILE_HTTP_PORT_START) + 1;
    let hash = profile
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |current, byte| {
            (current ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        });
    LEGACY_PROFILE_HTTP_PORT_START + (hash % u64::from(slot_count)) as u16
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(test)]
mod tests {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    use super::{accepted_conn_port, legacy_port_for_profile, process_log_tail, TestDaemon};
    use crate::{NodeBinarySet, TestProfile};

    #[test]
    fn legacy_profile_port_is_stable_and_in_reserved_range() {
        let port = legacy_port_for_profile("dev-upgrade-v0191-u01-fixed");
        assert_eq!(port, legacy_port_for_profile("dev-upgrade-v0191-u01-fixed"));
        assert!(port >= 42719);
    }

    #[test]
    fn process_log_tail_keeps_the_end_and_drops_the_head() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("e2e-daemon-process.log");
        let mut body = String::from("HEAD-MARKER\n");
        while body.len() <= super::PROCESS_LOG_TAIL_BYTES as usize + 2048 {
            body.push_str("padding line ------------------------------------------------\n");
        }
        body.push_str("TAIL-MARKER\n");
        std::fs::write(&path, body).unwrap();

        let tail = process_log_tail(&path);
        assert!(tail.contains("TAIL-MARKER"), "{tail}");
        assert!(!tail.contains("HEAD-MARKER"), "{tail}");
        assert!(tail.contains("truncated"), "{tail}");
    }

    #[test]
    fn process_log_tail_reports_a_missing_file() {
        let tail = process_log_tail(std::path::Path::new("/no/such/e2e-daemon-process.log"));
        assert!(tail.contains("failed to read daemon process log"), "{tail}");
    }

    #[test]
    fn accepted_conn_requires_format_token_and_matching_pid() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daemon.conn");
        assert!(accepted_conn_port(&path, Some(4)).is_none());

        write_conn(&path, 4, "token", 2, 1000);
        assert!(accepted_conn_port(&path, Some(4)).is_none());

        write_conn(&path, 4, "", 1, 1000);
        assert!(accepted_conn_port(&path, Some(4)).is_none());

        write_conn(&path, 9, "token", 1, 1000);
        assert!(accepted_conn_port(&path, Some(4)).is_none());

        write_conn(&path, 4, "token", 1, 4321);
        assert_eq!(accepted_conn_port(&path, Some(4)), Some(4321));
        assert_eq!(accepted_conn_port(&path, None), Some(4321));
    }

    #[tokio::test]
    async fn wait_for_conn_fails_fast_when_the_child_exits() {
        let profile = TestProfile::new("conn-exit");
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        std::fs::write(profile.process_log_path(), "exit-tail\n").unwrap();
        let mut command = if cfg!(windows) {
            let mut command = Command::new("cmd");
            command.args(["/C", "exit 7"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "exit 7"]);
            command
        };
        let child = command.spawn().expect("spawn exiting child");
        let mut daemon = parked(profile, Some(child));
        let started = Instant::now();
        let error = daemon
            .wait_for_conn(Duration::from_secs(30))
            .await
            .expect_err("an exited child cannot publish daemon.conn");
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "conn wait should fail at exit, not at the deadline: {error}"
        );
        assert!(
            error.contains("daemon exited before publishing daemon.conn"),
            "{error}"
        );
        assert!(error.contains("child exited: yes"), "{error}");
        assert!(error.contains("exit code 7"), "{error}");
        assert!(error.contains("conn file exists: no"), "{error}");
        assert!(error.contains("conn pid: <absent>"), "{error}");
        assert!(error.contains("exit-tail"), "{error}");
    }

    #[tokio::test]
    async fn wait_for_conn_reports_a_pid_mismatch_without_the_token() {
        let profile = TestProfile::new("conn-mismatch");
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        std::fs::write(profile.process_log_path(), "mismatch-tail\n").unwrap();
        let child = spawn_sleeper();
        let expected = child.id();
        let conn_path = profile.data_dir().join("daemon.conn");
        write_conn(&conn_path, 1, "super-secret-token", 1, 9);
        let mut daemon = parked(profile, Some(child));
        let started = Instant::now();
        let error = daemon
            .wait_for_conn(Duration::from_millis(300))
            .await
            .expect_err("a leftover conn pid must not be accepted");
        assert!(started.elapsed() < Duration::from_secs(3), "{error}");
        assert!(
            error.contains("daemon.conn not published within"),
            "{error}"
        );
        assert!(error.contains("child exited: no"), "{error}");
        assert!(error.contains("<still running>"), "{error}");
        assert!(
            error.contains(&format!("expected pid: {expected}")),
            "{error}"
        );
        assert!(error.contains("conn file exists: yes"), "{error}");
        assert!(error.contains("conn pid: 1\n"), "{error}");
        assert!(error.contains("conn rejected: pid mismatch"), "{error}");
        assert!(error.contains("conn token present: yes"), "{error}");
        assert!(!error.contains("super-secret-token"), "{error}");
        assert!(error.contains("mismatch-tail"), "{error}");
    }

    #[tokio::test]
    async fn wait_for_conn_accepts_a_matching_publication() {
        let profile = TestProfile::new("conn-match");
        std::fs::create_dir_all(profile.data_dir()).unwrap();
        let child = spawn_sleeper();
        let pid = child.id();
        let conn_path = profile.data_dir().join("daemon.conn");
        write_conn(&conn_path, pid, "token", 1, 4321);
        let mut daemon = parked(profile, Some(child));
        daemon
            .wait_for_conn(Duration::from_secs(2))
            .await
            .expect("matching daemon.conn should be accepted");
        assert_eq!(daemon.port(), 4321);
    }

    #[tokio::test]
    async fn start_clean_attaches_exit_status_and_the_process_log() {
        let (program, args) = noisy_exiting_shell();
        let binaries = NodeBinarySet::fixed("diag", "unused-cli", program);
        let started = Instant::now();
        let error = match TestDaemon::start_clean_configured_with(
            TestProfile::new("conn-clean"),
            &binaries,
            None,
            |command| {
                command.args(args);
            },
        )
        .await
        {
            Err(error) => error,
            Ok(_) => panic!("the clean start path must surface a conn failure"),
        };
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "clean start should fail when the child exits: {error}"
        );
        assert!(
            error.contains("daemon exited before publishing daemon.conn"),
            "{error}"
        );
        assert!(error.contains("child exited: yes"), "{error}");
        assert!(error.contains("exit code 9"), "{error}");
        assert!(error.contains("conn file exists: no"), "{error}");
        assert!(error.contains("TAIL-MARKER"), "{error}");
        let tail_at = error
            .find("TAIL-MARKER")
            .expect("tail marker present in failure");
        let head_at = error
            .find("FULL-LOG-HEAD-MARKER")
            .expect("full process log should be attached on the clean start path");
        assert!(
            tail_at < head_at,
            "the log tail should come before the full process log repeats the head\n{error}"
        );
    }

    fn parked(profile: TestProfile, child: Option<std::process::Child>) -> TestDaemon {
        TestDaemon {
            child,
            profile,
            port: 0,
            binary: std::path::PathBuf::from("unused"),
            uses_legacy_fixed_port: false,
            rendezvous_base_url: None,
        }
    }

    fn spawn_sleeper() -> std::process::Child {
        let mut command = if cfg!(windows) {
            let mut command = Command::new("powershell");
            command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 30"]);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", "sleep 30"]);
            command
        };
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleeper")
    }

    fn write_conn(path: &std::path::Path, pid: u32, token: &str, format: u32, port: u16) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let body = serde_json::json!({
            "format": format,
            "host": "127.0.0.1",
            "port": port,
            "token": token,
            "pid": pid,
            "startedAtMs": 1
        });
        std::fs::write(path, serde_json::to_vec(&body).unwrap()).unwrap();
    }

    fn noisy_exiting_shell() -> (String, Vec<String>) {
        let script = if cfg!(windows) {
            r#"
                Write-Output 'FULL-LOG-HEAD-MARKER'
                1..500 | ForEach-Object { Write-Output 'padding ------------------------------------------------' }
                Write-Output 'TAIL-MARKER'
                exit 9
            "#
        } else {
            r#"
                printf '%s\n' 'FULL-LOG-HEAD-MARKER'
                i=0
                while [ "$i" -lt 500 ]; do
                  printf '%s\n' 'padding ------------------------------------------------'
                  i=$((i + 1))
                done
                printf '%s\n' 'TAIL-MARKER'
                exit 9
            "#
        };
        if cfg!(windows) {
            (
                "powershell".to_string(),
                vec![
                    "-NoProfile".to_string(),
                    "-Command".to_string(),
                    script.to_string(),
                ],
            )
        } else {
            ("sh".to_string(), vec!["-c".to_string(), script.to_string()])
        }
    }
}
