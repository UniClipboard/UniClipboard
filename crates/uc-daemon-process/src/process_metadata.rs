use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use uc_app_paths::app_data_root;

/// 描述 daemon 进程是怎么被拉起的——决定它能不能由 `cli stop` SIGTERM 掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonProcessMode {
    /// 独立 daemon 进程：`cli start` 拉起的 detached 子进程，或者用户在
    /// 终端直接 `uniclipboard-daemon`。`cli stop` 可以安全地 SIGTERM 它。
    Standalone,
    /// in-process daemon：旧版 GUI 在自己的进程里跑 daemon 时写下的标记。
    /// ADR-008 P3-3 (B2'-3) 起 GUI 转纯客户端,**不再产生**此模式;保留它
    /// 只为读取旧版 GUI 留下的 legacy PID 文件——`cli stop` 据此**拒绝**对
    /// 这类 PID 发 SIGTERM(旧 GUI 进程内 daemon,SIGTERM 会把 GUI 一起带挂),
    /// 提示用户去关闭那个 GUI。
    InProcess,
}

/// Environment variable a spawner sets on the detached `uniclipd` child to
/// record who launched it (see [`DaemonSpawnOrigin`]).
pub const SPAWN_ORIGIN_ENV: &str = "UC_DAEMON_SPAWN_ORIGIN";

/// Who launched this daemon process (ADR-008 D3 ownership).
///
/// Persisted in the PID file so that even a *cold-restarted* GUI can tell
/// whether the daemon it attached to is one a GUI brought up (lifecycle-bound,
/// stoppable on full quit) versus a user's own `uniclip start` daemon (an
/// independent service a GUI must never stop).
///
/// Resolved from [`SPAWN_ORIGIN_ENV`], which the spawn primitive
/// ([`crate::spawn::spawn_detached_daemon`]) sets on the child; a manually-run
/// `uniclipd` (or a legacy PID file predating this field) is [`Self::Unknown`]
/// and conservatively treated as **not** GUI-owned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum DaemonSpawnOrigin {
    /// Detached-spawned by a GUI process — its lifecycle is bound to the GUI.
    Gui,
    /// Started by `uniclip start` / headless / oneshot / a service manager —
    /// an independent daemon a GUI must leave running.
    Cli,
    /// Unknown launcher: legacy PID files, or `uniclipd` run directly.
    #[default]
    Unknown,
}

impl DaemonSpawnOrigin {
    /// Stable wire/env token for this origin.
    pub fn as_env_str(self) -> &'static str {
        match self {
            Self::Gui => "gui",
            Self::Cli => "cli",
            Self::Unknown => "unknown",
        }
    }

    /// Resolve the origin of the *current* process from [`SPAWN_ORIGIN_ENV`].
    ///
    /// An unset or unrecognized value yields [`Self::Unknown`] — never panics,
    /// so a daemon launched outside the spawn primitive still writes a valid
    /// (conservative) PID file.
    pub fn from_env() -> Self {
        match std::env::var(SPAWN_ORIGIN_ENV).as_deref() {
            Ok("gui") => Self::Gui,
            Ok("cli") => Self::Cli,
            _ => Self::Unknown,
        }
    }
}

/// daemon 进程元数据（JSON 序列化进 PID 文件）。
///
/// 取代历史上"PID 文件 = 一个 u32 字符串"的格式。`cli stop` / 健康探测
/// 路径会先尝试解析这个 JSON；解析失败时 fall back 到旧 raw-u32 格式
/// 以兼容老版本 daemon 留下的 PID 文件。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonPidMetadata {
    pub pid: u32,
    pub mode: DaemonProcessMode,
    /// Unix epoch milliseconds 时刻——daemon 写 PID 文件那一瞬间。
    /// 只用于诊断（`uniclip status` / 日志），不参与功能判断。
    pub started_at_ms: u64,
    /// Who launched this daemon (ADR-008 D3). `#[serde(default)]` so PID files
    /// predating the field deserialize to [`DaemonSpawnOrigin::Unknown`] — i.e.
    /// conservatively not GUI-owned.
    #[serde(default)]
    pub spawned_by: DaemonSpawnOrigin,
}

impl DaemonPidMetadata {
    /// 构造一份"现在"的元数据：`pid` + `mode` + `spawned_by` + 当前时间戳。
    pub fn now(pid: u32, mode: DaemonProcessMode, spawned_by: DaemonSpawnOrigin) -> Self {
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            pid,
            mode,
            started_at_ms,
            spawned_by,
        }
    }

    /// Whether this daemon was detached-spawned by a GUI (ADR-008 D3).
    ///
    /// The "彻底退出→停" decision combines this with a successful
    /// [`verify_pid_identity`] check; a `cli start` / unknown daemon returns
    /// `false` and is never auto-stopped by a GUI.
    pub fn is_gui_spawned(&self) -> bool {
        self.spawned_by == DaemonSpawnOrigin::Gui
    }
}

