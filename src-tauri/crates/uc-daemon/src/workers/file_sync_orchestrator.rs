//! File sync orchestrator worker for the daemon.
//!
//! Subscribes to NetworkEventPort for file transfer lifecycle events
//! (TransferProgress, FileTransferCompleted, FileTransferFailed),
//! delegates to FileTransferOrchestrator for durable state tracking,
//! and restores completed files to the OS clipboard.
//!
//! Also runs startup reconciliation (orphaned in-flight → failed) and
//! periodic timeout sweeps (stalled pending/transferring → failed).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, info_span, instrument, warn, Instrument};

use uc_app::usecases::clipboard::clipboard_write_coordinator::{
    ClipboardWriteCoordinator, ClipboardWriteIntent,
};
use uc_app::usecases::file_sync::FileTransferOrchestrator;
use uc_app::usecases::file_sync::SyncInboundFileUseCase;
use uc_core::network::NetworkEvent;
use uc_core::ports::{NetworkEventPort, SettingsPort};

use crate::service::{DaemonService, ServiceHealth};

pub struct FileSyncOrchestratorWorker {
    orchestrator: Arc<FileTransferOrchestrator>,
    network_events: Arc<dyn NetworkEventPort>,
    clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    file_cache_dir: PathBuf,
    settings: Arc<dyn SettingsPort>,
}

impl FileSyncOrchestratorWorker {
    pub fn new(
        orchestrator: Arc<FileTransferOrchestrator>,
        network_events: Arc<dyn NetworkEventPort>,
        clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
        file_cache_dir: PathBuf,
        settings: Arc<dyn SettingsPort>,
    ) -> Self {
        Self {
            orchestrator,
            network_events,
            clipboard_write_coordinator,
            file_cache_dir,
            settings,
        }
    }
}

#[async_trait]
impl DaemonService for FileSyncOrchestratorWorker {
    fn name(&self) -> &str {
        "file-sync-orchestrator"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("file sync orchestrator starting");

        // 1. Run startup reconciliation (orphaned in-flight transfers → failed)
        self.orchestrator.reconcile_on_startup().await;

        // 2. Start timeout sweep (15s interval, cancellable via watch channel)
        let (sweep_cancel_tx, sweep_cancel_rx) = tokio::sync::watch::channel(false);
        let _sweep_handle = self.orchestrator.spawn_timeout_sweep(sweep_cancel_rx);

        // 3. Subscribe to network events
        let mut event_rx = match self.network_events.subscribe_events().await {
            Ok(rx) => rx,
            Err(err) => {
                let _ = sweep_cancel_tx.send(true);
                return Err(err);
            }
        };

        info!("file sync orchestrator subscribed to network events");

        // 4. Batch accumulator: batch_id -> (completed_paths, expected_total, peer_id)
        let mut batch_accumulator: HashMap<String, (Vec<PathBuf>, u32, String)> = HashMap::new();

        // 5. Event loop
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = sweep_cancel_tx.send(true);
                    info!("file sync orchestrator cancelled");
                    return Ok(());
                }
                maybe_event = event_rx.recv() => {
                    let Some(event) = maybe_event else {
                        warn!("network event channel closed");
                        let _ = sweep_cancel_tx.send(true);
                        return Ok(());
                    };
                    self.handle_network_event(event, &mut batch_accumulator).await;
                }
            }
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!("file sync orchestrator stopped");
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

