//! daemon Tokio runtime 创建。

use tokio::runtime::Runtime;

/// 创建 daemon 进程使用的长生命周期 Tokio runtime。
///
/// daemon 生命周期里的异步任务必须共享同一个 runtime。iroh endpoint
/// 绑定时会启动 magicsock、relay、STUN 等后台任务；如果这些任务跑在短
/// 生命周期 runtime 上，后续连接会变成不可用状态。
pub fn build_daemon_tokio_runtime() -> anyhow::Result<Runtime> {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(Into::into)
}
