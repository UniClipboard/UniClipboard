//! 桌面 GUI 进程的后台任务调度（GUI-framework agnostic）。
//!
//! 这里只关心任务的"注册与生命周期协调"——任务的业务定义留在
//! `uc-application` 的 facade。每个 starter 都是 `async fn`：它本身不
//! `spawn` 任何东西，只是 `await` `TaskRegistry::spawn(...)` 把任务注册
//! 进 registry。**进入 async 上下文的方式由 shell 决定**——Tauri shell 用
//! `tauri::async_runtime::spawn`（Tauri 持有的全局 tokio runtime），未来
//! native shell 用自己的 tokio handle。这样本模块完全不需要触发
//! `tokio::spawn`，从而不依赖"调用线程必须已经处于 tokio runtime 上下文"
//! 这个隐式假设——这正是 Tauri 的 `setup` 闭包不满足的假设。
//!
//! 这种"async fn + caller 决定 spawn"的形态与
//! [`uc_bootstrap::spawn_blob_processing_tasks`] 一致。

use std::sync::Arc;

use tracing::{info, warn};

use uc_application::facade::ClipboardHistoryFacade;
use uc_bootstrap::TaskRegistry;

/// Register the file cache cleanup task with `TaskRegistry`.
///
/// Cleanup goes through `ClipboardHistoryFacade::cleanup_expired_files`,
/// which routes every expired file through the entry-aware delete path
/// (untag iroh-blobs reference + remove cache file + drop sqlite rows
/// in one shot). Pre-Phase-C this called a standalone use case that
/// `tokio::fs::remove_file`-d cache files directly, leaving iroh-blobs
/// metadata pointing at vanished files (the precursor to the Poisoned
/// BaoFileStorage panic at `bao_file.rs:410`).
///
/// Caller must drive this future inside a tokio runtime context (e.g.
/// `tauri::async_runtime::spawn(async move { start_file_cache_cleanup(...).await })`).
pub async fn start_file_cache_cleanup(
    history_facade: Arc<ClipboardHistoryFacade>,
    task_registry: &Arc<TaskRegistry>,
) {
    task_registry
        .spawn("file_cache_cleanup", |_token| async move {
            match history_facade.cleanup_expired_files().await {
                Ok(result) => {
                    if result.files_removed > 0 {
                        info!(
                            files_removed = result.files_removed,
                            entries_deleted = result.entries_deleted,
                            orphans_removed = result.orphans_removed,
                            bytes_reclaimed = result.bytes_reclaimed,
                            errors = result.errors,
                            "Startup file cache cleanup completed"
                        );
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Startup file cache cleanup failed (non-fatal)");
                }
            }
        })
        .await;
    info!("All background tasks registered with TaskRegistry");
}
