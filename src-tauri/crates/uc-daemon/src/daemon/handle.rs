//! In-process daemon 控制句柄。
//!
//! [`DaemonHandle`] 是 GUI 进程内启动 daemon（uc-desktop 的 `start_in_process`）的产物。
//! 它持有 daemon main loop 的 `JoinHandle` 和一个外部触发的 `CancellationToken`：
//! caller 调用 [`DaemonHandle::shutdown`] 即可优雅关闭整个 daemon 子系统并等待
//! 资源回收完成。
//!
//! 独立 daemon 进程入口（uc-desktop 的 `run`）不使用此句柄
//! ——它在自己的 tokio runtime 里 block 到 main loop 自然退出。

use std::time::Duration;

use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// In-process daemon 实例的控制句柄。
///
/// 由 uc-desktop 的 `start_in_process` 返回，由 GUI shell 持有；GUI 关闭时
/// 调用 [`DaemonHandle::shutdown`] 触发 daemon 的优雅退出（cancel cascade →
/// HTTP graceful shutdown → service stop 顺序，与独立进程模式行为一致）。
pub struct DaemonHandle {
    cancel: CancellationToken,
    join: JoinHandle<anyhow::Result<()>>,
}

impl DaemonHandle {
    pub fn new(cancel: CancellationToken, join: JoinHandle<anyhow::Result<()>>) -> Self {
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
        // `&mut JoinHandle` 是 cancel-safe 的，timeout 不会消耗它——超时
        // 分支可以直接调 abort + 再 await 回收，避免 drop JoinHandle 把
        // daemon task detach 到后台继续跑。
        let mut join = self.join;
        match tokio::time::timeout(timeout, &mut join).await {
            Ok(Ok(result)) => result,
            Ok(Err(join_err)) => Err(anyhow::anyhow!("daemon task panicked: {join_err}")),
            Err(_) => {
                join.abort();
                let _ = join.await;
                Err(anyhow::anyhow!(
                    "daemon shutdown timed out after {timeout:?}"
                ))
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Build a handle whose backing task waits on the cancel token —
    /// mirrors how a real daemon's main loop uses the external cancel.
    fn handle_responding_to_cancel() -> DaemonHandle {
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let join = tokio::spawn(async move {
            cancel_for_task.cancelled().await;
            Ok(())
        });
        DaemonHandle::new(cancel, join)
    }

    #[tokio::test]
    async fn cancel_signal_returns_a_token_linked_to_handle() {
        let handle = handle_responding_to_cancel();
        let signal = handle.cancel_signal();
        assert!(
            !signal.is_cancelled(),
            "fresh handle's cancel_signal must start uncancelled"
        );

        // shutdown drives the underlying token, which the cloned signal
        // observes — proves cancel_signal isn't a disconnected new token.
        let signal_for_observer = signal.clone();
        let observer = tokio::spawn(async move { signal_for_observer.cancelled().await });

        handle
            .shutdown(Duration::from_secs(1))
            .await
            .expect("dummy task exits Ok on cancel");

        observer
            .await
            .expect("observer task should complete after cancellation propagates");
        assert!(
            signal.is_cancelled(),
            "shutdown must propagate to all clones of cancel_signal"
        );
    }

    #[tokio::test]
    async fn shutdown_succeeds_when_task_exits_cleanly_within_timeout() {
        let handle = handle_responding_to_cancel();
        handle
            .shutdown(Duration::from_secs(2))
            .await
            .expect("clean shutdown");
    }

    #[tokio::test]
    async fn shutdown_propagates_inner_error_from_daemon_task() {
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let join = tokio::spawn(async move {
            cancel_for_task.cancelled().await;
            Err(anyhow::anyhow!("daemon main loop blew up"))
        });
        let handle = DaemonHandle::new(cancel, join);

        let err = handle
            .shutdown(Duration::from_secs(1))
            .await
            .expect_err("daemon task error must surface, not be swallowed");
        assert!(
            err.to_string().contains("daemon main loop blew up"),
            "expected wrapped daemon error, got: {err}"
        );
    }

    #[tokio::test]
    async fn shutdown_reports_panic_in_daemon_task() {
        let cancel = CancellationToken::new();
        let join: tokio::task::JoinHandle<anyhow::Result<()>> =
            tokio::spawn(async { panic!("boom") });
        let handle = DaemonHandle::new(cancel, join);

        let err = handle
            .shutdown(Duration::from_secs(1))
            .await
            .expect_err("a panicked task must yield a JoinError-derived anyhow error");
        assert!(
            err.to_string().contains("panicked"),
            "panic must be classified as a panic, got: {err}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn shutdown_times_out_when_task_ignores_cancel() {
        // start_paused so the timeout doesn't actually wait wallclock-wise.
        let cancel = CancellationToken::new();
        // Task that never observes cancel — simulates a wedged main loop.
        let join: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async {
            // Sleep way past the timeout the test sets.
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok(())
        });
        let handle = DaemonHandle::new(cancel, join);

        let err = handle
            .shutdown(Duration::from_millis(50))
            .await
            .expect_err("wedged daemon must surface timeout error");
        assert!(
            err.to_string().contains("timed out"),
            "timeout error must mention timeout, got: {err}"
        );
    }

    #[tokio::test]
    async fn wait_does_not_trigger_cancel_and_returns_when_task_finishes() {
        // wait() is the standalone-binary entry: caller's signal handler
        // pulls cancel; wait() never touches cancel itself.
        let cancel = CancellationToken::new();
        let cancel_for_task = cancel.clone();
        let join = tokio::spawn(async move {
            cancel_for_task.cancelled().await;
            Ok(())
        });
        let handle = DaemonHandle::new(cancel.clone(), join);

        // Trigger cancel from the outside (simulating signal handler), then
        // wait — wait must observe the task exit without itself triggering cancel.
        let cancel_for_external = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(10)).await;
            cancel_for_external.cancel();
        });

        handle
            .wait()
            .await
            .expect("wait must surface task's Ok(())");
    }
}
