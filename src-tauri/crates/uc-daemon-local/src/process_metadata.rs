use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use uc_application::facade::AppPaths;
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

/// 描述 daemon 进程是怎么被拉起的——决定它能不能由 `cli stop` SIGTERM 掉。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DaemonProcessMode {
    /// 独立 daemon 进程：`cli start` 拉起的 detached 子进程，或者用户在
    /// 终端直接 `uniclipboard-daemon`。`cli stop` 可以安全地 SIGTERM 它。
    Standalone,
    /// in-process daemon：跑在 GUI shell 自己的进程里（`uc-tauri` 等
    /// 通过 [`DaemonRunMode::GuiInProcess`] 启动）。`cli stop` **必须拒绝**
    /// 对它发 SIGTERM——会把整个 GUI 一起带挂；正确的关闭方式是用户去
    /// 关闭 GUI。
    ///
    /// [`DaemonRunMode::GuiInProcess`]: ../uc_desktop/daemon/run_mode/enum.DaemonRunMode.html#variant.GuiInProcess
    InProcess,
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
}

impl DaemonPidMetadata {
    /// 构造一份"现在"的元数据：`pid` + `mode` + 当前时间戳。
    pub fn now(pid: u32, mode: DaemonProcessMode) -> Self {
        let started_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        Self {
            pid,
            mode,
            started_at_ms,
        }
    }
}

/// Provides the process-wide singleton `DaemonPidManager` used by standalone helpers.
fn default_manager() -> Result<&'static DaemonPidManager> {
    static DEFAULT_MANAGER: OnceLock<Result<DaemonPidManager, String>> = OnceLock::new();
    DEFAULT_MANAGER
        .get_or_init(|| {
            let adapter = DirsAppDirsAdapter::new();
            adapter
                .get_app_dirs()
                .context("failed to resolve application directories")
                .map(|app_dirs| DaemonPidManager::new(AppPaths::from_app_dirs(&app_dirs)))
                .map_err(|e| format!("{e:#}"))
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Manages the daemon PID metadata file lifecycle.
#[derive(Debug, Clone)]
pub struct DaemonPidManager {
    app_paths: AppPaths,
}

impl DaemonPidManager {
    /// Creates a new DaemonPidManager from the provided `AppPaths`.
    pub fn new(app_paths: AppPaths) -> Self {
        Self { app_paths }
    }

    /// Returns the filesystem path where the daemon PID file for the current app/profile is stored.
    fn pid_path(&self) -> PathBuf {
        self.app_paths.daemon_pid_path()
    }

    /// 写入当前进程的 PID + `mode` 到 PID 文件（JSON 格式）。
    ///
    /// 取代历史上的 `write_current_pid`——把进程模式（`standalone` /
    /// `in_process`）一并落盘，让 `cli stop` 能区分能不能 SIGTERM。
    pub fn write_current_pid_with_mode(&self, mode: DaemonProcessMode) -> Result<u32> {
        let pid_path = self.pid_path();
        let pid = std::process::id();
        let metadata = DaemonPidMetadata::now(pid, mode);
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
fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as libc::pid_t, 0) == 0 }
}

#[cfg(windows)]
fn is_pid_alive(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
        .unwrap_or(false)
}

#[cfg(not(any(unix, windows)))]
fn is_pid_alive(_pid: u32) -> bool {
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
    /// `AppPaths` has public fields, so we don't need to drag in `uc-core`'s
    /// `AppDirs` machinery just for unit tests.
    fn manager_in(temp: &TempDir) -> DaemonPidManager {
        let root = temp.path().to_path_buf();
        let app_paths = AppPaths {
            db_path: root.join("db.sqlite"),
            vault_dir: root.join("vault"),
            settings_path: root.join("settings.json"),
            logs_dir: root.join("logs"),
            cache_dir: root.join("cache"),
            file_cache_dir: root.join("file-cache"),
            spool_dir: root.join("spool"),
            app_data_root_dir: root,
        };
        DaemonPidManager::new(app_paths)
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
}
