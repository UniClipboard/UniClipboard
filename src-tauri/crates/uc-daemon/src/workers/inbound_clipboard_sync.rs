//! Inbound clipboard sync worker for the daemon.
//!
//! Subscribes to incoming clipboard messages from peers via ClipboardTransportPort,
//! applies them through SyncInboundClipboardUseCase in Full mode, and broadcasts
//! clipboard.new_content WS events when a new entry is persisted.
//!
//! Write-back loop prevention: the shared `clipboard_change_origin` Arc prevents
//! the daemon's own OS clipboard writes from triggering re-capture via ClipboardWatcher.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::{broadcast, mpsc};
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use uc_app::runtime::CoreRuntime;
use uc_app::usecases::clipboard::clipboard_write_coordinator::ClipboardWriteCoordinator;
use uc_app::usecases::clipboard::sync_inbound::{InboundApplyOutcome, SyncInboundClipboardUseCase};
use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_app::usecases::file_sync::FileTransferOrchestrator;
use uc_core::network::daemon_api_strings::{ws_event, ws_topic};
use uc_core::network::ClipboardMessage;
use uc_core::ports::file_transfer_repository::PendingInboundTransfer;
use uc_infra::clipboard::TransferPayloadDecryptorAdapter;

use crate::api::types::DaemonWsEvent;
use crate::service::{DaemonService, ServiceHealth};

// ---------------------------------------------------------------------------
// ClipboardNewContentPayload
// ---------------------------------------------------------------------------

/// Payload for the clipboard.new_content WS event.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardNewContentPayload {
    entry_id: String,
    preview: String,
    origin: String,
}

// ---------------------------------------------------------------------------
// InboundClipboardSyncWorker
// ---------------------------------------------------------------------------

/// Daemon service that receives inbound clipboard messages from peers.
///
/// Mirrors the `run_clipboard_receive_loop` pattern from wiring.rs, adapted for
/// daemon-mode execution as a `DaemonService`.
///
/// Key behaviors:
/// - Subscribes to `ClipboardTransportPort::subscribe_clipboard()` for incoming messages
/// - Uses `SyncInboundClipboardUseCase::with_capture_dependencies` in Full mode
/// - Emits `clipboard.new_content` WS event only for `Applied { entry_id: Some(_) }`
/// - Does NOT emit WS event for `Applied { entry_id: None }` — ClipboardWatcher handles it
/// - Does NOT emit WS event for `Skipped` outcomes (echo, dedup, encryption not ready)
pub struct InboundClipboardSyncWorker {
    runtime: Arc<CoreRuntime>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    /// Coordinator for Full-mode OS clipboard writes and write-back loop prevention.
    /// MUST wrap the SAME Arc<ClipboardChangeOriginPort> instance used by
    /// DaemonClipboardChangeHandler to share guard state.
    clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    file_cache_dir: Option<PathBuf>,
    file_transfer_orchestrator: Option<Arc<FileTransferOrchestrator>>,
}

impl InboundClipboardSyncWorker {
    /// Create a new InboundClipboardSyncWorker.
    ///
    /// The `clipboard_write_coordinator` MUST wrap the same `ClipboardChangeOriginPort`
    /// instance used by `DaemonClipboardChangeHandler` in the daemon composition root.
    /// Sharing the same origin port instance is what prevents write-back loops between
    /// inbound sync and the ClipboardWatcher.
    pub fn new(
        runtime: Arc<CoreRuntime>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
        file_cache_dir: Option<PathBuf>,
        file_transfer_orchestrator: Option<Arc<FileTransferOrchestrator>>,
    ) -> Self {
        Self {
            runtime,
            event_tx,
            clipboard_write_coordinator,
            file_cache_dir,
            file_transfer_orchestrator,
        }
    }

