//! daemon 运行时 worker 装配。

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::broadcast;
use uc_application::clipboard_capture::CaptureClipboardUseCase;
use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::clipboard_write::LocalActiveRegisterAdvancer;
use uc_application::deps::AppDeps;
use uc_application::facade::{
    BlobTransferFacade, ClipboardCaptureFacade, ClipboardLiveIndexDeps, ClipboardLiveIndexFacade,
    ClipboardLiveIndexPort, ClipboardLiveIndexer, ClipboardOutboundDeps, ClipboardOutboundFacade,
    ClipboardSyncFacade, HostEventBus, InboundClipboardFacade,
};
use uc_application::{
    ApplyInboundClipboardUseCase, FileCacheBlobMaterializer, InboundCapture as ApplyInboundCapture,
    InboundWrite as ApplyInboundWrite,
};
use uc_bootstrap::{FileTransferLifecycle, SystemClipboardWiring};
use uc_core::ports::{ClipboardEventRepositoryPort, EntryDeliveryRepositoryPort};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_webserver::api::types::DaemonWsEvent;

use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::workers::clipboard_watcher::{
    ClipboardWatcherWorker, DaemonClipboardChangeHandler,
};
use crate::daemon::workers::file_sync_orchestrator::FileSyncOrchestratorWorker;
use crate::daemon::workers::inbound_clipboard_sync::InboundClipboardSyncWorker;