/// Leaf filename of the daemon PID file, kept byte-identical to
/// `AppPaths::daemon_pid_path()` (`<app_data_root>/.daemon-pid`).
const DAEMON_PID_FILE_NAME: &str = ".daemon-pid";

/// Provides the process-wide singleton `DaemonPidManager` used by standalone helpers.
fn default_manager() -> Result<&'static DaemonPidManager> {
    static DEFAULT_MANAGER: OnceLock<Result<DaemonPidManager, String>> = OnceLock::new();
    DEFAULT_MANAGER
        .get_or_init(|| {
            resolve_pid_path_from_root()
                .context("failed to resolve application directories")
                .map(DaemonPidManager::new)
                .map_err(|e| format!("{e:#}"))
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Resolve `<app_data_root>/.daemon-pid` via the directory-layout authority
/// (`uc_app_paths::app_data_root`), reproducing `AppPaths::daemon_pid_path()`
/// byte-for-byte. Data-root only — daemon-process does not require the cache dir
/// (the benign P5-0 divergence, preserved).
fn resolve_pid_path_from_root() -> Result<PathBuf> {
    let root = app_data_root().context("the system data-local directory is unavailable")?;
    Ok(root.join(DAEMON_PID_FILE_NAME))
}

/// Manages the daemon PID metadata file lifecycle.
#[derive(Debug, Clone)]
pub struct DaemonPidManager {
    /// Fully-resolved `<app_data_root>/.daemon-pid` path. Stored directly so
    /// this module owns zero app-stack dependencies; path policy is delegated
    /// to [`uc_app_paths::app_data_root`].
    pid_path: PathBuf,
}

impl DaemonPidManager {
    /// Creates a new DaemonPidManager that reads/writes the daemon PID file at
    /// `pid_path`.
    pub fn new(pid_path: PathBuf) -> Self {
        Self { pid_path }
    }

    /// Returns the filesystem path where the daemon PID file for the current app/profile is stored.
    fn pid_path(&self) -> PathBuf {
        self.pid_path.clone()
    }

    /// 写入当前进程的 PID + `mode` 到 PID 文件（JSON 格式）。
    ///
    /// 取代历史上的 `write_current_pid`——把进程模式（`standalone` /
    /// `in_process`）一并落盘，让 `cli stop` 能区分能不能 SIGTERM。
    pub fn write_current_pid_with_mode(&self, mode: DaemonProcessMode) -> Result<u32> {
        let pid_path = self.pid_path();
        let pid = std::process::id();
        // Origin is inherited from the spawner via SPAWN_ORIGIN_ENV (same
        // pattern as UC_HOST_ROLE for the log role); a daemon run outside the
        // spawn primitive resolves to Unknown.
        let metadata = DaemonPidMetadata::now(pid, mode, DaemonSpawnOrigin::from_env());
        let payload = serde_json::to_string(&metadata).with_context(|| {
            format!(
                "failed to serialize daemon pid metadata for {}",
                pid_path.display()
            )
        })?;

        if let Some(parent) = pid_path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create daemon pid directory {}", parent.display())
            })?;
        }

        fs::write(&pid_path, payload)
            .with_context(|| format!("failed to write daemon pid file {}", pid_path.display()))?;

        repair_pid_permissions(&pid_path)?;
        Ok(pid)
    }

    /// Removes the daemon PID metadata file for this manager's configured path.
    ///
    /// If the PID file is missing, this operation succeeds and returns `Ok(())`.
    pub fn remove_pid_file(&self) -> Result<()> {
        let pid_path = self.pid_path();
        match fs::remove_file(&pid_path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(anyhow::Error::new(error).context(format!(
                "failed to remove daemon pid file {}",
                pid_path.display()
            ))),
        }
    }

    /// 读取 PID 文件并返回完整元数据。
    ///
    /// - 文件不存在 → `Ok(None)`
    /// - 是 JSON [`DaemonPidMetadata`] → 直接返回
    /// - 是旧 raw-u32 字符串（升级前 daemon 留下的）→ 当作
    ///   `mode = Standalone, started_at_ms = 0` 兼容返回
    pub fn read_pid_metadata(&self) -> Result<Option<DaemonPidMetadata>> {
        let pid_path = self.pid_path();
        if !pid_path.exists() {
            return Ok(None);
        }

        let raw = fs::read_to_string(&pid_path)
            .with_context(|| format!("failed to read daemon pid file {}", pid_path.display()))?;
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(None);
        }

        if let Ok(metadata) = serde_json::from_str::<DaemonPidMetadata>(trimmed) {
            return Ok(Some(metadata));
        }

        // Backward compat: pre-mode-aware daemons wrote a bare decimal pid.
        // Treat such files as standalone with unknown start time so existing
        // `cli stop` flows still work after upgrade.
        let pid = trimmed.parse::<u32>().with_context(|| {
            format!(
                "failed to parse daemon pid file {} as JSON metadata or u32",
                pid_path.display()
            )
        })?;
        Ok(Some(DaemonPidMetadata {
            pid,
            mode: DaemonProcessMode::Standalone,
            started_at_ms: 0,
            spawned_by: DaemonSpawnOrigin::Unknown,
        }))
    }

    /// Resolve the daemon PID file path used by this manager for tests.
    #[cfg(any(test, feature = "test-helpers"))]
    pub fn pid_path_for_testing(&self) -> PathBuf {
        self.pid_path()
    }
}

