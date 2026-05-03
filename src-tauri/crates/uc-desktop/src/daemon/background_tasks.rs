//! daemon 后台任务启动。

use std::sync::Arc;

use uc_bootstrap::{BackgroundRuntimeDeps, BlobProcessingPorts, TaskRegistry};

/// 启动 daemon 需要的后台 blob 处理任务。
///
/// caller 必须在 tokio runtime 上下文中调用——这里直接 `tokio::spawn`，
/// 而不是绑定到某个具体的 `Runtime` 实例（GUI shell 与 daemon binary
/// 用的不是同一个 runtime）。
pub fn spawn_daemon_background_tasks(
    background: BackgroundRuntimeDeps,
    blob_ports: BlobProcessingPorts,
    task_registry: Arc<TaskRegistry>,
) {
    tokio::spawn(async move {
        uc_bootstrap::spawn_blob_processing_tasks(background, blob_ports, &task_registry).await;
    });
}
