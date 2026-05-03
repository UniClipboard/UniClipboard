//! GUI-framework agnostic 健康轮询 helpers。
//!
//! 这两个函数只接收一个 `Probe` 闭包返回 `ProbeOutcome`——不绑定
//! `CommandChild`，所以可以放在默认编译路径里给 `uc-desktop` / 其它
//! shell（不启用 `sidecar-lifecycle` feature）直接复用。

use std::future::Future;
use std::time::Duration;

use crate::contract::{DaemonBootstrapError, ProbeOutcome};

/// 轮询 daemon 健康端点，直到 daemon 报告兼容、或超时、或观测到不兼容。
///
/// - `Compatible` → 返回 `Ok(())`
/// - `Incompatible` → 返回 `DaemonBootstrapError::IncompatibleDaemon`
/// - 超时 → 返回 `DaemonBootstrapError::StartupTimeout`
pub async fn wait_for_daemon_health<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        match probe().await? {
            ProbeOutcome::Compatible(_) => return Ok(()),
            ProbeOutcome::Absent => {}
            ProbeOutcome::Incompatible { details, .. } => {
                return Err(DaemonBootstrapError::IncompatibleDaemon { details });
            }
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::StartupTimeout {
                timeout_ms: timeout.as_millis() as u64,
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}

/// 轮询 daemon 健康端点直到观察到 `Absent`（端点消失），或在 `timeout`
/// 内仍能看到 daemon 时返回 `IncompatibleDaemon`。用于不兼容 daemon
/// 替换流程中等待旧进程退出。
pub async fn wait_for_endpoint_absent<Probe, ProbeFuture>(
    probe: &mut Probe,
    timeout: Duration,
    poll_interval: Duration,
    last_reason: &str,
) -> Result<(), DaemonBootstrapError>
where
    Probe: FnMut() -> ProbeFuture,
    ProbeFuture: Future<Output = Result<ProbeOutcome, DaemonBootstrapError>>,
{
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match probe().await? {
            ProbeOutcome::Absent => return Ok(()),
            ProbeOutcome::Compatible(_) | ProbeOutcome::Incompatible { .. } => {}
        }

        if tokio::time::Instant::now() >= deadline {
            return Err(DaemonBootstrapError::IncompatibleDaemon {
                details: format!(
                    "incompatible daemon did not exit within {}ms after replacement attempt: {}",
                    timeout.as_millis(),
                    last_reason
                ),
            });
        }

        tokio::time::sleep(poll_interval).await;
    }
}