/// daemon worker 装配所需输入。
pub struct DaemonRuntimeAssemblyInput<'a> {
    pub deps: &'a AppDeps,
    /// daemon 运行模式。决定是否装配系统剪贴板出站监听——
    /// `ServerHeadless` 不接 OS 剪贴板，`clipboard_watcher` 产出 `None`。
    pub run_mode: DaemonRunMode,
    /// System-clipboard wiring decision from the composition root
    /// (`create_platform_layer`). `Noop` (explicitly disabled, or headless —
    /// no graphical session) also yields `clipboard_watcher: None`: there is
    /// no OS clipboard to watch, and the platform event loop could only fail
    /// to connect (issue #1021).
    pub system_clipboard_wiring: SystemClipboardWiring,
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
    /// System clipboard outbound watcher worker. `None` under the
    /// `ServerHeadless` run mode and whenever the composition root wired the
    /// no-op clipboard adapter (disabled / headless session) — in both cases
    /// there is no OS clipboard to watch and the service plan won't spawn it.
    pub clipboard_watcher: Option<Arc<ClipboardWatcherWorker>>,
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
    let apply_inbound_capture_uc = Arc::new(
        CaptureClipboardUseCase::new(
            input.deps.clipboard.entry_ports.save.clone(),
            input.deps.clipboard.entry_ports.touch.clone(),
            input
                .deps
                .clipboard
                .entry_ports
                .find_by_snapshot_hash
                .clone(),
            input.deps.clipboard.clipboard_event_repo.clone(),
            input.deps.clipboard.representation_policy.clone(),
            input.deps.clipboard.representation_normalizer.clone(),
            input.deps.device.device_identity.clone(),
            input.deps.clipboard.representation_cache.clone(),
            input.deps.clipboard.spool_queue.clone(),
            input.deps.storage.blob_content_ingest.clone(),
            input.deps.clipboard.entry_ports.replace_content.clone(),
            input.deps.analytics.clone(),
        )
        // Shared so the OS-clipboard watcher's local capture (this same
        // instance, reused below) serializes with inbound apply on a per-content
        // lock — preventing a local copy and an inbound delivery of the same
        // content from creating two entries (R5-F3).
        .with_entry_identity_coordinator(input.deps.clipboard.entry_identity_coordinator.clone()),
    );
    let blob_materializer = Arc::new(FileCacheBlobMaterializer::new(
        input.blob_transfer_facade.clone(),
        input.file_cache_dir,
    ));
    // Shared search live-indexer: indexes both OS-clipboard captures (via the
    // watcher below) and remote-origin inbound entries (P2P + mobile, via
    // ApplyInbound), so remote clipboard becomes searchable like local copies.
    let search_live_indexer: Arc<dyn ClipboardLiveIndexPort> =
        Arc::new(ClipboardLiveIndexer::new(ClipboardLiveIndexDeps {
            clipboard_entry_repo: input.deps.clipboard.entry_ports.get.clone(),
            representation_policy: input.deps.clipboard.representation_policy.clone(),
            search_key_derivation: input.deps.search.search_key_derivation.clone(),
            search_pipeline: input.deps.search.search_pipeline.clone(),
            search_index: input.deps.search.search_index.clone(),
            event_repo: input.clipboard_event_reader_repo.clone(),
        }));
    let apply_inbound_uc = Arc::new(
        ApplyInboundClipboardUseCase::new(
            input
                .deps
                .clipboard
                .entry_ports
                .find_by_snapshot_hash
                .clone(),
            Arc::clone(&apply_inbound_capture_uc) as Arc<dyn ApplyInboundCapture>,
            Arc::clone(&input.clipboard_write_coordinator) as Arc<dyn ApplyInboundWrite>,
        )
        .with_blob_materializer(blob_materializer)
        .with_host_event_emitter(input.host_event_bus)
        .with_active_register(input.deps.clipboard.active_register.clone())
        .with_search_live_index(Arc::clone(&search_live_indexer))
        .with_check_entry_availability(input.deps.clipboard.entry_ports.availability.clone())
        .with_entry_identity_coordinator(input.deps.clipboard.entry_identity_coordinator.clone()),
    );
    let inbound_clipboard_facade = Arc::new(InboundClipboardFacade::new(apply_inbound_uc.clone()));
    let clipboard_outbound_facade = Arc::new(ClipboardOutboundFacade::new(ClipboardOutboundDeps {
        // dispatch path
        settings: input.deps.settings.clone(),
        clipboard_sync: input.clipboard_sync_facade.clone(),
        blob_transfer: input.blob_transfer_facade.clone(),
        // resend path
        entry_repo: input.deps.clipboard.entry_ports.get.clone(),
        event_repo: input.clipboard_event_reader_repo,
        selection_repo: input.deps.clipboard.selection_repo.clone(),
        representation_repo: input.deps.clipboard.representation_ports.get.clone(),
        rep_processing_repo: input
            .deps
            .clipboard
            .representation_ports
            .update_processing_result
            .clone(),
        payload_resolver: input.deps.clipboard.payload_resolver.clone(),
        blob_store: input.deps.storage.blob_store.clone(),
        entry_delivery_repo: input.entry_delivery_repo,
        trusted_peer_repo: input.trusted_peer_repo,
        device_identity: input.deps.device.device_identity.clone(),
    }));

    // System clipboard outbound watcher: assembled only when the run mode
    // takes over the OS clipboard AND the composition root wired the real
    // adapter. Two independent reasons to skip, decided elsewhere and only
    // consumed here:
    // - `ServerHeadless` run mode never integrates with the OS clipboard;
    // - `SystemClipboardWiring::Noop` means there is no OS clipboard to talk
    //   to (explicitly disabled, or headless session — issue #1021), so the
    //   watcher's platform event loop could only fail to connect.
    // Inbound persistence / mobile_lan gateway / fan-out are all unaffected.
    // The watcher reuses the port wired in the platform layer — this is NOT a
    // second clipboard adapter instance.
    let assemble_watcher = input.run_mode.runs_system_clipboard()
        && input.system_clipboard_wiring == SystemClipboardWiring::Real;
    if input.run_mode.runs_system_clipboard() && !assemble_watcher {
        tracing::info!("system clipboard wired as no-op; skipping OS clipboard watcher assembly");
    }
    let clipboard_watcher = if assemble_watcher {
        let local_clipboard = input.deps.clipboard.system_clipboard.clone();
        let clipboard_capture_facade =
            Arc::new(ClipboardCaptureFacade::new(apply_inbound_capture_uc));
        let clipboard_live_index_facade = Arc::new(ClipboardLiveIndexFacade::new(Arc::clone(
            &search_live_indexer,
        )));
        let clipboard_change_handler = Arc::new(DaemonClipboardChangeHandler::new(
            input.event_tx.clone(),
            input.deps.clipboard.clipboard_change_origin.clone(),
            input.clipboard_capture_gate,
            clipboard_capture_facade,
            clipboard_live_index_facade,
            clipboard_outbound_facade.clone(),
            LocalActiveRegisterAdvancer::new(
                input.deps.clipboard.active_register.clone(),
                input.deps.device.device_identity.clone(),
                input.deps.system.clock.clone(),
            ),
        ));
        Some(Arc::new(ClipboardWatcherWorker::new(
            local_clipboard,
            clipboard_change_handler,
        )))
    } else {
        None
    };

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