/// Ensures the daemon PID file is readable/writable only by the owner (mode 0o600) on Unix; does nothing on non-Unix platforms.
fn repair_pid_permissions(pid_path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let metadata = fs::metadata(pid_path).with_context(|| {
            format!("failed to read daemon pid metadata {}", pid_path.display())
        })?;
        let current_mode = metadata.permissions().mode() & 0o777;
        if current_mode != 0o600 {
            fs::set_permissions(pid_path, fs::Permissions::from_mode(0o600)).with_context(
                || {
                    format!(
                        "failed to repair daemon pid permissions {}",
                        pid_path.display()
                    )
                },
            )?;
        }
    }

    Ok(())
}

// ── PID identity verification (D22, ADR-008) ──────────────────────────

/// Result of verifying whether a PID file's metadata corresponds to a
/// live daemon process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PidVerification {
    /// The PID is alive and its executable looks like a daemon binary.
    Active,
    /// The PID file is stale — the recorded process is gone or does not
    /// match. The file should be removed, **not** sent a signal.
    Stale(StaleReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleReason {
    ProcessNotRunning,
    ExeMismatch {
        expected_suffix: &'static str,
        actual: String,
    },
}

impl std::fmt::Display for StaleReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ProcessNotRunning => write!(f, "process is not running"),
            Self::ExeMismatch {
                expected_suffix,
                actual,
            } => write!(
                f,
                "executable mismatch: expected name ending in `{expected_suffix}`, got `{actual}`"
            ),
        }
    }
}

/// Check whether the daemon described by `metadata` is actually running
/// and is a legitimate daemon binary.
///
/// **D22 iron rule**: callers MUST use this before sending any signal to
/// the PID. If `Stale` is returned, delete the PID file instead.
pub fn verify_pid_identity(metadata: &DaemonPidMetadata) -> PidVerification {
    if !is_pid_alive(metadata.pid) {
        return PidVerification::Stale(StaleReason::ProcessNotRunning);
    }

    if let Some(exe) = read_process_exe(metadata.pid) {
        let name = exe.rsplit(['/', '\\']).next().unwrap_or(&exe);
        if !is_daemon_binary_name(name) {
            return PidVerification::Stale(StaleReason::ExeMismatch {
                expected_suffix: DAEMON_BINARY_NAME,
                actual: name.to_string(),
            });
        }
    }
    // If we can't read the exe (permissions, platform), conservatively
    // treat it as active — the liveness check already passed.

    PidVerification::Active
}

const DAEMON_BINARY_NAME: &str = "uniclipd";

fn is_daemon_binary_name(name: &str) -> bool {
    // Match "uniclipd", "uniclipd.exe", and cargo test binary names like
    // "uniclipd-<hash>". Also accept "uniclip" for the legacy single-binary
    // mode where the daemon ran inside the CLI binary.
    let base = name.strip_suffix(".exe").unwrap_or(name);
    base == "uniclipd"
        || base.starts_with("uniclipd-")
        || base == "uniclip"
        || base.starts_with("uniclip-")
}

#[cfg(unix)]
pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(windows)]
pub fn is_pid_alive(pid: u32) -> bool {
    // Win32-native (no `tasklist` shell-out): the old text check flashed a
    // console window from the no-console GUI host on every PID verification
    // (i.e. every GUI startup), and its bare substring match could false-
    // positive on any numeric column that happened to contain the digits.
    crate::win_process::is_pid_alive(pid)
}