    fn build_sync_inbound_usecase(&self) -> SyncInboundClipboardUseCase {
        let deps = self.runtime.wiring_deps();
        SyncInboundClipboardUseCase::with_capture_dependencies(
            ClipboardIntegrationMode::Full,
            deps.security.encryption_session.clone(),
            deps.security.encryption.clone(),
            deps.device.device_identity.clone(),
            Arc::new(TransferPayloadDecryptorAdapter),
            deps.clipboard.clipboard_entry_repo.clone(),
            deps.clipboard.clipboard_event_repo.clone(),
            deps.clipboard.representation_policy.clone(),
            deps.clipboard.representation_normalizer.clone(),
            deps.clipboard.representation_cache.clone(),
            deps.clipboard.spool_queue.clone(),
            self.file_cache_dir.clone(),
            deps.settings.clone(),
        )
        .with_clipboard_write_coordinator(self.clipboard_write_coordinator.clone())
    }
}

#[async_trait]
impl DaemonService for InboundClipboardSyncWorker {
    fn name(&self) -> &str {
        "inbound-clipboard-sync"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("inbound clipboard sync starting");
        let usecase = Arc::new(self.build_sync_inbound_usecase());
        let clipboard_network = self.runtime.wiring_deps().network_ports.clipboard.clone();
        let event_tx = self.event_tx.clone();
        let orchestrator = self.file_transfer_orchestrator.clone();

        loop {
            let subscribe_result = tokio::select! {
                _ = cancel.cancelled() => {
                    info!("inbound clipboard sync cancelled");
                    return Ok(());
                }
                result = clipboard_network.subscribe_clipboard() => result,
            };

            match subscribe_result {
                Ok(rx) => {
                    // Run receive loop inline (not spawned) so we block until
                    // the channel closes. subscribe_clipboard() uses take-once
                    // semantics — calling it again after take would always fail
                    // with "clipboard receiver already taken".
                    Self::run_receive_loop(
                        rx,
                        Arc::clone(&usecase),
                        cancel.clone(),
                        event_tx.clone(),
                        orchestrator.clone(),
                    )
                    .await;
                    info!("inbound clipboard receive loop ended, service will exit");
                    return Ok(());
                }
                Err(e) => {
                    warn!(error = %e, "inbound clipboard subscribe failed; retrying in 2s");
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            info!("inbound clipboard sync cancelled during backoff");
                            return Ok(());
                        }
                        _ = sleep(Duration::from_secs(2)) => {}
                    }
                }
            }
        }
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

