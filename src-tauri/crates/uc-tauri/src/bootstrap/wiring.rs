//! # Dependency Injection / 依赖注入模块
//!
//! ## Responsibilities / 职责
//!
//! - ✅ Create infra implementations (db, fs, secure storage) / 创建 infra 层具体实现
//! - ✅ Create platform implementations (clipboard, network) / 创建 platform 层具体实现
//! - ✅ Inject all dependencies into App / 将所有依赖注入到 App
//!
//! ## Prohibited / 禁止事项
//!
//! ❌ **No business logic / 禁止包含任何业务逻辑**
//! - Do not decide "what to do if encryption uninitialized"
//! - 不判断"如果加密未初始化就怎样"
//! - Do not handle "what to do if device not registered"
//! - 不处理"如果设备未注册就怎样"
//!
//! ❌ **No configuration validation / 禁止做配置验证**
//! - Config already loaded in config.rs
//! - 配置已在 config.rs 加载
//! - Validation should be in use case or upper layer
//! - 验证应在 use case 或上层
//!
//! ❌ **No direct concrete implementation usage / 禁止直接使用具体实现**
//! - Must inject through Port traits
//! - 必须通过 Port trait 注入
//! - Do not call implementation methods directly after App construction
//! - 不在 App 构造后直接调用实现方法
//!
//! ## Architecture Principle / 架构原则
//!
//! > **This is the only place allowed to depend on uc-infra + uc-platform + uc-app simultaneously.**
//! > **这是唯一允许同时依赖 uc-infra、uc-platform 和 uc-app 的地方。**
//! > But this privilege is only for "assembly", not for "decision making".
//! > 但这种特权仅用于"组装"，不用于"决策"。

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tauri::async_runtime;
use tokio::sync::mpsc;
use tracing::{debug, error, info, info_span, warn, Instrument};

use super::task_registry::TaskRegistry;
use uc_daemon_client::realtime::start_realtime_runtime;

use uc_app::usecases::clipboard::sync_inbound::{InboundApplyOutcome, SyncInboundClipboardUseCase};
use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_app::AppDeps;
use uc_core::network::ClipboardMessage;
use uc_core::network::NetworkEvent;
use uc_core::ports::clipboard::ClipboardChangeOriginPort;
use uc_core::ports::host_event_emitter::{
    ClipboardHostEvent, ClipboardOriginKind, HostEvent, HostEventEmitterPort,
    PeerConnectionHostEvent, SpaceAccessHostEvent, TransferHostEvent,
};
use uc_core::ports::space::ProofPort;
use uc_core::ports::*;
use uc_core::security::model::{KeySlot, KeySlotFile};
use uc_core::security::space_access::event::SpaceAccessEvent;
use uc_infra::clipboard::{BackgroundBlobWorker, SpoolJanitor, SpoolScanner, SpoolerTask};
// Re-export assembly types from uc-bootstrap (via the thin stub in super::assembly).
pub use super::assembly::{
    get_storage_paths, resolve_pairing_config, resolve_pairing_device_name, wire_dependencies,
    wire_dependencies_with_identity_store, HostEventSetupPort, WiredDependencies, WiringError,
    WiringResult,
};

// Re-export BackgroundRuntimeDeps from uc-bootstrap (definition moved in Phase 40).
pub use uc_bootstrap::BackgroundRuntimeDeps;

const SPOOL_JANITOR_INTERVAL_SECS: u64 = 60 * 60;
const CLIPBOARD_SUBSCRIBE_BACKOFF_INITIAL_MS: u64 = 250;
const CLIPBOARD_SUBSCRIBE_BACKOFF_MAX_MS: u64 = 30_000;
const NETWORK_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS: u64 = 250;
const NETWORK_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS: u64 = 30_000;

fn subscribe_backoff_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let factor = 1u64 << exponent;
    CLIPBOARD_SUBSCRIBE_BACKOFF_INITIAL_MS
        .saturating_mul(factor)
        .min(CLIPBOARD_SUBSCRIBE_BACKOFF_MAX_MS)
}

fn network_events_subscribe_backoff_ms(attempt: u32) -> u64 {
    let exponent = attempt.saturating_sub(1).min(16);
    let factor = 1u64 << exponent;
    NETWORK_EVENTS_SUBSCRIBE_BACKOFF_INITIAL_MS
        .saturating_mul(factor)
        .min(NETWORK_EVENTS_SUBSCRIBE_BACKOFF_MAX_MS)
}