impl FileSyncOrchestratorWorker {
    async fn handle_network_event(
        &self,
        event: NetworkEvent,
        batch_accumulator: &mut HashMap<String, (Vec<PathBuf>, u32, String)>,
    ) {
        match event {
            NetworkEvent::TransferProgress(progress) => {
                // Track durable status transitions (pending->transferring, liveness refresh)
                self.orchestrator
                    .handle_transfer_progress(
                        &progress.transfer_id,
                        progress.direction.clone(),
                        progress.chunks_completed,
                    )
                    .await;
                // Note: transient progress events are NOT forwarded to WS here;
                // the orchestrator's emitter_cell handles StatusChanged events.
                // Phase 64 will add WS-based progress forwarding if needed.
            }
            NetworkEvent::FileTransferCompleted {
                transfer_id,
                peer_id,
                filename,
                file_path,
                batch_id,
                batch_total,
            } => {
                debug!(
                    transfer_id = %transfer_id,
                    peer_id = %peer_id,
                    filename = %filename,
                    file_path = %file_path.display(),
                    batch_id = ?batch_id,
                    batch_total = ?batch_total,
                    "File transfer completed, processing inbound file"
                );

                let inbound_uc =
                    SyncInboundFileUseCase::new(self.settings.clone(), self.file_cache_dir.clone());

                let orch = self.orchestrator.clone();
                let coordinator = self.clipboard_write_coordinator.clone();
                let is_batch = batch_id.is_some() && batch_total.is_some();
                let span_tid = transfer_id.clone();
                let file_path_for_spawn = file_path.clone();
                let transfer_id_for_spawn = transfer_id.clone();

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
                                orch.handle_transfer_failed(
                                    &transfer_id_for_spawn,
                                    &format!("Failed to read file: {}", err),
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

                                // Mark durable completion
                                orch.handle_transfer_completed(
                                    &result.transfer_id,
                                    Some(&expected_hash),
                                )
                                .await;

                                // Restore single file to clipboard only if NOT part of a batch
                                if !is_batch {
                                    restore_file_to_clipboard_after_transfer(
                                        vec![result.file_path],
                                        &coordinator,
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
                                orch.handle_transfer_failed(
                                    &transfer_id_for_spawn,
                                    &format!("Inbound file sync failed: {}", err),
                                )
                                .await;
                            }
                        }
                    }
                    .instrument(info_span!("inbound_file_sync", transfer_id = %span_tid)),
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

                        let coordinator_batch = self.clipboard_write_coordinator.clone();
                        tokio::spawn(async move {
                            restore_file_to_clipboard_after_transfer(all_paths, &coordinator_batch)
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
                self.orchestrator
                    .handle_transfer_failed(&transfer_id, &error_msg)
                    .await;
            }
            // All other network events (PeerDiscovered, PeerLost, PeerReady, etc.)
            // are handled by PeerDiscoveryWorker and PeerMonitor
            _ => {}
        }
    }
}

/// Restore file(s) to OS clipboard after successful inbound transfer.
///
/// Canonicalizes paths to absolute paths, then delegates guard-registration + write
/// to the ClipboardWriteCoordinator with LocalRestore intent.
#[instrument(
    name = "inbound_file_sync.restore_to_clipboard",
    level = "info",
    skip(file_paths, coordinator),
    fields(file_count = file_paths.len())
)]
async fn restore_file_to_clipboard_after_transfer(
    file_paths: Vec<PathBuf>,
    coordinator: &Arc<ClipboardWriteCoordinator>,
) {
    use uc_app::usecases::file_sync::copy_file_to_clipboard::{
        build_file_snapshot, build_path_list,
    };

    // Canonicalize paths to absolute paths.
    // The clipboard (CF_HDROP on Windows, NSPasteboard on macOS) requires absolute
    // paths; relative paths won't resolve when pasting.
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
    if !all_exist {
        warn!(
            paths = ?file_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            files_exist = ?files_exist,
            "Some files do not exist on disk — clipboard write will likely fail"
        );
    }

    let path_list = build_path_list(&file_paths);
    let snapshot = build_file_snapshot(&path_list);

    // FCLIP-03: Check for genuinely concurrent clipboard write operations.
    // Uses is_write_in_progress() which only returns true while another write()
    // call is actively executing — not merely because attribution guards from
    // a previous completed write are still within their TTL window.
    if coordinator.is_write_in_progress() {
        warn!(
            file_count = file_paths.len(),
            "Concurrent clipboard write in progress, skipping auto-restore. Files available in Dashboard."
        );
        return;
    }

    // Restore to system clipboard via coordinator (handles guard + write + cleanup-on-error)
    if let Err(err) = coordinator
        .write(snapshot, ClipboardWriteIntent::LocalRestore)
        .await
    {
        warn!(error = %err, "Failed to write file URIs to system clipboard");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use tokio::sync::mpsc;
    use tokio::time::{timeout, Duration};
    use uc_core::network::NetworkEvent;
    use uc_core::ports::transfer_progress::{TransferDirection, TransferProgress};
    use uc_infra::clipboard::new_in_memory_change_origin;

    fn test_origin() -> Arc<dyn uc_core::ports::clipboard::ClipboardChangeOriginPort> {
        new_in_memory_change_origin()
    }

    struct MockNetworkEvents {
        rx: Mutex<Option<mpsc::Receiver<NetworkEvent>>>,
    }

    #[async_trait]
    impl NetworkEventPort for MockNetworkEvents {
        async fn subscribe_events(&self) -> anyhow::Result<mpsc::Receiver<NetworkEvent>> {
            self.rx
                .lock()
                .unwrap()
                .take()
                .ok_or_else(|| anyhow::anyhow!("receiver already taken"))
        }
    }

    struct MockSystemClipboard;

    impl uc_core::ports::SystemClipboardPort for MockSystemClipboard {
        fn read_snapshot(&self) -> anyhow::Result<uc_core::clipboard::SystemClipboardSnapshot> {
            Ok(uc_core::clipboard::SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(
            &self,
            _snapshot: uc_core::clipboard::SystemClipboardSnapshot,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockSettings;

    #[async_trait]
    impl uc_core::ports::SettingsPort for MockSettings {
        async fn load(&self) -> anyhow::Result<uc_core::settings::model::Settings> {
            Ok(uc_core::settings::model::Settings::default())
        }

        async fn save(&self, _settings: &uc_core::settings::model::Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    /// Build a test ClipboardWriteCoordinator with no-op ports.
    fn build_test_coordinator() -> Arc<ClipboardWriteCoordinator> {
        Arc::new(ClipboardWriteCoordinator::new(
            Arc::new(MockSystemClipboard),
            test_origin(),
        ))
    }

    /// Build a minimal FileTransferOrchestrator backed by a NoopFileTransferRepository.
    fn build_test_orchestrator() -> Arc<FileTransferOrchestrator> {
        use std::sync::RwLock;
        use uc_app::usecases::file_sync::TrackInboundTransfersUseCase;
        use uc_bootstrap::non_gui_runtime::LoggingHostEventEmitter;
        use uc_core::ports::file_transfer_repository::NoopFileTransferRepositoryPort;
        use uc_core::ports::ClockPort;

        struct SystemClock;
        impl ClockPort for SystemClock {
            fn now_ms(&self) -> i64 {
                chrono::Utc::now().timestamp_millis()
            }
        }

        let repo = Arc::new(NoopFileTransferRepositoryPort);
        let tracker = Arc::new(TrackInboundTransfersUseCase::new(repo));
        let emitter: Arc<dyn uc_core::ports::host_event_emitter::HostEventEmitterPort> =
            Arc::new(LoggingHostEventEmitter);
        let emitter_cell = Arc::new(RwLock::new(emitter));
        let clock: Arc<dyn ClockPort> = Arc::new(SystemClock);

        Arc::new(FileTransferOrchestrator::new(tracker, emitter_cell, clock))
    }

    #[tokio::test]
    async fn handles_transfer_failed_event() {
        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let worker = FileSyncOrchestratorWorker::new(
            build_test_orchestrator(),
            Arc::new(MockNetworkEvents {
                rx: Mutex::new(Some(rx)),
            }),
            build_test_coordinator(),
            std::env::temp_dir(),
            Arc::new(MockSettings),
        );

        let worker_cancel = cancel.clone();
        let task = tokio::spawn(async move { worker.start(worker_cancel).await });

        // Send a FileTransferFailed event and then cancel
        tx.send(NetworkEvent::FileTransferFailed {
            transfer_id: "xfer-fail-1".to_string(),
            peer_id: "peer-1".to_string(),
            error: "simulated error".to_string(),
        })
        .await
        .unwrap();

        // Give the worker time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        cancel.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .expect("worker should stop within timeout")
            .expect("worker task should not panic")
            .expect("worker start should return Ok");
    }

    #[tokio::test]
    async fn handles_transfer_progress_event() {
        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let worker = FileSyncOrchestratorWorker::new(
            build_test_orchestrator(),
            Arc::new(MockNetworkEvents {
                rx: Mutex::new(Some(rx)),
            }),
            build_test_coordinator(),
            std::env::temp_dir(),
            Arc::new(MockSettings),
        );

        let worker_cancel = cancel.clone();
        let task = tokio::spawn(async move { worker.start(worker_cancel).await });

        // Send a TransferProgress event
        tx.send(NetworkEvent::TransferProgress(TransferProgress {
            transfer_id: "xfer-prog-1".to_string(),
            peer_id: "peer-1".to_string(),
            direction: TransferDirection::Receiving,
            chunks_completed: 1,
            total_chunks: 10,
            bytes_transferred: 1024,
            total_bytes: Some(10240),
        }))
        .await
        .unwrap();

        // Give the worker time to process
        tokio::time::sleep(Duration::from_millis(50)).await;

        cancel.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .expect("worker should stop within timeout")
            .expect("worker task should not panic")
            .expect("worker start should return Ok");
    }

    #[tokio::test]
    async fn ignores_peer_discovered_event() {
        let (tx, rx) = mpsc::channel(8);
        let cancel = CancellationToken::new();

        let worker = FileSyncOrchestratorWorker::new(
            build_test_orchestrator(),
            Arc::new(MockNetworkEvents {
                rx: Mutex::new(Some(rx)),
            }),
            build_test_coordinator(),
            std::env::temp_dir(),
            Arc::new(MockSettings),
        );

        let worker_cancel = cancel.clone();
        let task = tokio::spawn(async move { worker.start(worker_cancel).await });

        // Send a PeerDiscovered event — should be silently ignored
        tx.send(NetworkEvent::PeerDiscovered(
            uc_core::network::DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: None,
                device_id: None,
                addresses: vec![],
                discovered_at: chrono::Utc::now(),
                last_seen: chrono::Utc::now(),
                is_paired: false,
            },
        ))
        .await
        .unwrap();

        // Give the worker time to process (should not panic or fail)
        tokio::time::sleep(Duration::from_millis(50)).await;

        cancel.cancel();
        timeout(Duration::from_secs(2), task)
            .await
            .expect("worker should stop within timeout")
            .expect("worker task should not panic")
            .expect("worker start should return Ok");
    }
}