impl InboundClipboardSyncWorker {
    /// Receive loop: processes messages until the channel closes or cancellation.
    async fn run_receive_loop(
        mut rx: mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>,
        usecase: Arc<SyncInboundClipboardUseCase>,
        cancel: CancellationToken,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        file_transfer_orchestrator: Option<Arc<FileTransferOrchestrator>>,
    ) {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("inbound clipboard receive loop cancelled");
                    break;
                }
                item = rx.recv() => {
                    match item {
                        Some((message, pre_decoded)) => {
                            // Capture origin_device_id before message is consumed by execute_with_outcome.
                            let message_origin_device_id = message.origin_device_id.clone();

                            let outcome = match usecase.execute_with_outcome(message, pre_decoded).await {
                                Ok(o) => o,
                                Err(e) => {
                                    warn!(error = %e, "Failed to apply inbound clipboard message");
                                    continue;
                                }
                            };

                            // Emit WS event ONLY for Applied { entry_id: Some(_) }.
                            // In Full mode with non-file content, entry_id is None and
                            // ClipboardWatcher fires the event — emitting here would cause double events.
                            // In Passive mode or file transfers: entry_id is Some, must emit.
                            if let InboundApplyOutcome::Applied {
                                entry_id: Some(ref entry_id),
                                ref pending_transfers,
                            } = outcome {
                                // Seed pending transfer records for file transfers.
                                if !pending_transfers.is_empty() {
                                    if let Some(ref orch) = file_transfer_orchestrator {
                                        let now_ms = orch.now_ms();
                                        let db_transfers: Vec<PendingInboundTransfer> =
                                            pending_transfers.iter().map(|t| PendingInboundTransfer {
                                                transfer_id: t.transfer_id.clone(),
                                                entry_id: entry_id.to_string(),
                                                origin_device_id: message_origin_device_id.clone(),
                                                filename: t.filename.clone(),
                                                cached_path: t.cached_path.clone(),
                                                created_at_ms: now_ms,
                                            }).collect();

                                        match orch.tracker().record_pending_from_clipboard(db_transfers).await {
                                            Err(err) => {
                                                warn!(error = %err, "Failed to persist pending transfer records");
                                            }
                                            Ok(()) => {
                                                // Reconcile early completions that arrived before seeding.
                                                let seeded_ids: Vec<String> = pending_transfers
                                                    .iter()
                                                    .map(|t| t.transfer_id.clone())
                                                    .collect();
                                                let early = orch.early_completion_cache().drain_matching(&seeded_ids);
                                                for (tid, info) in &early {
                                                    info!(transfer_id = %tid, "Reconciling early completion after seeding");
                                                    if let Err(err) = orch.tracker().mark_completed(
                                                        tid,
                                                        info.content_hash.as_deref(),
                                                        info.completed_at_ms,
                                                    ).await {
                                                        warn!(error = %err, transfer_id = %tid, "Failed to mark early-completed transfer");
                                                    }
                                                }
                                            }
                                        }

                                        // Emit pending status events to frontend.
                                        orch.emit_pending_status(&entry_id.to_string(), pending_transfers);
                                    }
                                }

                                Self::emit_ws_event(&event_tx, entry_id.to_string());
                            }
                            // InboundApplyOutcome::Applied { entry_id: None } — ClipboardWatcher handles it
                            // InboundApplyOutcome::Skipped — nothing to do
                        }
                        None => {
                            info!("inbound clipboard receive channel closed");
                            break;
                        }
                    }
                }
            }
        }
    }

    fn emit_ws_event(event_tx: &broadcast::Sender<DaemonWsEvent>, entry_id: String) {
        let payload = ClipboardNewContentPayload {
            entry_id,
            preview: "Remote clipboard content".to_string(),
            origin: "remote".to_string(),
        };
        let payload_value = match serde_json::to_value(payload) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "Failed to serialize clipboard.new_content payload");
                return;
            }
        };

        let event = DaemonWsEvent {
            topic: ws_topic::CLIPBOARD.to_string(),
            event_type: ws_event::CLIPBOARD_NEW_CONTENT.to_string(),
            session_id: None,
            ts: chrono::Utc::now().timestamp_millis(),
            payload: payload_value,
        };

        if let Err(e) = event_tx.send(event) {
            debug!(error = %e, "No WS subscribers for clipboard.new_content");
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(deprecated)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use anyhow::Result;
    use async_trait::async_trait;
    use chrono::Utc;
    use uc_app::usecases::clipboard::clipboard_write_coordinator::ClipboardWriteCoordinator;
    use uc_app::usecases::clipboard::ClipboardIntegrationMode;
    use uc_core::ids::{EntryId, FormatId, RepresentationId};
    use uc_core::network::protocol::{
        BinaryRepresentation, ClipboardBinaryPayload, ClipboardPayloadVersion,
    };
    use uc_core::network::ClipboardMessage;
    use uc_core::ports::{ClipboardChangeOriginPort, SystemClipboardPort};
    use uc_core::security::model::{EncryptionError, KdfParams, Kek, MasterKey, Passphrase};
    use uc_core::{
        ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
        DeviceId, MimeType, ObservedClipboardRepresentation, PersistedClipboardRepresentation,
        SystemClipboardSnapshot,
    };
    use uc_infra::clipboard::{new_in_memory_change_origin, TransferPayloadDecryptorAdapter};

    fn test_origin() -> Arc<dyn ClipboardChangeOriginPort> {
        new_in_memory_change_origin()
    }

    // -------------------------------------------------------------------------
    // Mock ports for SyncInboundClipboardUseCase construction
    // -------------------------------------------------------------------------

    struct MockSystemClipboard {
        writes: Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
    }

    impl uc_core::ports::SystemClipboardPort for MockSystemClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }
        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
            self.writes.lock().unwrap().push(snapshot);
            Ok(())
        }
    }

    struct MockEncryptionSession;

    #[async_trait]
    impl uc_core::ports::EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            true
        }
        async fn get_master_key(&self) -> std::result::Result<MasterKey, EncryptionError> {
            Ok(MasterKey([3; 32]))
        }
        async fn set_master_key(&self, _: MasterKey) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }
        async fn clear(&self) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct MockEncryption;

    #[async_trait]
    impl uc_core::ports::EncryptionPort for MockEncryption {
        async fn derive_kek(
            &self,
            _: &Passphrase,
            _: &[u8],
            _: &KdfParams,
        ) -> std::result::Result<Kek, EncryptionError> {
            Err(EncryptionError::UnsupportedKdfAlgorithm)
        }
        async fn wrap_master_key(
            &self,
            _: &Kek,
            _: &MasterKey,
            _: uc_core::security::model::EncryptionAlgo,
        ) -> std::result::Result<uc_core::security::model::EncryptedBlob, EncryptionError> {
            Err(EncryptionError::EncryptFailed)
        }
        async fn unwrap_master_key(
            &self,
            _: &Kek,
            _: &uc_core::security::model::EncryptedBlob,
        ) -> std::result::Result<MasterKey, EncryptionError> {
            Err(EncryptionError::WrongPassphrase)
        }
        async fn encrypt_blob(
            &self,
            _: &MasterKey,
            _: &[u8],
            _: &[u8],
            _: uc_core::security::model::EncryptionAlgo,
        ) -> std::result::Result<uc_core::security::model::EncryptedBlob, EncryptionError> {
            Err(EncryptionError::EncryptFailed)
        }
        async fn decrypt_blob(
            &self,
            _: &MasterKey,
            encrypted: &uc_core::security::model::EncryptedBlob,
            _: &[u8],
        ) -> std::result::Result<Vec<u8>, EncryptionError> {
            Ok(encrypted.ciphertext.clone())
        }
    }

    struct MockDeviceIdentity;

    impl uc_core::ports::DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("local-device-id")
        }
    }

    struct MockEntryRepo {
        save_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl uc_core::ports::ClipboardEntryRepositoryPort for MockEntryRepo {
        async fn save_entry_and_selection(
            &self,
            _: &ClipboardEntry,
            _: &ClipboardSelectionDecision,
        ) -> Result<()> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn get_entry(&self, _: &EntryId) -> Result<Option<ClipboardEntry>> {
            Ok(None)
        }
        async fn list_entries(&self, _: usize, _: usize) -> Result<Vec<ClipboardEntry>> {
            Ok(vec![])
        }
        async fn delete_entry(&self, _: &EntryId) -> Result<()> {
            Ok(())
        }
    }

    struct MockEventWriter {
        insert_calls: Arc<AtomicUsize>,
        /// If set, returns Err instead of Ok — used to simulate Skipped outcomes.
        error_on_nth_call: Option<Arc<AtomicUsize>>,
    }

    #[async_trait]
    impl uc_core::ports::ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _: &ClipboardEvent,
            _: &Vec<PersistedClipboardRepresentation>,
        ) -> Result<()> {
            self.insert_calls.fetch_add(1, Ordering::SeqCst);
            if let Some(ref counter) = self.error_on_nth_call {
                let n = counter.fetch_add(1, Ordering::SeqCst);
                if n > 0 {
                    return Err(anyhow::anyhow!("Simulated capture failure for test"));
                }
            }
            Ok(())
        }
        async fn delete_event_and_representations(&self, _: &uc_core::ids::EventId) -> Result<()> {
            Ok(())
        }
    }

    struct MockRepresentationPolicy;

    impl uc_core::ports::SelectRepresentationPolicyPort for MockRepresentationPolicy {
        fn select(
            &self,
            snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<
            uc_core::clipboard::ClipboardSelection,
            uc_core::clipboard::PolicyError,
        > {
            let rep = snapshot
                .representations
                .first()
                .ok_or(uc_core::clipboard::PolicyError::NoUsableRepresentation)?;
            Ok(uc_core::clipboard::ClipboardSelection {
                primary_rep_id: rep.id.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: rep.id.clone(),
                paste_rep_id: rep.id.clone(),
                policy_version: uc_core::clipboard::SelectionPolicyVersion::V1,
            })
        }
    }

    struct MockNormalizer;

    #[async_trait]
    impl uc_core::ports::ClipboardRepresentationNormalizerPort for MockNormalizer {
        async fn normalize(
            &self,
            observed: &ObservedClipboardRepresentation,
        ) -> Result<PersistedClipboardRepresentation> {
            Ok(PersistedClipboardRepresentation::new(
                observed.id.clone(),
                observed.format_id.clone(),
                observed.mime.clone(),
                observed.bytes.len() as i64,
                Some(observed.bytes.clone()),
                None,
            ))
        }
    }

    struct MockRepresentationCache;

    #[async_trait]
    impl uc_core::ports::clipboard::RepresentationCachePort for MockRepresentationCache {
        async fn put(&self, _: &RepresentationId, _: Vec<u8>) {}
        async fn get(&self, _: &RepresentationId) -> Option<Vec<u8>> {
            None
        }
        async fn mark_completed(&self, _: &RepresentationId) {}
        async fn mark_spooling(&self, _: &RepresentationId) {}
        async fn remove(&self, _: &RepresentationId) {}
    }

    struct MockSpoolQueue;

    #[async_trait]
    impl uc_core::ports::clipboard::SpoolQueuePort for MockSpoolQueue {
        async fn enqueue(&self, _: uc_core::ports::clipboard::SpoolRequest) -> Result<()> {
            Ok(())
        }
    }

    struct MockSettings;

    #[async_trait]
    impl uc_core::ports::SettingsPort for MockSettings {
        async fn load(&self) -> Result<uc_core::settings::model::Settings> {
            Ok(uc_core::settings::model::Settings::default())
        }
        async fn save(&self, _: &uc_core::settings::model::Settings) -> Result<()> {
            Ok(())
        }
    }

    // -------------------------------------------------------------------------
    // TestInboundWorker
    // -------------------------------------------------------------------------

    /// Test helper for InboundClipboardSyncWorker tests.
    ///
    /// Mirrors the WS event emission logic of `InboundClipboardSyncWorker` without
    /// requiring a real `CoreRuntime`. Tests call `process_one()` with a pre-built
    /// `SyncInboundClipboardUseCase` and verify WS event emission.
    pub(crate) struct TestInboundWorker {
        event_tx: broadcast::Sender<DaemonWsEvent>,
        usecase: SyncInboundClipboardUseCase,
    }

    impl TestInboundWorker {
        /// Create a new TestInboundWorker.
        pub(crate) fn new(
            event_tx: broadcast::Sender<DaemonWsEvent>,
            usecase: SyncInboundClipboardUseCase,
        ) -> Self {
            Self { event_tx, usecase }
        }

        /// Process one inbound message and return the outcome.
        pub(crate) async fn process_one(
            &self,
            message: ClipboardMessage,
            pre_decoded: Option<Vec<u8>>,
        ) -> InboundApplyOutcome {
            let outcome = self
                .usecase
                .execute_with_outcome(message, pre_decoded)
                .await
                .expect("execute_with_outcome should not fail in tests");

            // Emit WS event only for Applied { entry_id: Some(_) }
            if let InboundApplyOutcome::Applied {
                entry_id: Some(ref entry_id),
                pending_transfers: _,
            } = outcome
            {
                Self::emit_ws_event(&self.event_tx, entry_id.to_string());
            }

            outcome
        }

        fn emit_ws_event(event_tx: &broadcast::Sender<DaemonWsEvent>, entry_id: String) {
            let payload = ClipboardNewContentPayload {
                entry_id,
                preview: "Remote clipboard content".to_string(),
                origin: "remote".to_string(),
            };
            let payload_value = serde_json::to_value(payload).expect("payload must serialize");
            let event = DaemonWsEvent {
                topic: ws_topic::CLIPBOARD.to_string(),
                event_type: ws_event::CLIPBOARD_NEW_CONTENT.to_string(),
                session_id: None,
                ts: chrono::Utc::now().timestamp_millis(),
                payload: payload_value,
            };
            let _ = event_tx.send(event);
        }
    }

    // -------------------------------------------------------------------------
    // Test helpers
    // -------------------------------------------------------------------------

    /// Build a V3 ClipboardMessage with pre-decoded plaintext (transport already decoded).
    fn make_v3_message(
        text: &str,
        origin_device_id: &str,
        message_id: &str,
    ) -> (ClipboardMessage, Vec<u8>) {
        let payload = ClipboardBinaryPayload {
            ts_ms: 1_713_000_000_000,
            representations: vec![BinaryRepresentation {
                format_id: "text".to_string(),
                mime: Some("text/plain".to_string()),
                data: text.as_bytes().to_vec(),
            }],
        };
        let plaintext = payload.encode_to_vec().expect("encode V3 payload");
        let message = ClipboardMessage {
            id: message_id.to_string(),
            content_hash: "test-hash".to_string(),
            encrypted_content: vec![],
            timestamp: Utc::now(),
            origin_device_id: origin_device_id.to_string(),
            origin_device_name: "test-peer".to_string(),
            payload_version: ClipboardPayloadVersion::V3,
            origin_flow_id: None,
            traceparent: None,
            file_transfers: vec![],
        };
        (message, plaintext)
    }

    /// Build a SyncInboundClipboardUseCase for Passive mode tests (returns entry_id: Some).
    fn build_passive_usecase() -> SyncInboundClipboardUseCase {
        SyncInboundClipboardUseCase::with_capture_dependencies(
            ClipboardIntegrationMode::Passive,
            Arc::new(MockEncryptionSession),
            Arc::new(MockEncryption),
            Arc::new(MockDeviceIdentity),
            Arc::new(TransferPayloadDecryptorAdapter),
            Arc::new(MockEntryRepo {
                save_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockEventWriter {
                insert_calls: Arc::new(AtomicUsize::new(0)),
                error_on_nth_call: None,
            }),
            Arc::new(MockRepresentationPolicy),
            Arc::new(MockNormalizer),
            Arc::new(MockRepresentationCache),
            Arc::new(MockSpoolQueue),
            None,
            Arc::new(MockSettings),
        )
    }

    /// Build a SyncInboundClipboardUseCase for Full mode tests (returns entry_id: None for text).
    fn build_full_usecase() -> SyncInboundClipboardUseCase {
        let clipboard: Arc<dyn SystemClipboardPort> = Arc::new(MockSystemClipboard {
            writes: Arc::new(Mutex::new(vec![])),
        });
        let origin: Arc<dyn ClipboardChangeOriginPort> = test_origin();
        let coordinator = Arc::new(ClipboardWriteCoordinator::new(
            clipboard.clone(),
            origin.clone(),
        ));
        SyncInboundClipboardUseCase::with_capture_dependencies(
            ClipboardIntegrationMode::Full,
            Arc::new(MockEncryptionSession),
            Arc::new(MockEncryption),
            Arc::new(MockDeviceIdentity),
            Arc::new(TransferPayloadDecryptorAdapter),
            Arc::new(MockEntryRepo {
                save_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockEventWriter {
                insert_calls: Arc::new(AtomicUsize::new(0)),
                error_on_nth_call: None,
            }),
            Arc::new(MockRepresentationPolicy),
            Arc::new(MockNormalizer),
            Arc::new(MockRepresentationCache),
            Arc::new(MockSpoolQueue),
            None,
            Arc::new(MockSettings),
        )
        .with_clipboard_write_coordinator(coordinator)
    }

    // -------------------------------------------------------------------------
    // PH62-02: Applied outcome with entry_id emits WS event with origin="remote"
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn applied_with_entry_id_emits_ws_event() {
        // Passive mode: returns Applied { entry_id: Some(id) }
        let usecase = build_passive_usecase();
        let (event_tx, mut rx) = broadcast::channel::<DaemonWsEvent>(64);

        let worker = TestInboundWorker::new(event_tx.clone(), usecase);

        let (message, plaintext) = make_v3_message("hello remote", "remote-peer-1", "msg-remote-1");
        let outcome = worker.process_one(message, Some(plaintext)).await;

        // Should be Applied with Some entry_id
        let entry_id = match outcome {
            InboundApplyOutcome::Applied { entry_id, .. } => entry_id,
            InboundApplyOutcome::Skipped => panic!("expected Applied, got Skipped"),
        };
        let entry_id = entry_id.expect("Passive mode should return Some entry_id");

        // Verify WS event was emitted with origin=remote
        let found_event = rx.try_recv().unwrap();
        assert_eq!(found_event.event_type, ws_event::CLIPBOARD_NEW_CONTENT);
        assert_eq!(found_event.topic, ws_topic::CLIPBOARD);
        assert_eq!(
            found_event.payload.get("origin").and_then(|v| v.as_str()),
            Some("remote")
        );
        assert_eq!(
            found_event.payload.get("entryId").and_then(|v| v.as_str()),
            Some(entry_id.to_string().as_str())
        );
    }

    // -------------------------------------------------------------------------
    // PH62-03: Applied outcome without entry_id (Full mode non-file) does NOT emit WS event
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn applied_without_entry_id_does_not_emit_ws_event() {
        // Full mode text: returns Applied { entry_id: None } — ClipboardWatcher handles it
        let usecase = build_full_usecase();
        let (event_tx, mut rx) = broadcast::channel::<DaemonWsEvent>(64);

        let worker = TestInboundWorker::new(event_tx.clone(), usecase);

        let (message, plaintext) =
            make_v3_message("hello full mode", "remote-peer-2", "msg-remote-2");
        let outcome = worker.process_one(message, Some(plaintext)).await;

        // Should be Applied with None entry_id (Full mode non-file)
        match outcome {
            InboundApplyOutcome::Applied { entry_id: None, .. } => {}
            InboundApplyOutcome::Applied {
                entry_id: Some(_), ..
            } => {
                panic!("Full mode non-file should not return Some entry_id")
            }
            InboundApplyOutcome::Skipped => panic!("expected Applied, got Skipped"),
        }

        // Verify NO clipboard.new_content WS event was emitted
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if event.event_type == ws_event::CLIPBOARD_NEW_CONTENT {
                        panic!(
                            "Expected no clipboard.new_content event, but found one: {:?}",
                            event
                        );
                    }
                }
                Err(_) => break, // No more events or channel closed
            }
        }
    }

    // -------------------------------------------------------------------------
    // PH62-04: Skipped outcome does NOT emit WS event
    // -------------------------------------------------------------------------

    #[tokio::test]
    async fn skipped_does_not_emit_ws_event() {
        let usecase = build_passive_usecase();
        let (event_tx, mut rx) = broadcast::channel::<DaemonWsEvent>(64);

        let worker = TestInboundWorker::new(event_tx.clone(), usecase);

        // First message — should be Applied
        let (message, plaintext) = make_v3_message("duplicate", "remote-peer-3", "msg-dup-1");
        let _ = worker
            .process_one(message.clone(), Some(plaintext.clone()))
            .await;

        // Drain the Applied event from the first message (expected)
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if event.event_type == ws_event::CLIPBOARD_NEW_CONTENT {
                        // Found and consumed the Applied event
                        break;
                    }
                    // Other events, keep draining
                }
                Err(_) => break, // No more events
            }
        }

        // Second message with same ID — should be Skipped (dedup by message_id)
        let outcome = worker.process_one(message, Some(plaintext)).await;

        match outcome {
            InboundApplyOutcome::Skipped => {}
            _ => panic!("expected Skipped (dedup), got {:?}", outcome),
        }

        // Verify NO clipboard.new_content WS event was emitted for the duplicate
        loop {
            match rx.try_recv() {
                Ok(event) => {
                    if event.event_type == ws_event::CLIPBOARD_NEW_CONTENT {
                        panic!(
                            "Expected no clipboard.new_content event for Skipped outcome, but found one: {:?}",
                            event
                        );
                    }
                    // Other events, keep draining
                }
                Err(_) => break, // No more events
            }
        }
    }

    // -------------------------------------------------------------------------
    // PH73-02: coordinator field type enforced by constructor signature
    // -------------------------------------------------------------------------

    #[test]
    fn constructor_requires_clipboard_write_coordinator_arc() {
        // Compile-time verification that the constructor requires Arc<ClipboardWriteCoordinator>.
        // If the field type changes this test will fail to compile.
        fn _assert_type_matches(worker: &InboundClipboardSyncWorker) {
            let _: &Arc<ClipboardWriteCoordinator> = &worker.clipboard_write_coordinator;
        }
    }
}
