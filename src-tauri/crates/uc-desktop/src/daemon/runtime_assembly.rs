//! daemon 运行时 worker 装配。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::broadcast;
use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::deps::AppDeps;
use uc_application::facade::{
    BlobTransferFacade, ClipboardCaptureFacade, ClipboardLiveIndexDeps, ClipboardLiveIndexFacade,
    ClipboardLiveIndexer, ClipboardOutboundDeps, ClipboardOutboundFacade, ClipboardSyncFacade,
    HostEventBus, InboundClipboardFacade,
};
use uc_application::{
    ApplyInboundClipboardUseCase, FileCacheBlobMaterializer, InboundCapture as ApplyInboundCapture,
    InboundWrite as ApplyInboundWrite,
};
use uc_bootstrap::file_transfer_lifecycle::FileTransferLifecycle;
use uc_core::ports::SystemClipboardPort;
use uc_core::ports::{ClipboardEventRepositoryPort, EntryDeliveryRepositoryPort};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_platform::clipboard::LocalClipboard;
use uc_webserver::api::types::DaemonWsEvent;

use crate::daemon::workers::clipboard_watcher::{
    ClipboardWatcherWorker, DaemonClipboardChangeHandler,
};
use crate::daemon::workers::file_sync_orchestrator::FileSyncOrchestratorWorker;
use crate::daemon::workers::inbound_clipboard_sync::InboundClipboardSyncWorker;

/// daemon worker 装配所需输入。
pub struct DaemonRuntimeAssemblyInput<'a> {
    pub deps: &'a AppDeps,
    pub event_tx: broadcast::Sender<DaemonWsEvent>,
    pub clipboard_capture_gate: Arc<AtomicBool>,
    pub clipboard_sync_facade: Arc<ClipboardSyncFacade>,
    pub blob_transfer_facade: Arc<BlobTransferFacade>,
    pub file_cache_dir: PathBuf,
    pub file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    /// 共享的 host event bus —— 与 `BlobTransferFacade` 同源。ApplyInbound
    /// 用它在 fetch 之前发 `IncomingPending`,让前端立即出现占位卡片。
    pub host_event_bus: Arc<HostEventBus>,
    /// `EntryDeliveryRecord` 读写仓储:dispatch fan-out 写、resend 派生差集
    /// 时读、视图层读。来自 `WiredDependencies`(uc-application 是消费者,
    /// AppDeps 之外的旁路)。
    pub entry_delivery_repo: Arc<dyn EntryDeliveryRepositoryPort>,
    /// `ClipboardEventRepositoryPort` 的读端口实例(与 AppDeps 里写端口
    /// 共享底层 Diesel impl);resend 用它反查"entry 来源设备是否本机"。
    pub clipboard_event_reader_repo: Arc<dyn ClipboardEventRepositoryPort>,
    /// 信任 peer 集合:resend 派生目标 + 校验 filter 时使用。
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
}

/// daemon 启动前已构造好的后台 worker + 共享 use case。
///
/// `apply_inbound` 不是 worker, 而是 worker 装配过程的 enhanced
/// `ApplyInboundClipboardUseCase` 实例 (带 blob materializer + host event
/// emitter)。同一份实例还要通过 `build_daemon_app_facade` 喂给 mobile
/// sync facade,所以放进本结构以便 host.rs 共享。P5a.6 引入。
///
/// `clipboard_outbound` 同样不是 worker, 而是 worker 装配过程构造的
/// `ClipboardOutboundFacade` 实例 ——
/// `ClipboardWatcherWorker` 用它把本机捕获 fan-out 给 paired peers;
/// `MobileSyncFacade` 也共享这一份, 让"手机上传 → 本机入站 → 同一条
/// 出站管线 fan-out 给其他桌面"成立, 文件类型的 blob 发布逻辑只此一处,
/// 不重复实现。
pub struct DaemonRuntimeWorkers {
    pub clipboard_watcher: Arc<ClipboardWatcherWorker>,
    pub inbound_clipboard_sync: Arc<InboundClipboardSyncWorker>,
    pub file_sync_orchestrator: Arc<FileSyncOrchestratorWorker>,
    pub apply_inbound: Arc<ApplyInboundClipboardUseCase>,
    pub clipboard_outbound: Arc<ClipboardOutboundFacade>,
}