#[cfg(not(any(unix, windows)))]
pub fn is_pid_alive(_pid: u32) -> bool {
    false
}

/// Best-effort read of the executable path for `pid`.
///
/// Returns `None` if the platform doesn't support it or if the process
/// is inaccessible (different user, security sandbox, etc.).
fn read_process_exe(pid: u32) -> Option<String> {
    read_process_exe_platform(pid)
}

#[cfg(target_os = "linux")]
fn read_process_exe_platform(pid: u32) -> Option<String> {
    fs::read_link(format!("/proc/{pid}/exe"))
        .ok()
        .map(|p| p.to_string_lossy().into_owned())
}

#[cfg(target_os = "macos")]
fn read_process_exe_platform(pid: u32) -> Option<String> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let ret = unsafe {
        libc::proc_pidpath(
            pid as i32,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if ret > 0 {
        buf.truncate(ret as usize);
        String::from_utf8(buf).ok()
    } else {
        None
    }
}

#[cfg(windows)]
fn read_process_exe_platform(_pid: u32) -> Option<String> {
    // Windows exe path resolution requires OpenProcess + QueryFullProcessImageNameW.
    // Conservative fallback: skip exe check, rely on liveness + started_at_ms.
    None
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn read_process_exe_platform(_pid: u32) -> Option<String> {
    None
}

// ── Port-based PID lookup (fallback when PID file is missing) ────────

/// Find the PID of a process listening on the given TCP port.
///
/// Best-effort fallback for when the daemon PID file is missing but a
/// health probe confirms something is listening on the daemon's port.
/// Uses platform-native tools (`netstat` on Windows, `lsof` on macOS/Linux).
///
/// Returns `None` if no listener is found or the platform tool is unavailable.
pub fn find_pid_by_port(port: u16) -> Option<u32> {
    find_pid_by_port_platform(port)
}

#[cfg(windows)]
fn find_pid_by_port_platform(port: u16) -> Option<u32> {
    let mut command = std::process::Command::new("netstat");
    command.args(["-ano"]);
    // The GUI-subsystem host has no console; without CREATE_NO_WINDOW this
    // fallback flashes a console window. (Numeric `netstat -ano` output is
    // locale-stable, unlike the tasklist banners — parsing it stays safe.
    // A Win32 GetExtendedTcpTable port would drop the shell-out entirely;
    // not worth the FFI surface for this rarely-hit fallback.)
    crate::win_console::configure_no_window(&mut command);
    let output = command.output().ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if !line.contains("LISTENING") {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }
        // Local address format: "127.0.0.1:PORT" or "[::1]:PORT"
        if let Some(colon_idx) = parts[1].rfind(':') {
            if parts[1][colon_idx + 1..].parse::<u16>().ok() == Some(port) {
                return parts[4].parse::<u32>().ok().filter(|&p| p != 0);
            }
        }
    }
    None
}

#[cfg(unix)]
fn find_pid_by_port_platform(port: u16) -> Option<u32> {
    let output = std::process::Command::new("lsof")
        .args(["-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.trim().lines().next()?.parse::<u32>().ok()
}

#[cfg(not(any(unix, windows)))]
fn find_pid_by_port_platform(_port: u16) -> Option<u32> {
    None
}

// Backward-compatible standalone functions for external callers.

/// Read the full daemon PID metadata (pid + mode + started_at_ms) from disk.
///
/// Falls back to `mode = Standalone, started_at_ms = 0` for legacy raw-u32
/// PID files left over from pre-Phase-C daemons. Termination call sites
/// **must** consume `metadata.mode` and refuse to SIGTERM
/// [`DaemonProcessMode::InProcess`] daemons — killing one tears down the
/// hosting GUI shell process. There is intentionally no PID-only helper:
/// callers cannot bypass the mode field.
pub fn read_pid_metadata() -> Result<Option<DaemonPidMetadata>> {
    default_manager()?.read_pid_metadata()
}

/// Write `pid + mode` to the configured daemon PID file.
///
/// `mode` records whether the daemon is running standalone (kill-able via
/// SIGTERM) or in-process inside a GUI shell (must not be killed externally).
pub fn write_current_pid_with_mode(mode: DaemonProcessMode) -> Result<u32> {
    default_manager()?.write_current_pid_with_mode(mode)
}

/// Removes the daemon PID metadata file for the current application profile.
pub fn remove_pid_file() -> Result<()> {
    default_manager()?.remove_pid_file()
}

/// Compute the filesystem path where the daemon PID metadata file for the
/// current application profile is stored.
pub fn resolve_pid_path() -> Result<PathBuf> {
    Ok(default_manager()?.pid_path().to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// Build a `DaemonPidManager` whose `pid_path()` lives inside `temp`.
    /// The manager now stores a resolved PID-file path directly, so tests
    /// construct it from a temp `<root>/.daemon-pid` instead of dragging in the
    /// app-stack `AppDirs` / `AppPaths` machinery.
    fn manager_in(temp: &TempDir) -> DaemonPidManager {
        let pid_path = temp.path().join(DAEMON_PID_FILE_NAME);
        DaemonPidManager::new(pid_path)
    }

    #[test]
    fn json_round_trip_preserves_mode() {
        let temp = TempDir::new().unwrap();
        let mgr = manager_in(&temp);

        let pid = mgr
            .write_current_pid_with_mode(DaemonProcessMode::InProcess)
            .unwrap();
        assert_eq!(pid, std::process::id());

        let metadata = mgr.read_pid_metadata().unwrap().expect("pid file written");
        assert_eq!(metadata.pid, pid);
        assert_eq!(metadata.mode, DaemonProcessMode::InProcess);
        assert!(metadata.started_at_ms > 0);
    }

    #[test]
    fn legacy_raw_u32_parses_as_standalone() {
        let temp = TempDir::new().unwrap();
        let mgr = manager_in(&temp);

        // Simulate a pre-Phase-C daemon by writing a bare decimal PID.
        let pid_path = mgr.pid_path();
        if let Some(parent) = pid_path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&pid_path, "12345").unwrap();

        let metadata = mgr.read_pid_metadata().unwrap().expect("pid file written");
        assert_eq!(metadata.pid, 12345);
        assert_eq!(
            metadata.mode,
            DaemonProcessMode::Standalone,
            "legacy PID files predate the mode field — must default to Standalone \
             so `cli stop` can still terminate them"
        );
        assert_eq!(
            metadata.started_at_ms, 0,
            "legacy PID files have no timestamp — surface that with sentinel zero"
        );
    }

    #[test]
    fn missing_file_returns_none() {
        let temp = TempDir::new().unwrap();
        let mgr = manager_in(&temp);

        assert!(mgr.read_pid_metadata().unwrap().is_none());
    }

    #[test]
    fn spawned_by_round_trips_through_json() {
        let meta =
            DaemonPidMetadata::now(42, DaemonProcessMode::Standalone, DaemonSpawnOrigin::Gui);
        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("\"spawnedBy\":\"gui\""), "got {json}");

        let parsed: DaemonPidMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.spawned_by, DaemonSpawnOrigin::Gui);
        assert!(parsed.is_gui_spawned());
    }

    #[test]
    fn json_without_spawned_by_defaults_to_unknown() {
        // PID files written before the field exists must still parse.
        let legacy = r#"{"pid":7,"mode":"standalone","startedAtMs":123}"#;
        let parsed: DaemonPidMetadata = serde_json::from_str(legacy).unwrap();
        assert_eq!(parsed.spawned_by, DaemonSpawnOrigin::Unknown);
        assert!(
            !parsed.is_gui_spawned(),
            "legacy daemons must never be treated as GUI-owned"
        );
    }

    #[test]
    fn from_env_resolves_origin() {
        // UC_DAEMON_SPAWN_ORIGIN is process-global; serialise the env mutation.
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _env = ENV_LOCK.lock().unwrap();

        std::env::set_var(SPAWN_ORIGIN_ENV, "gui");
        assert_eq!(DaemonSpawnOrigin::from_env(), DaemonSpawnOrigin::Gui);
        std::env::set_var(SPAWN_ORIGIN_ENV, "cli");
        assert_eq!(DaemonSpawnOrigin::from_env(), DaemonSpawnOrigin::Cli);
        std::env::set_var(SPAWN_ORIGIN_ENV, "bogus");
        assert_eq!(DaemonSpawnOrigin::from_env(), DaemonSpawnOrigin::Unknown);
        std::env::remove_var(SPAWN_ORIGIN_ENV);
        assert_eq!(DaemonSpawnOrigin::from_env(), DaemonSpawnOrigin::Unknown);
    }

    #[test]
    fn find_pid_by_port_finds_own_listener() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let found = find_pid_by_port(port);
        assert_eq!(
            found,
            Some(std::process::id()),
            "must find the current process as the listener on port {port}"
        );
    }

    #[test]
    fn find_pid_by_port_returns_none_for_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            find_pid_by_port(port).is_none(),
            "must return None when no process is listening on port {port}"
        );
    }
}
