use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::usecases::clipboard::ClipboardIntegrationMode;
use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing::{debug, info, info_span, warn, Instrument};
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::network::protocol::ClipboardTextPayloadV1;
use uc_core::network::ClipboardMessage;
use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort};
use uc_core::ports::{
    ClipboardChangeOriginPort, ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
    ClipboardRepresentationNormalizerPort, DeviceIdentityPort, EncryptionPort,
    EncryptionSessionPort, SelectRepresentationPolicyPort, SystemClipboardPort,
};
use uc_core::security::{aad, model::EncryptedBlob};
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

const RECENT_ID_TTL: Duration = Duration::from_secs(600);
const RECENT_ID_MAX: usize = 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundApplyOutcome {
    Applied { entry_id: Option<EntryId> },
    Skipped,
}

pub struct SyncInboundClipboardUseCase {
    mode: ClipboardIntegrationMode,
    local_clipboard: Arc<dyn SystemClipboardPort>,
    clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
    encryption_session: Arc<dyn EncryptionSessionPort>,
    encryption: Arc<dyn EncryptionPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    capture_clipboard:
        Option<crate::usecases::internal::capture_clipboard::CaptureClipboardUseCase>,
    recent_ids: Mutex<VecDeque<(String, Instant)>>,
}

impl SyncInboundClipboardUseCase {
    pub fn new(
        mode: ClipboardIntegrationMode,
        local_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        encryption: Arc<dyn EncryptionPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
    ) -> Result<Self> {
        if mode == ClipboardIntegrationMode::Passive {
            return Err(anyhow::anyhow!(
                "invalid inbound sync configuration: Passive mode requires capture dependencies; use with_capture_dependencies"
            ));
        }

        Ok(Self {
            mode,
            local_clipboard,
            clipboard_change_origin,
            encryption_session,
            encryption,
            device_identity,
            capture_clipboard: None,
            recent_ids: Mutex::new(VecDeque::new()),
        })
    }

    pub fn with_capture_dependencies(
        mode: ClipboardIntegrationMode,
        local_clipboard: Arc<dyn SystemClipboardPort>,
        clipboard_change_origin: Arc<dyn ClipboardChangeOriginPort>,
        encryption_session: Arc<dyn EncryptionSessionPort>,
        encryption: Arc<dyn EncryptionPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
        representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
        representation_cache: Arc<dyn RepresentationCachePort>,
        spool_queue: Arc<dyn SpoolQueuePort>,
    ) -> Self {
        Self {
            mode,
            local_clipboard,
            clipboard_change_origin,
            encryption_session,
            encryption,
            device_identity: device_identity.clone(),
            capture_clipboard: Some(
                crate::usecases::internal::capture_clipboard::CaptureClipboardUseCase::new(
                    entry_repo,
                    event_writer,
                    representation_policy,
                    representation_normalizer,
                    device_identity,
                    representation_cache,
                    spool_queue,
                ),
            ),
            recent_ids: Mutex::new(VecDeque::new()),
        }
    }

    pub async fn execute(&self, message: ClipboardMessage) -> Result<()> {
        self.execute_with_outcome(message).await.map(|_| ())
    }

    pub fn mode(&self) -> ClipboardIntegrationMode {
        self.mode
    }

    async fn prune_recent_ids(&self) {
        let now = Instant::now();
        let mut recent_ids = self.recent_ids.lock().await;
        while let Some((_id, ts)) = recent_ids.front() {
            if now.duration_since(*ts) > RECENT_ID_TTL {
                recent_ids.pop_front();
            } else {
                break;
            }
        }
    }

    async fn rollback_recent_id(&self, message_id: &str) {
        self.prune_recent_ids().await;
        let mut recent_ids = self.recent_ids.lock().await;
        if let Some(index) = recent_ids.iter().position(|(id, _)| id == message_id) {
            recent_ids.remove(index);
        }
        while recent_ids.len() > RECENT_ID_MAX {
            recent_ids.pop_front();
        }
    }