/// Start background spooler and blob worker tasks.
/// 启动后台假脱机写入和 blob 物化任务。
///
/// All long-lived tasks are spawned through the `TaskRegistry` for centralized
/// lifecycle management and graceful shutdown via cooperative cancellation.
pub fn start_background_tasks(
    background: BackgroundRuntimeDeps,
    deps: &AppDeps,
    event_emitter: Arc<dyn HostEventEmitterPort>,
    daemon_connection_state: uc_daemon_client::DaemonConnectionState,
    setup_pairing_event_hub: Arc<uc_app::realtime::SetupPairingEventHub>,
    task_registry: &Arc<TaskRegistry>,
) {
    let BackgroundRuntimeDeps {
        libp2p_network: _,
        representation_cache,
        spool_manager,
        spool_rx,
        worker_rx,
        spool_dir,
        file_cache_dir,
        spool_ttl_days,
        worker_retry_max_attempts,
        worker_retry_backoff_ms,
    } = background;

    info!("Starting background clipboard spooler and blob worker");

    let clipboard_emitter = event_emitter.clone();
    let representation_repo = deps.clipboard.representation_repo.clone();
    let worker_tx = deps.clipboard.worker_tx.clone();
    let blob_writer = deps.storage.blob_writer.clone();
    let hasher = deps.system.hash.clone();
    let clock = deps.system.clock.clone();
    let thumbnail_repo = deps.storage.thumbnail_repo.clone();
    let thumbnail_generator = deps.storage.thumbnail_generator.clone();
    let pairing_events = deps.network_ports.events.clone();
    let peer_directory = deps.network_ports.peers.clone();
    let clipboard_network = deps.network_ports.clipboard.clone();
    let sync_inbound_usecase =
        new_sync_inbound_clipboard_usecase(deps, Some(file_cache_dir.clone()));
    let inbound_file_settings = deps.settings.clone();
    let inbound_file_cache_dir = file_cache_dir;

    // Create clipboard deps for inbound file clipboard integration
    let inbound_system_clipboard = deps.clipboard.system_clipboard.clone();
    let inbound_clipboard_change_origin = deps.clipboard.clipboard_change_origin.clone();

    // File transfer tracking deps
    let inbound_file_transfer_repo = deps.storage.file_transfer_repo.clone();
    let inbound_clock = deps.system.clock.clone();

    // Clones for file cache cleanup task and startup reconciliation
    let deps_settings = deps.settings.clone();
    let cleanup_file_cache_dir = inbound_file_cache_dir.clone();
    let reconcile_file_transfer_repo = deps.storage.file_transfer_repo.clone();
    let reconcile_clock = deps.system.clock.clone();
    let reconcile_emitter = event_emitter.clone();

    // Spawn all long-lived tasks through the TaskRegistry for lifecycle management.
    // We use a single orchestration spawn to set up all registry tasks, since
    // registry.spawn() is async and start_background_tasks is sync.
    let registry = task_registry.clone();
    async_runtime::spawn(async move {
        // --- Spool scanner (runs once at startup, then spawns long-lived sub-tasks) ---
        let scanner = SpoolScanner::new(spool_dir, representation_repo.clone(), worker_tx.clone());
        match scanner.scan_and_recover().await {
            Ok(recovered) => info!("Recovered {} representations from spool", recovered),
            Err(err) => warn!(error = %err, "Spool scan failed; continuing startup"),
        }

        // --- Spooler task (long-lived, channel-driven) ---
        let spooler = SpoolerTask::new(
            spool_rx,
            spool_manager.clone(),
            worker_tx,
            representation_cache.clone(),
        );
        registry
            .spawn("spooler", |_token| async move {
                spooler.run().await;
                warn!("SpoolerTask stopped");
            })
            .await;

        // --- Background blob worker (long-lived, channel-driven) ---
        let worker = BackgroundBlobWorker::new(
            worker_rx,
            representation_cache,
            spool_manager.clone(),
            representation_repo.clone(),
            blob_writer,
            hasher,
            thumbnail_repo,
            thumbnail_generator,
            clock.clone(),
            worker_retry_max_attempts,
            Duration::from_millis(worker_retry_backoff_ms),
        );
        registry
            .spawn("blob_worker", |_token| async move {
                worker.run().await;
                warn!("BackgroundBlobWorker stopped");
            })
            .await;

        // --- Spool janitor (long-lived, interval-based loop with cooperative cancellation) ---
        let janitor = SpoolJanitor::new(
            spool_manager.clone(),
            representation_repo.clone(),
            clock,
            spool_ttl_days,
        );
        registry
            .spawn("spool_janitor", |token| async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(SPOOL_JANITOR_INTERVAL_SECS));
                loop {
                    tokio::select! {
                        _ = token.cancelled() => {
                            info!("Spool janitor shutting down");
                            return;
                        }
                        _ = interval.tick() => {
                            match janitor.run_once().await {
                                Ok(removed) => {
                                    if removed > 0 {
                                        info!("Spool janitor removed {} expired entries", removed);
                                    }
                                }
                                Err(err) => {
                                    warn!(error = %err, "Spool janitor run failed");
                                }
                            }
                        }
                    }
                }
            })
            .await;

        // --- Clipboard receive loop (replaces ctrl_c with CancellationToken) ---
        let clipboard_transfer_tracker = Arc::new(
            uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(
                inbound_file_transfer_repo.clone(),
            ),
        );
        let clipboard_clock = inbound_clock.clone();

        // Shared early-completion cache: captures completions that arrive
        // before the pending record is seeded (race condition fix).
        let early_completion_cache =
            Arc::new(super::file_transfer_wiring::EarlyCompletionCache::default());
        let pairing_early_completion_cache = early_completion_cache.clone();

        register_pairing_background_tasks(
            &registry,
            pairing_events,
            peer_directory,
            event_emitter.clone(),
            inbound_file_settings.clone(),
            inbound_file_cache_dir.clone(),
            inbound_system_clipboard.clone(),
            inbound_clipboard_change_origin.clone(),
            inbound_file_transfer_repo.clone(),
            inbound_clock.clone(),
            pairing_early_completion_cache,
        )
        .await;

        registry
            .spawn("clipboard_receive", |token| {
                async move {
                    let mut subscribe_attempt: u32 = 0;
                    let mut first_subscribe_failure_emitted = false;

                    loop {
                        let subscribe_result = tokio::select! {
                            _ = token.cancelled() => {
                                info!("Clipboard receive task stopping on shutdown signal");
                                return;
                            }
                            result = clipboard_network.subscribe_clipboard() => result,
                        };

                        match subscribe_result {
                            Ok(clipboard_rx) => {
                                if subscribe_attempt > 0 {
                                    info!(
                                        attempts = subscribe_attempt,
                                        "Recovered clipboard subscription"
                                    );

                                    if let Err(err) = clipboard_emitter.emit(HostEvent::Clipboard(
                                        ClipboardHostEvent::InboundSubscribeRecovered {
                                            recovered_after_attempts: subscribe_attempt,
                                        },
                                    )) {
                                        warn!(
                                            error = %err,
                                            "Failed to emit inbound clipboard subscribe recovered event"
                                        );
                                    }
                                }

                                subscribe_attempt = 0;
                                first_subscribe_failure_emitted = false;
                                run_clipboard_receive_loop(
                                    clipboard_rx,
                                    &sync_inbound_usecase,
                                    clipboard_emitter.clone(),
                                    Some(clipboard_transfer_tracker.clone()),
                                    Some(clipboard_clock.clone()),
                                    Some(early_completion_cache.clone()),
                                )
                                .await;
                            }
                            Err(err) => {
                                subscribe_attempt = subscribe_attempt.saturating_add(1);
                                let retry_in_ms = subscribe_backoff_ms(subscribe_attempt);

                                warn!(
                                    error = %err,
                                    attempt = subscribe_attempt,
                                    retry_in_ms,
                                    "Failed to subscribe to clipboard messages"
                                );

                                if !first_subscribe_failure_emitted {
                                    if let Err(emit_err) =
                                        clipboard_emitter.emit(HostEvent::Clipboard(
                                            ClipboardHostEvent::InboundSubscribeError {
                                                attempt: subscribe_attempt,
                                                error: err.to_string(),
                                            },
                                        ))
                                    {
                                        warn!(
                                            error = %emit_err,
                                            "Failed to emit inbound clipboard subscribe error event"
                                        );
                                    }
                                    first_subscribe_failure_emitted = true;
                                }

                                if let Err(emit_err) =
                                    clipboard_emitter.emit(HostEvent::Clipboard(
                                        ClipboardHostEvent::InboundSubscribeRetry {
                                            attempt: subscribe_attempt,
                                            retry_in_ms,
                                            error: err.to_string(),
                                        },
                                    ))
                                {
                                    warn!(
                                        error = %emit_err,
                                        "Failed to emit inbound clipboard subscribe retry event"
                                    );
                                }

                                let backoff = Duration::from_millis(retry_in_ms);
                                tokio::select! {
                                    _ = token.cancelled() => {
                                        info!("Clipboard receive task stopping during backoff on shutdown signal");
                                        return;
                                    }
                                    _ = tokio::time::sleep(backoff) => {}
                                }
                            }
                        }
                    }
                }
                .instrument(info_span!("loop.clipboard.receive_task"))
            })
            .await;

        // --- Unified realtime runtime (daemon WebSocket bridge + app consumers) ---
        start_realtime_runtime(
            daemon_connection_state,
            event_emitter.clone(),
            setup_pairing_event_hub,
            &registry,
        )
        .await;
        info!("Started unified daemon realtime runtime");

        // --- File cache cleanup (runs once at startup, fire-and-forget) ---
        {
            let cleanup_settings = deps_settings.clone();
            let cleanup_cache_dir = cleanup_file_cache_dir.clone();
            registry
                .spawn("file_cache_cleanup", |_token| async move {
                    let uc = uc_app::usecases::file_sync::CleanupExpiredFilesUseCase::new(
                        cleanup_settings,
                        cleanup_cache_dir,
                    );
                    match uc.execute().await {
                        Ok(result) => {
                            if result.files_removed > 0 {
                                info!(
                                    files_removed = result.files_removed,
                                    bytes_reclaimed = result.bytes_reclaimed,
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
        }

        // --- File transfer startup reconciliation (runs once, fire-and-forget) ---
        {
            let reconcile_repo = reconcile_file_transfer_repo.clone();
            let reconcile_clk = reconcile_clock.clone();
            let reconcile_emit = reconcile_emitter.clone();
            registry
                .spawn("file_transfer_reconcile", |_token| async move {
                    let tracker = uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(
                        reconcile_repo,
                    );
                    let now_ms = reconcile_clk.now_ms();
                    super::file_transfer_wiring::reconcile_on_startup(
                        &tracker,
                        &*reconcile_emit,
                        now_ms,
                    )
                    .await;
                })
                .await;
        }

        // --- File transfer timeout sweep (long-lived, interval-based) ---
        {
            let sweep_repo = reconcile_file_transfer_repo;
            let sweep_clock = reconcile_clock;
            let sweep_emitter = reconcile_emitter;
            let sweep_tracker = Arc::new(
                uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(sweep_repo),
            );
            let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
            let _sweep_handle = super::file_transfer_wiring::spawn_timeout_sweep(
                sweep_tracker,
                sweep_emitter,
                sweep_clock,
                cancel_rx,
            );
            // Cancel sender is dropped when the registry shuts down
            // (the sweep task will terminate when cancel_tx is dropped)
            std::mem::forget(cancel_tx);
        }

        info!("All background tasks registered with TaskRegistry");
    });
}

fn new_sync_inbound_clipboard_usecase(
    deps: &AppDeps,
    file_cache_dir: Option<PathBuf>,
) -> SyncInboundClipboardUseCase {
    let mode = super::resolve_clipboard_integration_mode();
    SyncInboundClipboardUseCase::with_capture_dependencies(
        mode,
        deps.clipboard.system_clipboard.clone(),
        deps.clipboard.clipboard_change_origin.clone(),
        deps.security.encryption_session.clone(),
        deps.security.encryption.clone(),
        deps.device.device_identity.clone(),
        Arc::new(uc_infra::clipboard::TransferPayloadDecryptorAdapter),
        deps.clipboard.clipboard_entry_repo.clone(),
        deps.clipboard.clipboard_event_repo.clone(),
        deps.clipboard.representation_policy.clone(),
        deps.clipboard.representation_normalizer.clone(),
        deps.clipboard.representation_cache.clone(),
        deps.clipboard.spool_queue.clone(),
        file_cache_dir,
        deps.settings.clone(),
    )
}

async fn register_pairing_background_tasks(
    registry: &Arc<TaskRegistry>,
    pairing_events: Arc<dyn NetworkEventPort>,
    peer_directory: Arc<dyn PeerDirectoryPort>,
    event_emitter: Arc<dyn HostEventEmitterPort>,
    inbound_file_settings: Arc<dyn SettingsPort>,
    inbound_file_cache_dir: PathBuf,
    inbound_system_clipboard: Arc<dyn SystemClipboardPort>,
    inbound_clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    inbound_file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
    inbound_clock: Arc<dyn uc_core::ports::ClockPort>,
    early_completion_cache: Arc<super::file_transfer_wiring::EarlyCompletionCache>,
) {
    registry
        .spawn("pairing_events", |token| async move {
            let mut subscribe_attempt: u32 = 0;

            loop {
                let subscribe_result = tokio::select! {
                    _ = token.cancelled() => {
                        info!("Pairing event task stopping on shutdown signal");
                        return;
                    }
                    result = pairing_events.subscribe_events() => result,
                };

                match subscribe_result {
                    Ok(event_rx) => {
                        if subscribe_attempt > 0 {
                            info!(
                                attempts = subscribe_attempt,
                                "Recovered pairing event subscription"
                            );
                        }
                        subscribe_attempt = 0;

                        tokio::select! {
                            _ = token.cancelled() => {
                                info!("Pairing event task stopping on shutdown signal");
                                return;
                            }
                            _ = run_network_realtime_loop(
                                event_rx,
                                event_emitter.clone(),
                                peer_directory.clone(),
                                inbound_file_settings.clone(),
                                inbound_file_cache_dir.clone(),
                                inbound_system_clipboard.clone(),
                                inbound_clipboard_change_origin.clone(),
                                inbound_file_transfer_repo.clone(),
                                inbound_clock.clone(),
                                early_completion_cache.clone(),
                            ) => {
                                warn!("Pairing event loop stopped");
                            }
                        }
                    }
                    Err(err) => {
                        subscribe_attempt = subscribe_attempt.saturating_add(1);
                        let retry_in_ms = network_events_subscribe_backoff_ms(subscribe_attempt);

                        warn!(
                            error = %err,
                            attempt = subscribe_attempt,
                            retry_in_ms,
                            "Failed to subscribe to pairing network events"
                        );
                    }
                }

                let backoff =
                    Duration::from_millis(network_events_subscribe_backoff_ms(subscribe_attempt));
                tokio::select! {
                    _ = token.cancelled() => {
                        info!("Pairing event task stopping during backoff on shutdown signal");
                        return;
                    }
                    _ = tokio::time::sleep(backoff) => {}
                }
            }
        })
        .await;
}

async fn run_clipboard_receive_loop(
    mut clipboard_rx: mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>,
    usecase: &SyncInboundClipboardUseCase,
    event_emitter: Arc<dyn HostEventEmitterPort>,
    transfer_tracker: Option<Arc<uc_app::usecases::file_sync::TrackInboundTransfersUseCase>>,
    clock: Option<Arc<dyn uc_core::ports::ClockPort>>,
    early_completion_cache: Option<Arc<super::file_transfer_wiring::EarlyCompletionCache>>,
) {
    while let Some((message, pre_decoded)) = clipboard_rx.recv().await {
        let flow_id = uc_observability::FlowId::generate();
        let message_id = message.id.clone();
        let origin_device_id = message.origin_device_id.clone();
        let origin_flow_id_display = message.origin_flow_id.as_deref().unwrap_or("");

        // Warn if message is from an older peer that doesn't send origin_flow_id
        if message.origin_flow_id.is_none() {
            warn!(
                message_id = %message_id,
                origin_device_id = %origin_device_id,
                "Inbound message has no origin_flow_id (sender may be an older version)"
            );
        }

        let span = info_span!(
            "loop.clipboard.receive_message",
            %flow_id,
            message_id = %message_id,
            origin_device_id = %origin_device_id,
            origin_flow_id = origin_flow_id_display,
        );

        let result = async { usecase.execute_with_outcome(message, pre_decoded).await }
            .instrument(span)
            .await;

        match result {
            Ok(outcome) => {
                // Persist pending transfer records and emit status for file transfers
                if let InboundApplyOutcome::Applied {
                    entry_id: Some(ref entry_id),
                    ref pending_transfers,
                } = outcome
                {
                    if !pending_transfers.is_empty() {
                        // Persist pending records to DB so mark_completed/mark_transferring can find them
                        if let (Some(tracker), Some(clk)) =
                            (transfer_tracker.as_ref(), clock.as_ref())
                        {
                            let now_ms = clk.now_ms();
                            let db_transfers: Vec<
                                uc_core::ports::file_transfer_repository::PendingInboundTransfer,
                            > = pending_transfers
                                .iter()
                                .map(|t| {
                                    uc_core::ports::file_transfer_repository::PendingInboundTransfer {
                                        transfer_id: t.transfer_id.clone(),
                                        entry_id: entry_id.to_string(),
                                        origin_device_id: origin_device_id.clone(),
                                        filename: t.filename.clone(),
                                        cached_path: t.cached_path.clone(),
                                        created_at_ms: now_ms,
                                    }
                                })
                                .collect();
                            if let Err(err) =
                                tracker.record_pending_from_clipboard(db_transfers).await
                            {
                                warn!(
                                    error = %err,
                                    message_id = %message_id,
                                    "Failed to persist pending transfer records"
                                );
                            } else if let Some(cache) = early_completion_cache.as_ref() {
                                // Reconcile early completions that arrived before seeding
                                let seeded_ids: Vec<String> = pending_transfers
                                    .iter()
                                    .map(|t| t.transfer_id.clone())
                                    .collect();
                                let early = cache.drain_matching(&seeded_ids);
                                for (tid, info) in &early {
                                    info!(
                                        transfer_id = %tid,
                                        "Reconciling early completion after seeding"
                                    );
                                    match tracker
                                        .mark_completed(
                                            tid,
                                            info.content_hash.as_deref(),
                                            info.completed_at_ms,
                                        )
                                        .await
                                    {
                                        Ok(_) => {
                                            if let Err(err) =
                                                event_emitter.emit(HostEvent::Transfer(
                                                    TransferHostEvent::StatusChanged {
                                                        transfer_id: tid.clone(),
                                                        entry_id: entry_id.to_string(),
                                                        status: "completed".to_string(),
                                                        reason: None,
                                                    },
                                                ))
                                            {
                                                warn!(error = %err, "Failed to emit reconciled completion status");
                                            }
                                        }
                                        Err(err) => {
                                            warn!(
                                                error = %err,
                                                transfer_id = %tid,
                                                "Failed to reconcile early completion"
                                            );
                                        }
                                    }
                                }
                            }
                        }

                        // Emit pending status events to frontend
                        super::file_transfer_wiring::emit_pending_status(
                            event_emitter.as_ref(),
                            &entry_id.to_string(),
                            pending_transfers,
                        );
                    }
                }

                // Emit clipboard://event so frontend list refreshes.
                // In Passive mode: always emit (no OS clipboard write happens).
                // In Full mode: emit only for file entries (OS clipboard write is skipped for files).
                match outcome {
                    InboundApplyOutcome::Applied {
                        entry_id: Some(entry_id),
                        ref pending_transfers,
                    } => {
                        let is_passive = matches!(
                            usecase.mode(),
                            uc_app::usecases::clipboard::ClipboardIntegrationMode::Passive
                        );
                        let has_file_transfers = !pending_transfers.is_empty();

                        // Passive mode always needs explicit event (no ClipboardWatcher).
                        // Full mode with file transfers also needs it (write_snapshot is skipped).
                        if is_passive || has_file_transfers {
                            if let Err(emit_err) = event_emitter.emit(HostEvent::Clipboard(
                                ClipboardHostEvent::NewContent {
                                    entry_id: entry_id.to_string(),
                                    preview: "Remote clipboard content applied".to_string(),
                                    origin: ClipboardOriginKind::Remote,
                                },
                            )) {
                                warn!(error = %emit_err, message_id = %message_id, "Failed to emit clipboard event after inbound apply");
                            }
                        }
                    }
                    InboundApplyOutcome::Applied { entry_id: None, .. } => {
                        if matches!(
                            usecase.mode(),
                            uc_app::usecases::clipboard::ClipboardIntegrationMode::Passive
                        ) {
                            warn!(
                                message_id = %message_id,
                                "Inbound apply reported success in passive mode without persisted entry id"
                            );
                        }
                    }
                    InboundApplyOutcome::Skipped => {}
                }
            }
            Err(err) => {
                warn!(
                    error = %err,
                    message_id = %message_id,
                    origin_device_id = %origin_device_id,
                    "Failed to apply inbound clipboard message"
                );

                if let Err(emit_err) =
                    event_emitter.emit(HostEvent::Clipboard(ClipboardHostEvent::InboundError {
                        message_id: message_id.clone(),
                        origin_device_id: origin_device_id.clone(),
                        error: err.to_string(),
                    }))
                {
                    warn!(error = %emit_err, "Failed to emit inbound clipboard error event");
                }
            }
        }
    }

    info!("Clipboard receive channel closed; stopping background receive loop");
}

#[derive(Clone)]
#[cfg(test)]
struct RuntimeSpaceAccessPorts {
    transport: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::SpaceAccessTransportPort>>,
    proof: Arc<dyn ProofPort>,
    timer: Arc<tokio::sync::Mutex<dyn TimerPort>>,
    persistence: Arc<tokio::sync::Mutex<dyn uc_core::ports::space::PersistencePort>>,
}

#[cfg(test)]
async fn dispatch_space_access_busy_event(
    orchestrator: &SpaceAccessOrchestrator,
    runtime_ports: &RuntimeSpaceAccessPorts,
    event: SpaceAccessEvent,
    session_id: &str,
) -> Result<(), uc_app::usecases::space_access::SpaceAccessError> {
    let noop_crypto = NoopSpaceAccessCrypto;
    let mut transport = runtime_ports.transport.lock().await;
    let mut timer = runtime_ports.timer.lock().await;
    let mut store = runtime_ports.persistence.lock().await;

    orchestrator
        .dispatch(
            &mut uc_app::usecases::space_access::SpaceAccessExecutor {
                crypto: &noop_crypto,
                transport: &mut *transport,
                proof: runtime_ports.proof.as_ref(),
                timer: &mut *timer,
                store: &mut *store,
            },
            event,
            Some(session_id.to_string()),
        )
        .await
        .map(|_| ())
}

#[cfg(test)]
const BUSY_PAYLOAD_PREVIEW_MAX_CHARS: usize = 256;

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg(test)]
struct SpaceAccessBusyOfferPayload {
    kind: String,
    space_id: String,
    nonce: Vec<u8>,
    keyslot: KeySlot,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg(test)]
struct SpaceAccessBusyProofPayload {
    kind: String,
    pairing_session_id: String,
    space_id: String,
    challenge_nonce: Vec<u8>,
    proof_bytes: Vec<u8>,
}

#[allow(dead_code)]
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg(test)]
struct SpaceAccessBusyResultPayload {
    kind: String,
    space_id: String,
    #[serde(default)]
    sponsor_peer_id: Option<String>,
    success: bool,
    #[serde(default)]
    deny_reason: Option<String>,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
#[cfg(test)]
enum SpaceAccessBusyPayload {
    Offer(SpaceAccessBusyOfferPayload),
    Proof(SpaceAccessBusyProofPayload),
    Result(SpaceAccessBusyResultPayload),
}

#[derive(Debug, thiserror::Error)]
#[cfg(test)]
enum ParseError {
    #[error("busy payload is not valid json: {source}")]
    InvalidJson {
        #[source]
        source: serde_json::Error,
    },
    #[error("busy payload missing string field `kind`")]
    MissingKind,
    #[error("busy payload kind `{kind}` is not supported")]
    UnknownKind { kind: String },
    #[error("busy payload kind `{kind}` has invalid structure: {source}")]
    InvalidStructure {
        kind: String,
        #[source]
        source: serde_json::Error,
    },
}

#[cfg(test)]
impl ParseError {
    fn payload_kind(&self) -> Option<&str> {
        match self {
            Self::UnknownKind { kind } | Self::InvalidStructure { kind, .. } => Some(kind.as_str()),
            Self::InvalidJson { .. } | Self::MissingKind => None,
        }
    }
}

#[cfg(test)]
fn parse_space_access_busy_payload(json: &str) -> Result<SpaceAccessBusyPayload, ParseError> {
    let payload: serde_json::Value =
        serde_json::from_str(json).map_err(|source| ParseError::InvalidJson { source })?;

    let kind = payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .ok_or(ParseError::MissingKind)?
        .to_string();

    match kind.as_str() {
        "space_access_offer" => serde_json::from_value::<SpaceAccessBusyOfferPayload>(payload)
            .map(SpaceAccessBusyPayload::Offer)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        "space_access_proof" => serde_json::from_value::<SpaceAccessBusyProofPayload>(payload)
            .map(SpaceAccessBusyPayload::Proof)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        "space_access_result" => serde_json::from_value::<SpaceAccessBusyResultPayload>(payload)
            .map(SpaceAccessBusyPayload::Result)
            .map_err(|source| ParseError::InvalidStructure {
                kind: kind.clone(),
                source,
            }),
        _ => Err(ParseError::UnknownKind { kind }),
    }
}

#[cfg(test)]
fn extract_space_access_busy_payload_kind(json: &str) -> Option<String> {
    let payload: serde_json::Value = serde_json::from_str(json).ok()?;
    payload
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
fn raw_payload_preview(payload: &str) -> String {
    let mut chars = payload.chars();
    let mut preview: String = chars
        .by_ref()
        .take(BUSY_PAYLOAD_PREVIEW_MAX_CHARS)
        .collect();
    if chars.next().is_some() {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
struct NoopSpaceAccessCrypto;

#[cfg(test)]
struct LoadedKeyslotSpaceAccessCrypto {
    keyslot_file: KeySlotFile,
}

#[cfg(test)]
impl LoadedKeyslotSpaceAccessCrypto {
    fn new(keyslot_file: KeySlotFile) -> Self {
        Self { keyslot_file }
    }
}

#[async_trait::async_trait]
#[cfg(test)]
impl uc_core::ports::space::CryptoPort for NoopSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(
        &self,
        _space_id: &uc_core::ids::SpaceId,
    ) -> anyhow::Result<uc_core::security::model::KeySlot> {
        Err(anyhow::anyhow!(
            "noop crypto port cannot export keyslot blob"
        ))
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> anyhow::Result<uc_core::security::model::MasterKey> {
        Err(anyhow::anyhow!("noop crypto port cannot derive master key"))
    }
}

#[async_trait::async_trait]
#[cfg(test)]
impl uc_core::ports::space::CryptoPort for LoadedKeyslotSpaceAccessCrypto {
    async fn generate_nonce32(&self) -> [u8; 32] {
        [0u8; 32]
    }

    async fn export_keyslot_blob(
        &self,
        _space_id: &uc_core::ids::SpaceId,
    ) -> anyhow::Result<uc_core::security::model::KeySlot> {
        Ok(self.keyslot_file.clone().into())
    }

    async fn derive_master_key_from_keyslot(
        &self,
        _keyslot_blob: &[u8],
        _passphrase: uc_core::security::SecretString,
    ) -> anyhow::Result<uc_core::security::model::MasterKey> {
        Err(anyhow::anyhow!(
            "loaded keyslot crypto cannot derive master key in sponsor flow"
        ))
    }
}

async fn resolve_device_name_for_peer(
    network: &Arc<dyn PeerDirectoryPort>,
    peer_id: &str,
) -> Option<String> {
    match network.get_discovered_peers().await {
        Ok(peers) => peers
            .into_iter()
            .find(|peer| peer.peer_id == peer_id)
            .and_then(|peer| peer.device_name),
        Err(err) => {
            warn!(error = %err, peer_id = %peer_id, "Failed to load discovered peers");
            None
        }
    }
}

/// Restore received file paths to system clipboard after transfer completes.
/// DB entry was already created by inbound clipboard sync, so this uses
/// `LocalRestore` origin to prevent the clipboard watcher from re-capturing.
/// Checks for clipboard race (FCLIP-03): if user copied other content
/// during transfer, auto-restore is skipped.
async fn restore_file_to_clipboard_after_transfer(
    file_paths: Vec<PathBuf>,
    system_clipboard: &Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: &Arc<dyn ClipboardChangeOriginPort>,
) {
    use uc_app::usecases::file_sync::copy_file_to_clipboard::{
        build_file_snapshot, build_path_list,
    };

    // Canonicalize paths to absolute paths.
    // The clipboard (CF_HDROP on Windows, NSPasteboard on macOS) requires absolute
    // paths; relative paths like ".app_data/cache/..." won't resolve when pasting.
    let file_paths: Vec<PathBuf> = file_paths
        .into_iter()
        .map(|p| {
            if p.is_relative() {
                match p.canonicalize() {
                    Ok(abs) => abs,
                    Err(err) => {
                        warn!(
                            path = %p.display(),
                            error = %err,
                            "Failed to canonicalize relative file path, using as-is"
                        );
                        p
                    }
                }
            } else {
                p
            }
        })
        .collect();

    // Verify all files exist before attempting clipboard write
    let files_exist: Vec<bool> = file_paths.iter().map(|p| p.exists()).collect();
    let all_exist = files_exist.iter().all(|&e| e);
    info!(
        file_count = file_paths.len(),
        paths = ?file_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
        files_exist = ?files_exist,
        all_exist,
        "restore_file_to_clipboard_after_transfer: starting restore"
    );

    if !all_exist {
        warn!(
            paths = ?file_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            files_exist = ?files_exist,
            "Some files do not exist on disk — clipboard write will likely fail"
        );
    }

    let path_list = build_path_list(&file_paths);
    let snapshot = build_file_snapshot(&path_list);

    // FCLIP-03: Non-destructive check for concurrent clipboard operations.
    // Use has_pending_origin() (peek) instead of consume_origin_or_default()
    // to avoid stealing another restore's LocalRestore origin protection,
    // which would leave that restore's clipboard write unprotected and
    // cause a ping-pong bounce-back.
    if clipboard_change_origin.has_pending_origin().await {
        info!(
            file_count = file_paths.len(),
            "Concurrent clipboard operation detected, skipping auto-restore. Files available in Dashboard."
        );
        return;
    }

    // Set origin to LocalRestore so the clipboard watcher skips capture entirely.
    // The DB entry was already created by inbound sync — RemotePush would still
    // trigger a duplicate capture; only LocalRestore is skipped.
    clipboard_change_origin
        .set_next_origin(
            uc_core::ClipboardChangeOrigin::LocalRestore,
            std::time::Duration::from_secs(2),
        )
        .await;

    // Restore to system clipboard
    info!(
        path_list = %path_list,
        "restore_file_to_clipboard_after_transfer: restoring to OS clipboard"
    );
    if let Err(err) = system_clipboard.write_snapshot(snapshot) {
        // Consume origin on failure to avoid stale origin
        clipboard_change_origin
            .consume_origin_or_default(uc_core::ClipboardChangeOrigin::LocalCapture)
            .await;
        warn!(error = %err, "Failed to write file URIs to system clipboard");
    } else {
        info!(
            file_count = file_paths.len(),
            "File URIs written to system clipboard"
        );
    }
}

async fn run_network_realtime_loop(
    mut event_rx: mpsc::Receiver<NetworkEvent>,
    event_emitter: Arc<dyn HostEventEmitterPort>,
    peer_directory: Arc<dyn PeerDirectoryPort>,
    inbound_file_settings: Arc<dyn SettingsPort>,
    inbound_file_cache_dir: PathBuf,
    system_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    file_transfer_repo: Arc<dyn uc_core::ports::FileTransferRepositoryPort>,
    clock: Arc<dyn uc_core::ports::ClockPort>,
    early_completion_cache: Arc<super::file_transfer_wiring::EarlyCompletionCache>,
) {
    // Batch accumulator: batch_id -> (completed_paths: Vec<PathBuf>, expected_total: u32, peer_id: String)
    let mut batch_accumulator: std::collections::HashMap<String, (Vec<PathBuf>, u32, String)> =
        std::collections::HashMap::new();

    // File transfer tracker for durable status transitions
    let transfer_tracker = Arc::new(
        uc_app::usecases::file_sync::TrackInboundTransfersUseCase::new(file_transfer_repo),
    );

    while let Some(event) = event_rx.recv().await {
        match event {
            NetworkEvent::PeerDiscovered(peer) => {
                debug!(
                    peer_id = %peer.peer_id,
                    address_count = peer.addresses.len(),
                    is_paired = peer.is_paired,
                    "Ignoring local peer discovered event; daemon owns frontend peer discovery"
                );

                // Announce our device name so the remote peer can display it
                // in its device-selection UI (before pairing begins).
                let device_name = resolve_pairing_device_name(inbound_file_settings.clone()).await;
                if let Err(err) = peer_directory.announce_device_name(device_name).await {
                    warn!(
                        error = %err,
                        "Failed to announce device name after peer discovery"
                    );
                }
            }
            NetworkEvent::PeerLost(peer_id) => {
                debug!(
                    peer_id = %peer_id,
                    "Ignoring local peer lost event; daemon owns frontend peer discovery"
                );
            }
            NetworkEvent::PeerReady { ref peer_id }
            | NetworkEvent::PeerNotReady { ref peer_id } => {
                let connected = matches!(event, NetworkEvent::PeerReady { .. });
                let device_name = resolve_device_name_for_peer(&peer_directory, peer_id).await;
                if connected {
                    if let Err(err) = event_emitter.emit(HostEvent::PeerConnection(
                        PeerConnectionHostEvent::Connected {
                            peer_id: peer_id.clone(),
                            device_name,
                        },
                    )) {
                        warn!(error = %err, "Failed to emit peer connection event");
                    }
                } else if let Err(err) = event_emitter.emit(HostEvent::PeerConnection(
                    PeerConnectionHostEvent::Disconnected {
                        peer_id: peer_id.clone(),
                        device_name,
                    },
                )) {
                    warn!(error = %err, "Failed to emit peer connection event");
                }
            }
            NetworkEvent::PeerConnected(peer) => {
                if let Err(err) = event_emitter.emit(HostEvent::PeerConnection(
                    PeerConnectionHostEvent::Connected {
                        peer_id: peer.peer_id,
                        device_name: Some(peer.device_name),
                    },
                )) {
                    warn!(error = %err, "Failed to emit peer connection event");
                }
            }
            NetworkEvent::PeerDisconnected(peer_id) => {
                let device_name = resolve_device_name_for_peer(&peer_directory, &peer_id).await;
                if let Err(err) = event_emitter.emit(HostEvent::PeerConnection(
                    PeerConnectionHostEvent::Disconnected {
                        peer_id,
                        device_name,
                    },
                )) {
                    warn!(error = %err, "Failed to emit peer connection event");
                }
            }
            NetworkEvent::PeerNameUpdated {
                peer_id,
                device_name,
            } => {
                if let Err(err) = event_emitter.emit(HostEvent::PeerConnection(
                    PeerConnectionHostEvent::NameUpdated {
                        peer_id,
                        device_name,
                    },
                )) {
                    warn!(error = %err, "Failed to emit peer name updated event");
                }
            }
            NetworkEvent::TransferProgress(progress) => {
                // Track durable status transitions (pending->transferring, liveness refresh)
                let now_ms = clock.now_ms();
                super::file_transfer_wiring::handle_transfer_progress(
                    transfer_tracker.as_ref(),
                    event_emitter.as_ref(),
                    &progress.transfer_id,
                    progress.direction.clone(),
                    progress.chunks_completed,
                    now_ms,
                )
                .await;

                // Forward the transient progress event to frontend
                if let Err(err) =
                    event_emitter.emit(HostEvent::Transfer(TransferHostEvent::Progress(progress)))
                {
                    warn!(error = %err, "Failed to emit transfer progress event");
                }
            }
            NetworkEvent::FileTransferCompleted {
                transfer_id,
                peer_id,
                filename,
                file_path,
                batch_id,
                batch_total,
            } => {
                info!(
                    transfer_id = %transfer_id,
                    peer_id = %peer_id,
                    filename = %filename,
                    file_path = %file_path.display(),
                    batch_id = ?batch_id,
                    batch_total = ?batch_total,
                    "File transfer completed, processing inbound file"
                );

                let inbound_uc = uc_app::usecases::file_sync::SyncInboundFileUseCase::new(
                    inbound_file_settings.clone(),
                    inbound_file_cache_dir.clone(),
                );

                // Clone tracker for spawn
                let tracker_for_spawn = transfer_tracker.clone();
                let clock_for_spawn = clock.clone();
                let early_cache_for_spawn = early_completion_cache.clone();

                // Clone values before spawn takes ownership
                let emitter_for_spawn = event_emitter.clone();
                let span_transfer_id = transfer_id.clone();
                let system_clipboard_clone = system_clipboard.clone();
                let clipboard_change_origin_clone = clipboard_change_origin.clone();
                let file_path_for_spawn = file_path.clone();
                let peer_id_for_spawn = peer_id.clone();
                let filename_for_spawn = filename.clone();
                let transfer_id_for_spawn = transfer_id.clone();
                let is_batch = batch_id.is_some() && batch_total.is_some();
                tokio::spawn(
                    async move {
                        let file_bytes = match tokio::fs::read(&file_path_for_spawn).await {
                            Ok(bytes) => bytes,
                            Err(err) => {
                                error!(
                                    transfer_id = %transfer_id_for_spawn,
                                    error = %err,
                                    "Failed to read transferred file for hash verification"
                                );
                                // Mark durable failure
                                let now_ms = clock_for_spawn.now_ms();
                                super::file_transfer_wiring::handle_transfer_failed(
                                    tracker_for_spawn.as_ref(),
                                    emitter_for_spawn.as_ref(),
                                    &transfer_id_for_spawn,
                                    &format!("Failed to read file: {}", err),
                                    now_ms,
                                )
                                .await;
                                return;
                            }
                        };

                        let expected_hash = blake3::hash(&file_bytes).to_hex().to_string();

                        match inbound_uc
                            .handle_transfer_complete(
                                &transfer_id_for_spawn,
                                &file_path_for_spawn,
                                &expected_hash,
                            )
                            .await
                        {
                            Ok(result) => {
                                info!(
                                    transfer_id = %result.transfer_id,
                                    file_size = result.file_size,
                                    auto_pulled = result.auto_pulled,
                                    "Inbound file sync processed"
                                );

                                // Mark durable completion before emitting events
                                let now_ms = clock_for_spawn.now_ms();
                                super::file_transfer_wiring::handle_transfer_completed(
                                    tracker_for_spawn.as_ref(),
                                    emitter_for_spawn.as_ref(),
                                    &result.transfer_id,
                                    Some(&expected_hash),
                                    now_ms,
                                    Some(early_cache_for_spawn.as_ref()),
                                )
                                .await;

                                // Emit the existing file-transfer://completed event
                                // (UI code depends on it; status-changed is the durable authority)
                                if let Err(err) = emitter_for_spawn.emit(HostEvent::Transfer(
                                    TransferHostEvent::Completed {
                                        transfer_id: result.transfer_id,
                                        filename: filename_for_spawn,
                                        peer_id: peer_id_for_spawn,
                                        file_size: result.file_size,
                                        auto_pulled: result.auto_pulled,
                                        file_path: result.file_path.to_string_lossy().to_string(),
                                    },
                                )) {
                                    warn!(
                                        error = %err,
                                        "Failed to emit file transfer completed event"
                                    );
                                }

                                // Restore single file to clipboard only if NOT part of a batch
                                // Batch clipboard restores are handled by the batch accumulator
                                if !is_batch {
                                    restore_file_to_clipboard_after_transfer(
                                        vec![result.file_path],
                                        &system_clipboard_clone,
                                        &clipboard_change_origin_clone,
                                    )
                                    .await;
                                }
                            }
                            Err(err) => {
                                error!(
                                    transfer_id = %transfer_id_for_spawn,
                                    error = %err,
                                    "Inbound file sync processing failed"
                                );
                                // Mark durable failure
                                let now_ms = clock_for_spawn.now_ms();
                                super::file_transfer_wiring::handle_transfer_failed(
                                    tracker_for_spawn.as_ref(),
                                    emitter_for_spawn.as_ref(),
                                    &transfer_id_for_spawn,
                                    &format!("Inbound file sync failed: {}", err),
                                    now_ms,
                                )
                                .await;
                            }
                        }
                    }
                    .instrument(info_span!(
                        "inbound_file_sync",
                        transfer_id = %span_transfer_id,
                    )),
                );

                // Handle batch accumulation (outside spawn for state access)
                if let (Some(bid), Some(total)) = (batch_id, batch_total) {
                    let entry = batch_accumulator
                        .entry(bid.clone())
                        .or_insert_with(|| (Vec::new(), total, peer_id.clone()));
                    entry.0.push(file_path.clone());

                    if entry.0.len() < total as usize {
                        info!(
                            batch_id = %bid,
                            completed = entry.0.len(),
                            total = total,
                            "Batch file received, waiting for remaining files"
                        );
                    } else {
                        let all_paths = entry.0.clone();
                        batch_accumulator.remove(&bid);
                        info!(
                            batch_id = %bid,
                            total = total,
                            "Batch complete, restoring all files to clipboard"
                        );

                        // Restore all batch files to clipboard
                        let system_clipboard_batch = system_clipboard.clone();
                        let clipboard_origin_batch = clipboard_change_origin.clone();
                        tokio::spawn(async move {
                            restore_file_to_clipboard_after_transfer(
                                all_paths,
                                &system_clipboard_batch,
                                &clipboard_origin_batch,
                            )
                            .await;
                        });
                    }
                }
            }
            NetworkEvent::FileTransferFailed {
                transfer_id,
                peer_id,
                error: error_msg,
            } => {
                warn!(
                    transfer_id = %transfer_id,
                    peer_id = %peer_id,
                    error = %error_msg,
                    "File transfer failed"
                );

                let now_ms = clock.now_ms();
                super::file_transfer_wiring::handle_transfer_failed(
                    transfer_tracker.as_ref(),
                    event_emitter.as_ref(),
                    &transfer_id,
                    &error_msg,
                    now_ms,
                )
                .await;
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wiring_error_display() {
        let err = WiringError::DatabaseInit("connection failed".to_string());
        assert!(err.to_string().contains("Database initialization"));
        assert!(err.to_string().contains("connection failed"));
    }
}
