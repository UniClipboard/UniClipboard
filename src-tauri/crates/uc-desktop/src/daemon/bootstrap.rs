//! daemon-lifecycle 装配 (每次 daemon start/stop 重建)。
//!
//! 进程级资源 (sqlite pool / repos / settings / secure storage / blob
//! workers / clipboard_write_coordinator / file_transfer_lifecycle 等)
//! 由 caller 通过 [`crate::bootstrap::build_process_runtime`] 一次性装好,
//! 透传 [`uc_bootstrap::WiredDependencies`] 给本模块复用。
//!
//! 这条链上**不再** 跑 `wire_dependencies` —— sqlite pool 等跨 daemon
//! reload 不会重建。

use std::sync::Arc;

use uc_application::facade::{BlobTransferFacade, ClipboardSyncFacade};
use uc_bootstrap::assembly::WiredDependencies;
use uc_bootstrap::builders::build_daemon_lifecycle;
use uc_bootstrap::SpaceSetupAssembly;

/// daemon-lifecycle 装配结果。
///
/// 方案 C 后 daemon 进程内只起一次, 这些字段也只装一次, 跟随 AppFacade
/// Arc drop 自然回收。caller 持有的进程级资源 (deps / storage_paths /
/// clipboard_write_coordinator / file_transfer_lifecycle / emitter_cell)
/// 不在这里 —— 它们走 [`crate::daemon::host::ProcessRuntimeHandles`]
/// 传入 daemon spawn。
pub struct DaemonBootstrapAssembly {
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    pub blob_transfer_facade: Arc<BlobTransferFacade>,
    pub space_setup_assembly: SpaceSetupAssembly,
    /// Mobile sync LAN endpoint adapter(具体类型旁路) — daemon LAN
    /// listener 启停时调 inherent `set` / `clear` 写它,facade 通过
    /// `AppDeps.mobile_sync.endpoint_info` 只读,两端共享同一份 Arc。
    ///
    /// 来自 caller 的 `WiredDependencies.mobile_sync_endpoint_info`,
    /// 在这里 clone 一份给 daemon main loop 使用。
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
}

/// 构造 daemon-lifecycle 装配。
///
/// async 形态 —— caller 必须在 tokio runtime 上下文中调用 ([`build_daemon_lifecycle`]
/// 内部 `Endpoint::bind` 会 spawn magicsock / relay / STUN actor)。
///
/// `wired` 由 caller 通过 [`crate::bootstrap::build_process_runtime`] 一次
/// 性装好,daemon reload 复用同一份 —— sqlite pool / repos / settings repo
/// 跨 daemon 启停**不重建**。
pub async fn build_daemon_bootstrap_assembly(
    wired: &WiredDependencies,
) -> anyhow::Result<DaemonBootstrapAssembly> {
    let lifecycle = build_daemon_lifecycle(wired).await?;
    let blob_transfer_facade = lifecycle.space_setup_assembly.blob.clone();
    Ok(DaemonBootstrapAssembly {
        clipboard_sync_facade: lifecycle.clipboard_sync_facade,
        blob_transfer_facade,
        space_setup_assembly: lifecycle.space_setup_assembly,
        mobile_sync_endpoint_info: Arc::clone(&wired.mobile_sync_endpoint_info),
    })
}
