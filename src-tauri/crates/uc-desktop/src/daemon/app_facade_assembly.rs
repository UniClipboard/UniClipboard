//! daemon AppFacade 装配。

use std::sync::Arc;

use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::deps::AppDeps;
use uc_application::facade::{
    AppFacade, AppPaths, BlobTransferFacade, ClipboardSyncFacade, LifecycleStatusGateway,
    SearchCoordinator,
};
use uc_application::ApplyInboundClipboardUseCase;
use uc_bootstrap::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly,
    SpaceSetupAssembly,
};
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ports::blob::BlobTransferPort;

/// daemon AppFacade 装配结果。
pub struct DaemonAppFacadeAssembly {
    pub app_facade: Arc<AppFacade>,
    pub local_device_id: String,
}

/// daemon AppFacade 装配输入。
pub struct DaemonAppFacadeAssemblyInput<'a> {
    pub deps: &'a AppDeps,
    pub storage_paths: &'a AppPaths,
    pub lifecycle_status: Arc<dyn LifecycleStatusGateway>,
    pub space_setup_assembly: &'a SpaceSetupAssembly,
    pub clipboard_sync: Arc<ClipboardSyncFacade>,
    pub blob_transfer: Arc<BlobTransferFacade>,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub clipboard_integration_mode: ClipboardIntegrationMode,
    pub search_coordinator: Arc<SearchCoordinator>,
    /// daemon worker 装配过程中已构造好的 `ApplyInboundClipboardUseCase`
    /// (带 blob materializer + host event emitter)。同一份实例在 mobile
    /// sync facade 装配时也喂进去 —— 让 LAN 入站 PUT 路径与 P2P 入站走
    /// 同一条 ApplyInbound 链(host event 单一源 / blob 状态共享)。
    pub mobile_sync_apply_inbound: Arc<ApplyInboundClipboardUseCase>,
}

/// 构造 daemon 对外统一业务入口。
pub fn build_daemon_app_facade(input: DaemonAppFacadeAssemblyInput<'_>) -> DaemonAppFacadeAssembly {
    let blob_transfer_port: Arc<dyn BlobTransferPort> =
        Arc::clone(&input.space_setup_assembly.blob_transfer);
    let app_facade = build_app_facade_from_deps(
        input.deps,
        input.storage_paths,
        input.lifecycle_status,
        AppFacadeAssemblyOptions {
            space_setup: Some(input.space_setup_assembly.facade.clone()),
            member_roster: Some(input.space_setup_assembly.roster.clone()),
            clipboard_sync: Some(input.clipboard_sync),
            blob_transfer: Some(input.blob_transfer),
            blob_transfer_port: Some(blob_transfer_port),
            clipboard_restore: Some(ClipboardRestoreAssembly {
                write_coordinator: input.clipboard_write_coordinator,
                integration_mode: input.clipboard_integration_mode,
            }),
            search_coordinator: Some(input.search_coordinator),
            mobile_sync_apply_inbound: Some(input.mobile_sync_apply_inbound),
        },
    );

    DaemonAppFacadeAssembly {
        app_facade,
        local_device_id: input
            .deps
            .device
            .device_identity
            .current_device_id()
            .to_string(),
    }
}