    pub async fn execute_with_outcome(
        &self,
        message: ClipboardMessage,
    ) -> Result<InboundApplyOutcome> {
        let span = info_span!(
            "usecase.clipboard.sync_inbound.execute",
            message_id = %message.id,
            origin_device_id = %message.origin_device_id,
        );

        async move {
            let local_device_id = self.device_identity.current_device_id().to_string();
            if message.origin_device_id == local_device_id {
                debug!("Ignoring inbound clipboard message from local device");
                return Ok(InboundApplyOutcome::Skipped);
            }

            if !self.encryption_session.is_ready().await {
                info!("Skipping inbound apply because encryption session is not ready");
                return Ok(InboundApplyOutcome::Skipped);
            }

            let encrypted_blob: EncryptedBlob = serde_json::from_slice(&message.encrypted_content)
                .context("failed to deserialize encrypted inbound clipboard payload")?;

            let master_key = self
                .encryption_session
                .get_master_key()
                .await
                .map_err(anyhow::Error::from)
                .context("failed to access encryption session master key for inbound apply")?;

            let plaintext = self
                .encryption
                .decrypt_blob(
                    &master_key,
                    &encrypted_blob,
                    &aad::for_network_clipboard(&message.id),
                )
                .await
                .map_err(anyhow::Error::from)
                .context("failed to decrypt inbound clipboard payload")?;

            let payload: ClipboardTextPayloadV1 = serde_json::from_slice(&plaintext)
                .context("failed to deserialize inbound clipboard payload")?;
            if !is_text_plain_mime(&payload.mime) {
                warn!(mime = %payload.mime, "Skipping inbound apply because payload mime is not text/plain");
                return Ok(InboundApplyOutcome::Skipped);
            }

            let snapshot = SystemClipboardSnapshot {
                ts_ms: payload.ts_ms,
                representations: vec![ObservedClipboardRepresentation {
                    id: RepresentationId::new(),
                    format_id: FormatId::from("text"),
                    mime: Some(MimeType::text_plain()),
                    bytes: payload.text.into_bytes(),
                }],
            };

            if !self.mode.allow_os_read() {
                let message_id = message.id.clone();
                self.prune_recent_ids().await;
                {
                    let now = Instant::now();
                    let mut recent_ids = self.recent_ids.lock().await;
                    let is_duplicate = recent_ids.iter().any(|(id, _)| id == &message_id);
                    if is_duplicate {
                        debug!(message_id = %message_id, "Skipping inbound apply because passive mode already processed this message id");
                        return Ok(InboundApplyOutcome::Skipped);
                    }
                    recent_ids.push_back((message_id.clone(), now));
                    while recent_ids.len() > RECENT_ID_MAX {
                        recent_ids.pop_front();
                    }
                }

                let capture = self
                    .capture_clipboard
                    .as_ref()
                    .context("passive inbound sync requires capture clipboard dependencies")?;
                let persisted_entry_id = match capture
                    .execute_with_origin(snapshot, ClipboardChangeOrigin::RemotePush)
                    .await
                {
                    Ok(Some(entry_id)) => entry_id,
                    Ok(None) => {
                        self.rollback_recent_id(&message_id).await;
                        return Err(anyhow::anyhow!(
                            "capture usecase skipped persistence for RemotePush origin"
                        ))
                        .context("failed to persist inbound clipboard in passive mode");
                    }
                    Err(err) => {
                        self.rollback_recent_id(&message_id).await;
                        return Err(err).context("failed to persist inbound clipboard in passive mode");
                    }
                };

                self.prune_recent_ids().await;

                info!(mode = ?self.mode, "Inbound clipboard message persisted in passive mode");
                return Ok(InboundApplyOutcome::Applied {
                    entry_id: Some(persisted_entry_id),
                });
            }

            if !self.mode.allow_os_write() {
                info!(mode = ?self.mode, "Skipping inbound apply because clipboard integration mode disallows OS clipboard write");
                return Ok(InboundApplyOutcome::Skipped);
            }

            let current_snapshot = self
                .local_clipboard
                .read_snapshot()
                .context("failed to read local clipboard snapshot for inbound dedupe")?;
            let already_applied = current_snapshot
                .representations
                .iter()
                .any(|rep| rep.content_hash().to_string() == message.content_hash);
            if already_applied {
                debug!("Skipping inbound apply because local clipboard already matches content hash");
                return Ok(InboundApplyOutcome::Skipped);
            }

            self.clipboard_change_origin
                .set_next_origin(
                    ClipboardChangeOrigin::RemotePush,
                    Duration::from_millis(REMOTE_PUSH_ORIGIN_TTL_MS),
                )
                .await;

            if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
                self.clipboard_change_origin
                    .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
                    .await;
                return Err(err).context("failed to write inbound clipboard snapshot");
            }

            info!("Inbound clipboard message applied");
            Ok(InboundApplyOutcome::Applied { entry_id: None })
        }
        .instrument(span)
        .await
    }
}

