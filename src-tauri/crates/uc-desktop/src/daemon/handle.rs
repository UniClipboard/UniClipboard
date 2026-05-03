//! In-process daemon 控制句柄。
//!
//! [`DaemonHandle`] 是 GUI 进程内启动 daemon（[`crate::daemon::start_in_process`]）的产物。
//! 它持有 daemon main loop 的 `JoinHandle` 和一个外部触发的 `CancellationToken`：
//! caller 调用 [`DaemonHandle::shutdown`] 即可优雅关闭整个 daemon 子系统并等待
//! 资源回收完成。
//!
//! 独立 daemon 进程入口（`daemon` binary 的 [`crate::daemon::run`]）不使用此句柄
//! ——它在自己的 tokio runtime 里 block 到 main loop 自然退出。

use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// In-process daemon 实例的控制句柄。
///
/// 由 [`crate::daemon::start_in_process`] 返回，由 GUI shell 持有；GUI 关闭时
/// 调用 [`DaemonHandle::shutdown`] 触发 daemon 的优雅退出（cancel cascade →
/// HTTP graceful shutdown → service stop 顺序，与独立进程模式行为一致）。
pub struct DaemonHandle {
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<()>>,
}

impl DaemonHandle {
    pub(crate) fn new(cancel: CancellationToken, join: JoinHandle<anyhow::Result<()>>) -> Self {
        Self { cancel, join }
    }

    /// 复制一份外部 shutdown 信号 token——通常用于把 daemon 的关闭信号
    /// 转接给 GUI 内其他需要"daemon 是否仍在跑"语义的子系统。
    pub fn cancel_signal(&self) -> CancellationToken {
        self.cancel.clone()
    }

    /// 触发 daemon 关闭并等待 main loop 完成（含资源清理）。
    ///
    /// 如果在 `timeout` 内 main loop 没有自行退出，返回超时错误；此时 daemon
    /// 内部的资源（HTTP server、worker tasks、PID 文件）可能处于不确定状态。
    pub async fn shutdown(self, timeout: Duration) -> anyhow::Result<()> {
        self.cancel.cancel();
        match tokio::time::timeout(timeout, self.join).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_err)) => Err(anyhow::anyhow!("daemon task panicked: {join_err}")),
            Err(_) => Err(anyhow::anyhow!(
                "daemon shutdown timed out after {timeout:?}"
            )),
        }
    }

    /// 等 daemon main loop 自行退出（异常崩溃 / 内部 cancel cascade）。
    ///
    /// 不主动触发 shutdown——只 await。`run()` 这种"独立 daemon binary"
    /// 入口在 block_on 里调用此方法，由内部的信号 listener 来触发 cancel。
    pub async fn wait(self) -> anyhow::Result<()> {
        self.join
            .await
            .map_err(|e| anyhow::anyhow!("daemon task panicked: {e}"))?
    }
}