/// 构造 daemon runtime worker。
///
/// 这里只做桌面宿主侧的 worker 装配；具体业务动作仍由
/// `uc-application` facade/usecase 处理。
pub fn build_daemon_runtime_workers(
    input: DaemonRuntimeAssemblyInput<'_>,
) -> anyhow::Result<DaemonRuntimeWorkers> {
    let local_clipboard: Arc<dyn SystemClipboardPort> = Arc::new(
        LocalClipboard::new()
            .map_err(|e| anyhow::anyhow!("failed to create LocalClipboard: {}", e))?,
    );

    let clipboard_change_origin = input.deps.clipboard.clipboard_change_origin.clone();

    let apply_inbound_capture_uc = Arc::new(CaptureClipboardUseCase::new(
        input.deps.clipboard.clipboard_entry_repo.clone(),
        input.deps.clipboard.clipboard_event_repo.clone(),
        input.deps.clipboard.representation_policy.clone(),
        input.deps.clipboard.representation_normalizer.clone(),
        input.deps.device.device_identity.clone(),
        input.deps.clipboard.representation_cache.clone(),
        input.deps.clipboard.spool_queue.clone(),
        input.deps.storage.blob_writer.clone(),
        input.deps.analytics.clone(),
    ));
    let blob_materializer = Arc::new(FileCacheBlobMaterializer::new(
        input.blob_transfer_facade.clone(),
        input.file_cache_dir,
    ));
    let apply_inbound_uc = Arc::new(
        ApplyInboundClipboardUseCase::new(
            input.deps.clipboard.clipboard_entry_repo.clone(),
            Arc::clone(&apply_inbound_capture_uc) as Arc<dyn ApplyInboundCapture>,
            Arc::clone(&input.clipboard_write_coordinator) as Arc<dyn ApplyInboundWrite>,
        )
        .with_blob_materializer(blob_materializer)
        .with_host_event_emitter(input.host_event_bus),
    );
    let inbound_clipboard_facade = Arc::new(InboundClipboardFacade::new(apply_inbound_uc.clone()));
    let clipboard_capture_facade = Arc::new(ClipboardCaptureFacade::new(apply_inbound_capture_uc));
    let clipboard_live_index_facade = Arc::new(ClipboardLiveIndexFacade::new(Arc::new(
        ClipboardLiveIndexer::new(ClipboardLiveIndexDeps {
            clipboard_entry_repo: input.deps.clipboard.clipboard_entry_repo.clone(),
            representation_policy: input.deps.clipboard.representation_policy.clone(),
            search_key_derivation: input.deps.search.search_key_derivation.clone(),
            search_pipeline: input.deps.search.search_pipeline.clone(),
            search_index: input.deps.search.search_index.clone(),
        }),
    )));
    let clipboard_outbound_facade = Arc::new(ClipboardOutboundFacade::new(ClipboardOutboundDeps {
        // dispatch path
        settings: input.deps.settings.clone(),
        clipboard_sync: input.clipboard_sync_facade.clone(),
        blob_transfer: input.blob_transfer_facade.clone(),
        // resend path
        entry_repo: input.deps.clipboard.clipboard_entry_repo.clone(),
        event_repo: input.clipboard_event_reader_repo,
        selection_repo: input.deps.clipboard.selection_repo.clone(),
        representation_repo: input.deps.clipboard.representation_repo.clone(),
        payload_resolver: input.deps.clipboard.payload_resolver.clone(),
        blob_store: input.deps.storage.blob_store.clone(),
        entry_delivery_repo: input.entry_delivery_repo,
        trusted_peer_repo: input.trusted_peer_repo,
        device_identity: input.deps.device.device_identity.clone(),
    }));

    let clipboard_change_handler = Arc::new(DaemonClipboardChangeHandler::new(
        input.event_tx.clone(),
        clipboard_change_origin,
        input.clipboard_capture_gate,
        clipboard_capture_facade,
        clipboard_live_index_facade,
        clipboard_outbound_facade.clone(),
    ));
    let clipboard_watcher = Arc::new(ClipboardWatcherWorker::new(
        local_clipboard,
        clipboard_change_handler,
    ));

    let inbound_clipboard_sync = Arc::new(InboundClipboardSyncWorker::new(
        input.clipboard_sync_facade,
        inbound_clipboard_facade,
        input.event_tx,
    ));

    let file_sync_orchestrator = Arc::new(FileSyncOrchestratorWorker::new(
        input.file_transfer_lifecycle,
        input.blob_transfer_facade.clone(),
    ));

    Ok(DaemonRuntimeWorkers {
        clipboard_watcher,
        inbound_clipboard_sync,
        file_sync_orchestrator,
        apply_inbound: apply_inbound_uc,
        clipboard_outbound: clipboard_outbound_facade,
    })
}
