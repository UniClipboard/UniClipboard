//! daemon bootstrap 装配。

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::facade::{BlobTransferFacade, ClipboardSyncFacade, HostEventEmitterPort};
use uc_bootstrap::builders::build_daemon_app;
use uc_bootstrap::file_transfer_lifecycle::FileTransferLifecycle;
use uc_bootstrap::{
    build_non_gui_bundle, BackgroundRuntimeDeps, BlobProcessingPorts, NonGuiBundle,
    SpaceSetupAssembly,
};

/// daemon bootstrap 装配结果。
pub struct DaemonBootstrapAssembly {
    pub non_gui_bundle: NonGuiBundle,
    pub background: BackgroundRuntimeDeps,
    pub blob_ports: BlobProcessingPorts,
    pub file_cache_dir: PathBuf,
    pub file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    pub blob_transfer_facade: Arc<BlobTransferFacade>,
    pub space_setup_assembly: SpaceSetupAssembly,
    /// Mobile sync LAN endpoint adapter(具体类型旁路) — daemon LAN listener
    /// 启停时调 inherent `set` / `clear` 写它,facade 通过
    /// `AppDeps.mobile_sync.endpoint_info` 只读,两端共享同一份 Arc。
    pub mobile_sync_endpoint_info:
        Arc<uc_infra::mobile_sync::InMemoryMobileSyncEndpointInfoAdapter>,
}

/// 构造 daemon bootstrap 所需句柄。
///
/// async 形态——caller 必须在 tokio runtime 上下文中调用。独立 daemon binary
/// 入口（[`crate::daemon::run`]）通过 `Runtime::block_on` 进入 async 上下文；
/// in-process 入口（[`crate::daemon::start_in_process`]）由 GUI 提供的 runtime
/// 直接 await。
pub async fn build_daemon_bootstrap_assembly() -> anyhow::Result<DaemonBootstrapAssembly> {
    let ctx = build_daemon_app().await?;

    let file_cache_dir = ctx.storage_paths.file_cache_dir.clone();
    let file_transfer_lifecycle = ctx.background.file_transfer_lifecycle.clone();
    let clipboard_write_coordinator = ctx.background.clipboard_write_coordinator.clone();
    let emitter_cell = ctx.emitter_cell.clone();
    let blob_ports = BlobProcessingPorts::from_app_deps(&ctx.deps);
    let background = ctx.background;
    let clipboard_sync_facade = ctx.clipboard_sync_facade.clone();
    let blob_transfer_facade = ctx.space_setup_assembly.blob.clone();
    let mobile_sync_endpoint_info = ctx.mobile_sync_endpoint_info.clone();

    let non_gui_bundle = build_non_gui_bundle(
        ctx.deps,
        ctx.storage_paths.clone(),
        Arc::clone(&emitter_cell),
    )?;

    Ok(DaemonBootstrapAssembly {
        non_gui_bundle,
        background,
        blob_ports,
        file_cache_dir,
        file_transfer_lifecycle,
        clipboard_write_coordinator,
        emitter_cell,
        clipboard_sync_facade,
        blob_transfer_facade,
        space_setup_assembly: ctx.space_setup_assembly,
        mobile_sync_endpoint_info,
    })
}
