//! File sync orchestrator worker for the daemon.
//!
//! Slice4 P5c: 旧的 `FileTransferEventInboundPort` 已退役,事件循环本身
//! 失去了上游(libp2p adapter 删除后,iroh 侧通过 `FileTransferEventPublisherPort`
//! 直接写 store/lifecycle,不再经此 worker)。本 worker 现在只承担两件事:
//!
//! 1. 启动期 reconcile —— 把进程崩溃留下的 in-flight transfer 标记为 failed
//! 2. 周期性 sweep —— 把超时的 pending/transferring 状态收口
//!
//! Slice4 P5c C8a: 历史的 `handle_event` / `handle_completed` /
//! `restore_file_to_clipboard_after_transfer` / `fail_transfer` 死链路已物理
//! 删除,iroh 侧若再需要这些行为应改走 `FileTransferEventPublisherPort` +
//! `FileTransferLifecycle` 的应用层 use case,不再在本 worker 内复活。

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::info;

use uc_application::facade::BlobTransferFacade;
use uc_bootstrap::FileTransferLifecycle;

use crate::daemon::service::{DaemonService, ServiceHealth};

pub struct FileSyncOrchestratorWorker {
    lifecycle: Arc<FileTransferLifecycle>,
    blob_transfer: Arc<BlobTransferFacade>,
}

impl FileSyncOrchestratorWorker {
    pub fn new(
        lifecycle: Arc<FileTransferLifecycle>,
        blob_transfer: Arc<BlobTransferFacade>,
    ) -> Self {
        Self {
            lifecycle,
            blob_transfer,
        }
    }
}

#[async_trait]
impl DaemonService for FileSyncOrchestratorWorker {
    fn name(&self) -> &str {
        "file-sync-orchestrator"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("file sync orchestrator starting");

        // 1. Run startup reconciliation (orphaned in-flight transfers → failed)
        self.lifecycle.reconcile_on_startup().await;

        // 2. Start timeout sweep (15s interval, cancellable via watch channel)
        let (sweep_cancel_tx, sweep_cancel_rx) = tokio::sync::watch::channel(false);
        let _sweep_handle = self
            .lifecycle
            .spawn_timeout_sweep(sweep_cancel_rx, self.blob_transfer.clone());

        // 3. 等取消 —— 旧的 inbound 事件循环已下线,iroh 侧改走
        //    `FileTransferEventPublisherPort` 直接写 store/lifecycle。
        cancel.cancelled().await;
        let _ = sweep_cancel_tx.send(true);
        info!("file sync orchestrator cancelled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!("file sync orchestrator stopped");
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