const REMOTE_PUSH_ORIGIN_TTL_MS: u64 = 100;

fn is_text_plain_mime(mime: &str) -> bool {
    let normalized = mime.trim();
    normalized.eq_ignore_ascii_case(ClipboardTextPayloadV1::MIME_TEXT_PLAIN)
        || normalized.to_ascii_lowercase().starts_with("text/plain;")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use uc_core::clipboard::{ClipboardSelection, PolicyError, SelectionPolicyVersion};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::network::protocol::ClipboardTextPayloadV1;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfParams, Kek,
        MasterKey, Passphrase,
    };
    use uc_core::{
        ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
        DeviceId, MimeType, ObservedClipboardRepresentation, PersistedClipboardRepresentation,
        SystemClipboardSnapshot,
    };

    struct MockSystemClipboard {
        reads: SystemClipboardSnapshot,
        writes: Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl SystemClipboardPort for MockSystemClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            self.calls.lock().expect("calls lock").push("read_snapshot");
            Ok(self.reads.clone())
        }

        fn write_snapshot(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
            self.calls
                .lock()
                .expect("calls lock")
                .push("write_snapshot");
            self.writes.lock().expect("writes lock").push(snapshot);
            Ok(())
        }
    }

    struct MockChangeOrigin {
        calls: Arc<Mutex<Vec<&'static str>>>,
        values: Arc<Mutex<Vec<(ClipboardChangeOrigin, Duration)>>>,
    }

    #[async_trait]
    impl ClipboardChangeOriginPort for MockChangeOrigin {
        async fn set_next_origin(&self, origin: ClipboardChangeOrigin, ttl: Duration) {
            self.calls.lock().expect("calls lock").push("set_origin");
            self.values.lock().expect("values lock").push((origin, ttl));
        }

        async fn consume_origin_or_default(
            &self,
            default_origin: ClipboardChangeOrigin,
        ) -> ClipboardChangeOrigin {
            default_origin
        }
    }

    struct MockEncryptionSession {
        ready: bool,
    }

    #[async_trait]
    impl EncryptionSessionPort for MockEncryptionSession {
        async fn is_ready(&self) -> bool {
            self.ready
        }

        async fn get_master_key(&self) -> std::result::Result<MasterKey, EncryptionError> {
            Ok(MasterKey([3; 32]))
        }

        async fn set_master_key(
            &self,
            _master_key: MasterKey,
        ) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> std::result::Result<(), EncryptionError> {
            Ok(())
        }
    }

    struct MockEncryption {
        decrypt_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl EncryptionPort for MockEncryption {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf: &KdfParams,
        ) -> std::result::Result<Kek, EncryptionError> {
            Err(EncryptionError::UnsupportedKdfAlgorithm)
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> std::result::Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::EncryptFailed)
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _wrapped: &EncryptedBlob,
        ) -> std::result::Result<MasterKey, EncryptionError> {
            Err(EncryptionError::WrongPassphrase)
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _aead: EncryptionAlgo,
        ) -> std::result::Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::EncryptFailed)
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            encrypted: &EncryptedBlob,
            _aad: &[u8],
        ) -> std::result::Result<Vec<u8>, EncryptionError> {
            self.decrypt_calls.fetch_add(1, Ordering::SeqCst);
            Ok(encrypted.ciphertext.clone())
        }
    }

    struct MockDeviceIdentity {
        id: DeviceId,
    }

    impl DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            self.id.clone()
        }
    }

    struct MockEntryRepository {
        save_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl uc_core::ports::ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> Result<()> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn get_entry(
            &self,
            _entry_id: &uc_core::ids::EntryId,
        ) -> Result<Option<ClipboardEntry>> {
            Ok(None)
        }

        async fn list_entries(&self, _limit: usize, _offset: usize) -> Result<Vec<ClipboardEntry>> {
            Ok(Vec::new())
        }

        async fn delete_entry(&self, _entry_id: &uc_core::ids::EntryId) -> Result<()> {
            Ok(())
        }
    }

    struct MockEventWriter {
        insert_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl uc_core::ports::ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _event: &ClipboardEvent,
            _representations: &Vec<PersistedClipboardRepresentation>,
        ) -> Result<()> {
            self.insert_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn delete_event_and_representations(
            &self,
            _event_id: &uc_core::ids::EventId,
        ) -> Result<()> {
            Ok(())
        }
    }

    struct MockRepresentationPolicy;

    impl uc_core::ports::SelectRepresentationPolicyPort for MockRepresentationPolicy {
        fn select(
            &self,
            snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<ClipboardSelection, PolicyError> {
            let rep = snapshot
                .representations
                .first()
                .ok_or(PolicyError::NoUsableRepresentation)?;
            Ok(ClipboardSelection {
                primary_rep_id: rep.id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: rep.id.clone(),
                paste_rep_id: rep.id.clone(),
                policy_version: SelectionPolicyVersion::V1,
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
        async fn put(&self, _rep_id: &RepresentationId, _bytes: Vec<u8>) {}

        async fn get(&self, _rep_id: &RepresentationId) -> Option<Vec<u8>> {
            None
        }

        async fn mark_completed(&self, _rep_id: &RepresentationId) {}

        async fn mark_spooling(&self, _rep_id: &RepresentationId) {}

        async fn remove(&self, _rep_id: &RepresentationId) {}
    }

    struct MockSpoolQueue;

    #[async_trait]
    impl uc_core::ports::clipboard::SpoolQueuePort for MockSpoolQueue {
        async fn enqueue(&self, _request: uc_core::ports::clipboard::SpoolRequest) -> Result<()> {
            Ok(())
        }
    }

    fn build_text_snapshot(text: &str) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![ObservedClipboardRepresentation {
                id: RepresentationId::new(),
                format_id: FormatId::from("text"),
                mime: Some(MimeType::text_plain()),
                bytes: text.as_bytes().to_vec(),
            }],
        }
    }

    fn build_message(text: &str, origin_device_id: &str) -> ClipboardMessage {
        let payload = ClipboardTextPayloadV1::new(text.to_string(), 42);
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let encrypted_blob = EncryptedBlob {
            version: EncryptionFormatVersion::V1,
            aead: EncryptionAlgo::XChaCha20Poly1305,
            nonce: vec![9; 24],
            ciphertext: payload_bytes,
            aad_fingerprint: None,
        };
        let encrypted_content = serde_json::to_vec(&encrypted_blob).expect("serialize blob");
        let content_hash = build_text_snapshot(text).representations[0]
            .content_hash()
            .to_string();

        ClipboardMessage {
            id: "msg-1".to_string(),
            content_hash,
            encrypted_content,
            timestamp: Utc::now(),
            origin_device_id: origin_device_id.to_string(),
            origin_device_name: "peer-device".to_string(),
        }
    }

    fn build_usecase(
        mode: ClipboardIntegrationMode,
        local_snapshot: SystemClipboardSnapshot,
        local_device_id: &str,
        ready: bool,
    ) -> (
        SyncInboundClipboardUseCase,
        Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
        Arc<Mutex<Vec<&'static str>>>,
        Arc<Mutex<Vec<(ClipboardChangeOrigin, Duration)>>>,
        Arc<AtomicUsize>,
    ) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let origin_values = Arc::new(Mutex::new(Vec::new()));
        let decrypt_calls = Arc::new(AtomicUsize::new(0));

        let usecase = SyncInboundClipboardUseCase::new(
            mode,
            Arc::new(MockSystemClipboard {
                reads: local_snapshot,
                writes: writes.clone(),
                calls: calls.clone(),
            }),
            Arc::new(MockChangeOrigin {
                calls: calls.clone(),
                values: origin_values.clone(),
            }),
            Arc::new(MockEncryptionSession { ready }),
            Arc::new(MockEncryption {
                decrypt_calls: decrypt_calls.clone(),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new(local_device_id),
            }),
        )
        .expect("build inbound usecase");

        (usecase, writes, calls, origin_values, decrypt_calls)
    }

    fn build_passive_usecase(
        local_snapshot: SystemClipboardSnapshot,
        local_device_id: &str,
    ) -> (
        SyncInboundClipboardUseCase,
        Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
        Arc<Mutex<Vec<&'static str>>>,
        Arc<AtomicUsize>,
        Arc<AtomicUsize>,
    ) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let save_calls = Arc::new(AtomicUsize::new(0));
        let insert_calls = Arc::new(AtomicUsize::new(0));

        let usecase = SyncInboundClipboardUseCase::with_capture_dependencies(
            ClipboardIntegrationMode::Passive,
            Arc::new(MockSystemClipboard {
                reads: local_snapshot,
                writes: writes.clone(),
                calls: calls.clone(),
            }),
            Arc::new(MockChangeOrigin {
                calls: calls.clone(),
                values: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(MockEncryptionSession { ready: true }),
            Arc::new(MockEncryption {
                decrypt_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new(local_device_id),
            }),
            Arc::new(MockEntryRepository {
                save_calls: save_calls.clone(),
            }),
            Arc::new(MockEventWriter {
                insert_calls: insert_calls.clone(),
            }),
            Arc::new(MockRepresentationPolicy),
            Arc::new(MockNormalizer),
            Arc::new(MockRepresentationCache),
            Arc::new(MockSpoolQueue),
        );

        (usecase, writes, calls, save_calls, insert_calls)
    }

    #[tokio::test]
    async fn valid_inbound_message_applies_exactly_one_text_plain_snapshot() {
        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        usecase
            .execute(build_message("hello inbound", "remote-1"))
            .await
            .expect("execute inbound message");

        let writes = writes.lock().expect("writes lock");
        assert_eq!(writes.len(), 1);
        assert_eq!(writes[0].representations.len(), 1);
        assert_eq!(
            writes[0].representations[0].mime,
            Some(MimeType::text_plain())
        );
        assert_eq!(writes[0].representations[0].bytes, b"hello inbound");
    }

    #[test]
    fn new_rejects_passive_mode_without_capture_dependencies() {
        let result = SyncInboundClipboardUseCase::new(
            ClipboardIntegrationMode::Passive,
            Arc::new(MockSystemClipboard {
                reads: SystemClipboardSnapshot {
                    ts_ms: 0,
                    representations: vec![],
                },
                writes: Arc::new(Mutex::new(Vec::new())),
                calls: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(MockChangeOrigin {
                calls: Arc::new(Mutex::new(Vec::new())),
                values: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(MockEncryptionSession { ready: true }),
            Arc::new(MockEncryption {
                decrypt_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new("local-1"),
            }),
        );

        match result {
            Ok(_) => panic!("expected passive mode configuration error"),
            Err(err) => {
                assert!(
                    err.to_string()
                        .contains("Passive mode requires capture dependencies"),
                    "unexpected error: {err}"
                );
            }
        }
    }

    #[tokio::test]
    async fn sets_origin_to_remote_push_before_write() {
        let (usecase, _, calls, origin_values, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        usecase
            .execute(build_message("hello order", "remote-1"))
            .await
            .expect("execute inbound message");

        assert_eq!(
            calls.lock().expect("calls lock").as_slice(),
            ["read_snapshot", "set_origin", "write_snapshot"]
        );
        let values = origin_values.lock().expect("origin values lock");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0].0, ClipboardChangeOrigin::RemotePush);
        assert_eq!(values[0].1, Duration::from_millis(100));
    }

    #[tokio::test]
    async fn no_op_when_clipboard_already_matches() {
        let (usecase, writes, calls, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            build_text_snapshot("same text"),
            "local-1",
            true,
        );

        usecase
            .execute(build_message("same text", "remote-1"))
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(
            calls.lock().expect("calls lock").as_slice(),
            ["read_snapshot"]
        );
    }

    #[tokio::test]
    async fn ignores_self_origin_messages() {
        let (usecase, writes, calls, _, decrypt_calls) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "device-self",
            true,
        );

        usecase
            .execute(build_message("self text", "device-self"))
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(decrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn no_op_when_encryption_session_not_ready() {
        let (usecase, writes, calls, _, decrypt_calls) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            false,
        );

        usecase
            .execute(build_message("not ready", "remote-1"))
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(decrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn passive_mode_persists_without_os_clipboard_calls_and_dedupes_by_message_id() {
        let (usecase, writes, calls, save_calls, insert_calls) =
            build_passive_usecase(build_text_snapshot("local"), "local-1");

        let message = build_message("passive inbound", "remote-1");
        usecase
            .execute(message.clone())
            .await
            .expect("execute passive inbound message");
        usecase
            .execute(message)
            .await
            .expect("execute duplicated passive inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(save_calls.load(Ordering::SeqCst), 1);
        assert_eq!(insert_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_with_outcome_marks_duplicate_as_skipped_in_passive_mode() {
        let (usecase, _, _, _, _) = build_passive_usecase(build_text_snapshot("local"), "local-1");
        let message = build_message("passive inbound", "remote-1");

        let first = usecase
            .execute_with_outcome(message.clone())
            .await
            .expect("first passive apply");
        let second = usecase
            .execute_with_outcome(message)
            .await
            .expect("second passive apply");

        assert!(matches!(
            first,
            InboundApplyOutcome::Applied { entry_id: Some(_) }
        ));
        assert_eq!(second, InboundApplyOutcome::Skipped);
    }
}
