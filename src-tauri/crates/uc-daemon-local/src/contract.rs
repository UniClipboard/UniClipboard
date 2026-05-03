//! GUI-framework agnostic contract types for daemon process coordination.
//!
//! 这些类型不依赖 `tauri-plugin-shell::CommandChild`，可以被
//! `uc-desktop` / 其它 shell 在**不启用** `sidecar-lifecycle` feature
//! 的情况下直接消费。绑 `CommandChild` 的状态（`OwnedDaemonChild`、
//! `GuiOwnedDaemonState`）以及拉起编排（`bootstrap_daemon_connection_with_hooks`）
//! 仍然在 [`crate::daemon_lifecycle`] / [`crate::daemon_bootstrap`] 后面
//! 的 feature gate 里。

use std::process::Command;

use thiserror::Error;
use uc_daemon_contract::api::types::HealthResponse;

/// 一次健康探测的分类结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProbeOutcome {
    Absent,
    Compatible(HealthResponse),
    Incompatible {
        details: String,
        observed_package_version: Option<String>,
        observed_api_revision: Option<String>,
    },
}

/// 桌面侧 daemon 拉起 / 监督流程中可能产生的错误。
#[derive(Debug, Error)]
pub enum DaemonBootstrapError {
    #[error("failed to initialize daemon HTTP probe client: {0}")]
    Client(anyhow::Error),
    #[error("failed to probe daemon health: {0}")]
    Probe(anyhow::Error),
    #[error("incompatible daemon is already running: {details}")]
    IncompatibleDaemon { details: String },
    #[error("failed to spawn uniclipboard-daemon: {0}")]
    Spawn(anyhow::Error),
    #[error("daemon startup timed out after {timeout_ms}ms")]
    StartupTimeout { timeout_ms: u64 },
    #[error("failed to load daemon connection info: {0}")]
    ConnectionInfo(anyhow::Error),
}

/// 当前 daemon child 是因为什么场景被 spawn 的——监督循环用它判断是否需要
/// 重连等附加动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnReason {
    Absent,
    Replacement,
}

/// `terminate_local_daemon_pid` 的返回错误，仅承载一个 detail string。
#[derive(Debug)]
pub struct TerminateDaemonError(pub String);

impl std::fmt::Display for TerminateDaemonError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for TerminateDaemonError {}

/// 通过平台原生命令向指定 PID 发送 SIGTERM（或 Windows 上 taskkill /F）。
///
/// 使用 `std::process::Command`——不依赖任何 GUI 框架或 sidecar 体系，
/// 可以被 daemon 二进制、GUI shell、CLI 工具任意一方消费。
pub fn terminate_local_daemon_pid(pid: u32) -> Result<(), TerminateDaemonError> {
    #[cfg(unix)]
    let mut command = {
        let mut command = Command::new("kill");
        command.arg("-TERM").arg(pid.to_string());
        command
    };

    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("taskkill");
        command.arg("/PID").arg(pid.to_string()).arg("/T").arg("/F");
        command
    };

    let output = command
        .output()
        .map_err(|e| TerminateDaemonError(format!("failed to launch terminator: {e}")))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(TerminateDaemonError(format!(
        "failed to terminate pid {pid}: status={} stdout={} stderr={}",
        output.status,
        stdout.trim(),
        stderr.trim()
    )))
}
