//! Daemon-lifecycle assembly rebuilt for each daemon session.
//!
//! Process-level resources are prepared once by the daemon process runtime and
//! passed here as [`uc_bootstrap::WiredDependencies`]. This path never runs
//! `wire_dependencies`, so persistent resources survive session restarts.

use std::sync::Arc;

use uc_application::facade::{BlobTransferFacade, ClipboardSyncFacade};
use uc_bootstrap::{build_daemon_lifecycle, SyncEngineAssembly, WiredDependencies};

/// daemon-lifecycle 装配结果。
///
/// Session-owned resources that are released when the runtime stops.
/// Process-level resources remain owned by `DaemonProcessRuntime`.
pub struct DaemonBootstrapAssembly {
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    pub blob_transfer_facade: Arc<BlobTransferFacade>,
    pub sync_engine_assembly: SyncEngineAssembly,
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
/// `wired` 由 caller 通过 uc-desktop 的 `build_process_runtime` 一次
/// 性装好,daemon reload 复用同一份 —— sqlite pool / repos / settings repo
/// 跨 daemon 启停**不重建**。
pub async fn build_daemon_bootstrap_assembly(
    wired: &WiredDependencies,
) -> anyhow::Result<DaemonBootstrapAssembly> {
    let lifecycle = build_daemon_lifecycle(&wired.deps, &wired.sync_engine, &wired.shared).await?;
    let blob_transfer_facade = lifecycle.sync_engine_assembly.blob.clone();
    Ok(DaemonBootstrapAssembly {
        clipboard_sync_facade: lifecycle.clipboard_sync_facade,
        blob_transfer_facade,
        sync_engine_assembly: lifecycle.sync_engine_assembly,
        mobile_sync_endpoint_info: Arc::clone(&wired.daemon_runtime.mobile_sync_endpoint_info),
    })
}
