use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::usecases::clipboard::ClipboardIntegrationMode;
use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span, warn, Instrument};
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::network::protocol::{
    BinaryRepresentation, ClipboardBinaryPayload, ClipboardPayloadVersion, MIME_IMAGE_PREFIX,
    MIME_TEXT_HTML, MIME_TEXT_PLAIN, MIME_TEXT_RTF,
};

use uc_core::network::ClipboardMessage;
use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort};
use uc_core::ports::{
    ClipboardChangeOriginPort, ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
    ClipboardRepresentationNormalizerPort, DeviceIdentityPort, EncryptionPort,
    EncryptionSessionPort, SelectRepresentationPolicyPort, SystemClipboardPort,
    TransferPayloadDecryptorPort,
};
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
    #[allow(dead_code)]
    encryption: Arc<dyn EncryptionPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
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
        transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
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
            transfer_decryptor,
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
        transfer_decryptor: Arc<dyn TransferPayloadDecryptorPort>,
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
            transfer_decryptor,
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

    pub async fn execute(
        &self,
        message: ClipboardMessage,
        pre_decoded_plaintext: Option<Vec<u8>>,
    ) -> Result<()> {
        self.execute_with_outcome(message, pre_decoded_plaintext)
            .await
            .map(|_| ())
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
        pre_decoded_plaintext: Option<Vec<u8>>,
    ) -> Result<InboundApplyOutcome> {
        let span = info_span!(
            "usecase.clipboard.sync_inbound.execute",
            message_id = %message.id,
            origin_device_id = %message.origin_device_id,
            payload_version = ?message.payload_version,
        );

        async move {
            info!(
                mode = ?self.mode,
                allow_os_read = self.mode.allow_os_read(),
                allow_os_write = self.mode.allow_os_write(),
                incoming_content_hash = %message.content_hash,
                "Processing inbound clipboard message"
            );

            // Echo prevention: check before any decryption attempt
            let local_device_id = self.device_identity.current_device_id().to_string();
            if message.origin_device_id == local_device_id {
                debug!("Ignoring inbound clipboard message from local device");
                return Ok(InboundApplyOutcome::Skipped);
            }

            if !self.encryption_session.is_ready().await {
                info!("Skipping inbound apply because encryption session is not ready");
                return Ok(InboundApplyOutcome::Skipped);
            }

            match message.payload_version {
                ClipboardPayloadVersion::V3 => {
                    self.apply_v3_inbound(message, pre_decoded_plaintext).await
                }
                #[allow(unreachable_patterns)]
                other => {
                    error!(version = ?other, "Unsupported inbound payload version — dropping message");
                    Ok(InboundApplyOutcome::Skipped)
                }
            }
        }
        .instrument(span)
        .await
    }

    /// V3 inbound path: decode V3 binary payload, select highest-priority representation.
    ///
    /// Dedup strategy: uses recent_ids by message.id only.
    /// Unlike V1, we do NOT read the OS clipboard to compare snapshot_hash.
    /// Rationale: V3 carries a multi-representation payload whose snapshot_hash is computed from ALL
    /// representations. The OS clipboard holds only the highest-priority representation written by a
    /// prior receive. Comparing snapshot_hash against the OS clipboard would require re-reading
    /// the OS clipboard and re-computing a hash, which is expensive and fragile (OS clipboard format
    /// may not round-trip exactly). The recent_ids dedup (by message.id, TTL-bounded) is sufficient
    /// to prevent duplicate processing from the same message broadcast to multiple paths.
    async fn apply_v3_inbound(
        &self,
        message: ClipboardMessage,
        pre_decoded_plaintext: Option<Vec<u8>>,
    ) -> Result<InboundApplyOutcome> {
        // V3 dedup: by message.id only (see rationale above)
        self.prune_recent_ids().await;
        {
            let now = Instant::now();
            let mut recent_ids = self.recent_ids.lock().await;
            let is_duplicate = recent_ids.iter().any(|(id, _)| id == &message.id);
            if is_duplicate {
                debug!(
                    message_id = %message.id,
                    dedupe_hit = true,
                    "Skipping V3 inbound: already processed this message id"
                );
                return Ok(InboundApplyOutcome::Skipped);
            }
            recent_ids.push_back((message.id.clone(), now));
            while recent_ids.len() > RECENT_ID_MAX {
                recent_ids.pop_front();
            }
        }

        // Decrypt/decode within inbound.decode span
        let payload = async {
            // Use pre-decoded plaintext from transport layer when available (streaming decode),
            // otherwise fall back to in-process decrypt + decode.
            let plaintext_bytes = match pre_decoded_plaintext {
                Some(bytes) => bytes,
                None => {
                    // Fallback: transport didn't pre-decode — decrypt in-process
                    let master_key = self
                        .encryption_session
                        .get_master_key()
                        .await
                        .map_err(anyhow::Error::from)
                        .context("failed to get master key for V3 inbound")?;
                    match self
                        .transfer_decryptor
                        .decrypt(&message.encrypted_content, &master_key)
                    {
                        Ok(bytes) => bytes,
                        Err(e) => {
                            error!(
                                error = %e,
                                message_id = %message.id,
                                "V3 inbound: failed to decrypt chunked payload — dropping message"
                            );
                            self.rollback_recent_id(&message.id).await;
                            return Err(anyhow::anyhow!(
                                "V3 inbound: failed to decrypt chunked payload for message {}: {e}",
                                message.id
                            ));
                        }
                    }
                }
            };

            // Decode V3 binary payload
            let v3_payload = ClipboardBinaryPayload::decode_from(&mut Cursor::new(
                &plaintext_bytes,
            ))
            .map_err(|e| {
                anyhow::anyhow!(
                    "V3 inbound: failed to decode binary payload for message {}: {e}",
                    message.id
                )
            })?;

            Ok::<ClipboardBinaryPayload, anyhow::Error>(v3_payload)
        }
        .instrument(info_span!(
            "inbound.decode",
            wire_bytes = message.encrypted_content.len(),
        ))
        .await;

        let v3_payload = match payload {
            Ok(p) => p,
            Err(e) => {
                self.rollback_recent_id(&message.id).await;
                return Err(e);
            }
        };

        // Log each representation at debug level
        for rep in &v3_payload.representations {
            debug!(
                format_id = %rep.format_id,
                mime = ?rep.mime,
                size = rep.data.len(),
                "inbound rep"
            );
        }
        info!(
            rep_count = v3_payload.representations.len(),
            "V3 inbound payload decoded"
        );

        let selected_idx = match select_highest_priority_repr_index(&v3_payload.representations) {
            Some(i) => i,
            None => {
                warn!(message_id = %message.id, "V3 inbound: no representations — dropping");
                self.rollback_recent_id(&message.id).await;
                return Ok(InboundApplyOutcome::Skipped);
            }
        };

        let representations: Vec<ObservedClipboardRepresentation> = v3_payload
            .representations
            .into_iter()
            .map(|rep| {
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from(rep.format_id.as_str()),
                    rep.mime.map(MimeType),
                    rep.data,
                )
            })
            .collect();
        // Keep only the highest-priority representation.
        // write_snapshot requires exactly ONE representation (tracked in issue #92).
        let representations = vec![representations.into_iter().nth(selected_idx).unwrap()];

        let snapshot = SystemClipboardSnapshot {
            ts_ms: v3_payload.ts_ms,
            representations,
        };

        // In Full mode: remember inbound snapshot hash + write to OS clipboard
        if self.mode.allow_os_write() {
            let snapshot_hash = snapshot.snapshot_hash().to_string();
            self.clipboard_change_origin
                .remember_remote_snapshot_hash(
                    snapshot_hash.clone(),
                    Duration::from_millis(REMOTE_SNAPSHOT_HASH_TTL_MS),
                )
                .await;

            if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
                self.clipboard_change_origin
                    .consume_origin_for_snapshot_or_default(
                        &snapshot_hash,
                        ClipboardChangeOrigin::LocalCapture,
                    )
                    .await;
                self.rollback_recent_id(&message.id).await;
                return Err(err).context("V3 inbound: failed to write snapshot to OS clipboard");
            }
            info!(message_id = %message.id, "V3 inbound clipboard applied");
            return Ok(InboundApplyOutcome::Applied { entry_id: None });
        }

        // In Passive mode (allow_os_read = false): persist via capture use case
        if !self.mode.allow_os_read() {
            let capture = self
                .capture_clipboard
                .as_ref()
                .context("V3 passive inbound: capture dependencies required")?;
            return match capture
                .execute_with_origin(snapshot, ClipboardChangeOrigin::RemotePush)
                .await
            {
                Ok(Some(entry_id)) => {
                    info!(message_id = %message.id, "V3 inbound clipboard persisted (passive)");
                    Ok(InboundApplyOutcome::Applied {
                        entry_id: Some(entry_id),
                    })
                }
                Ok(None) => {
                    self.rollback_recent_id(&message.id).await;
                    Err(anyhow::anyhow!("V3 passive capture skipped persistence"))
                }
                Err(err) => {
                    self.rollback_recent_id(&message.id).await;
                    Err(err).context("V3 passive inbound: capture failed")
                }
            };
        }

        // WriteOnly mode — should not happen in practice for inbound
        info!(mode = ?self.mode, "V3 inbound: mode disallows write — skipped");
        Ok(InboundApplyOutcome::Skipped)
    }
}

const REMOTE_SNAPSHOT_HASH_TTL_MS: u64 = 60_000;

/// Returns the index of the highest-priority BinaryRepresentation, or None if empty.
///
/// Priority order (highest first): image/* > text/plain > text/html > text/rtf > other.
/// While write_snapshot is single-representation-only, prefer plain text for textual payloads.
fn select_highest_priority_repr_index(representations: &[BinaryRepresentation]) -> Option<usize> {
    fn fallback_priority_from_format_id(format_id: &str) -> u8 {
        if format_id.eq_ignore_ascii_case("public.png")
            || format_id.eq_ignore_ascii_case("public.jpeg")
            || format_id.eq_ignore_ascii_case("public.jpg")
            || format_id.eq_ignore_ascii_case("public.tiff")
            || format_id.eq_ignore_ascii_case("public.gif")
            || format_id.eq_ignore_ascii_case("public.webp")
            || format_id.eq_ignore_ascii_case("image/png")
            || format_id.eq_ignore_ascii_case("image/jpeg")
            || format_id.eq_ignore_ascii_case("image/jpg")
            || format_id.eq_ignore_ascii_case("image/gif")
            || format_id.eq_ignore_ascii_case("image/webp")
        {
            4
        } else if format_id.eq_ignore_ascii_case("text")
            || format_id.eq_ignore_ascii_case("public.utf8-plain-text")
            || format_id.eq_ignore_ascii_case("public.text")
            || format_id.eq_ignore_ascii_case("NSStringPboardType")
            || format_id.eq_ignore_ascii_case(MIME_TEXT_PLAIN)
        {
            3
        } else if format_id.eq_ignore_ascii_case("public.html")
            || format_id.eq_ignore_ascii_case("html")
            || format_id.eq_ignore_ascii_case(MIME_TEXT_HTML)
        {
            2
        } else if format_id.eq_ignore_ascii_case("public.rtf")
            || format_id.eq_ignore_ascii_case("rtf")
            || format_id.eq_ignore_ascii_case(MIME_TEXT_RTF)
        {
            1
        } else {
            0
        }
    }

    fn priority_from_mime(mime: &str) -> u8 {
        let normalized = mime
            .split(';')
            .next()
            .map(str::trim)
            .unwrap_or_default()
            .to_ascii_lowercase();
        if normalized.starts_with(MIME_IMAGE_PREFIX) {
            4
        } else if normalized == MIME_TEXT_PLAIN {
            3
        } else if normalized == MIME_TEXT_HTML {
            2
        } else if normalized == MIME_TEXT_RTF {
            1
        } else {
            0
        }
    }

    fn priority(rep: &BinaryRepresentation) -> u8 {
        match rep.mime.as_deref() {
            Some(mime) => {
                let mime_priority = priority_from_mime(mime);
                if mime_priority > 0 {
                    mime_priority
                } else {
                    fallback_priority_from_format_id(&rep.format_id)
                }
            }
            None => fallback_priority_from_format_id(&rep.format_id),
        }
    }

    representations
        .iter()
        .enumerate()
        .max_by_key(|(_, r)| priority(r))
        .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use tracing_subscriber::{fmt::MakeWriter, EnvFilter};
    use uc_core::clipboard::{ClipboardSelection, PolicyError, SelectionPolicyVersion};
    use uc_core::ids::RepresentationId;
    use uc_core::network::protocol::ClipboardPayloadVersion;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, KdfParams, Kek, MasterKey, Passphrase,
    };
    use uc_core::{
        ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
        DeviceId, MimeType, ObservedClipboardRepresentation, PersistedClipboardRepresentation,
        SystemClipboardSnapshot,
    };
    use uc_infra::clipboard::TransferPayloadDecryptorAdapter;

    #[test]
    fn select_highest_priority_uses_format_id_fallback_when_mime_missing() {
        let representations = vec![
            BinaryRepresentation {
                format_id: "public.utf8-plain-text".to_string(),
                mime: None,
                data: b"plain".to_vec(),
            },
            BinaryRepresentation {
                format_id: "public.html".to_string(),
                mime: None,
                data: b"<b>html</b>".to_vec(),
            },
        ];

        let idx = select_highest_priority_repr_index(&representations).expect("selected index");
        assert_eq!(
            idx, 0,
            "plain text fallback should outrank html while single-rep restore is enabled"
        );
    }

    #[test]
    fn select_highest_priority_trims_mime_parameters() {
        let representations = vec![
            BinaryRepresentation {
                format_id: "public.html".to_string(),
                mime: Some("text/html; charset=utf-8".to_string()),
                data: b"<b>html</b>".to_vec(),
            },
            BinaryRepresentation {
                format_id: "public.html".to_string(),
                mime: Some("text/plain; charset=utf-8".to_string()),
                data: b"<b>html</b>".to_vec(),
            },
            BinaryRepresentation {
                format_id: "public.utf8-plain-text".to_string(),
                mime: Some("text/plain; charset=utf-8".to_string()),
                data: b"plain".to_vec(),
            },
        ];

        let idx = select_highest_priority_repr_index(&representations).expect("selected index");
        assert_eq!(
            representations[idx].mime.as_deref(),
            Some("text/plain; charset=utf-8"),
            "mime params should be trimmed before priority matching"
        );
    }

    #[test]
    fn select_highest_priority_falls_back_to_format_id_when_mime_unknown() {
        let representations = vec![
            BinaryRepresentation {
                format_id: "public.utf8-plain-text".to_string(),
                mime: Some("application/x-custom; version=1".to_string()),
                data: b"plain".to_vec(),
            },
            BinaryRepresentation {
                format_id: "public.html".to_string(),
                mime: Some("application/x-custom; version=1".to_string()),
                data: b"<b>html</b>".to_vec(),
            },
        ];

        let idx = select_highest_priority_repr_index(&representations).expect("selected index");
        assert_eq!(
            idx, 0,
            "unknown mime should fall back to format_id priority"
        );
    }

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
        remote_hash_values: Arc<Mutex<Vec<(String, Duration)>>>,
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

        async fn remember_remote_snapshot_hash(&self, snapshot_hash: String, ttl: Duration) {
            self.calls
                .lock()
                .expect("calls lock")
                .push("remember_remote_snapshot_hash");
            self.remote_hash_values
                .lock()
                .expect("remote hash values lock")
                .push((snapshot_hash, ttl));
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

    #[derive(Clone)]
    struct SharedLogBuffer {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    struct SharedLogWriter {
        buffer: Arc<Mutex<Vec<u8>>>,
    }

    impl<'a> MakeWriter<'a> for SharedLogBuffer {
        type Writer = SharedLogWriter;

        fn make_writer(&'a self) -> Self::Writer {
            SharedLogWriter {
                buffer: self.buffer.clone(),
            }
        }
    }

    impl Write for SharedLogWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let mut guard = self.buffer.lock().expect("log buffer lock");
            guard.extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    static LOG_BUFFER: OnceLock<Arc<Mutex<Vec<u8>>>> = OnceLock::new();

    fn init_test_tracing() -> Arc<Mutex<Vec<u8>>> {
        LOG_BUFFER
            .get_or_init(|| {
                let buffer = Arc::new(Mutex::new(Vec::new()));
                let writer = SharedLogBuffer {
                    buffer: buffer.clone(),
                };
                let subscriber = tracing_subscriber::fmt()
                    .with_ansi(false)
                    .with_env_filter(EnvFilter::new("info,warn"))
                    .with_writer(writer)
                    .finish();
                let _ = tracing::subscriber::set_global_default(subscriber);
                buffer
            })
            .clone()
    }

    /// Build a V3 binary payload as plaintext bytes.
    fn build_v3_plaintext(representations: Vec<BinaryRepresentation>, ts_ms: i64) -> Vec<u8> {
        let payload = ClipboardBinaryPayload {
            ts_ms,
            representations,
        };
        payload.encode_to_vec().expect("encode V3 payload")
    }

    /// Build a V3 ClipboardMessage with pre-decoded plaintext (transport already decoded).
    fn build_v3_message_pre_decoded(
        representations: Vec<BinaryRepresentation>,
        origin_device_id: &str,
        message_id: &str,
    ) -> (ClipboardMessage, Vec<u8>) {
        let plaintext = build_v3_plaintext(representations, 1_713_000_000_000);
        let message = ClipboardMessage {
            id: message_id.to_string(),
            content_hash: "v3-snapshot-hash".to_string(),
            encrypted_content: vec![], // empty — transport already decoded
            timestamp: Utc::now(),
            origin_device_id: origin_device_id.to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V3,
        };
        (message, plaintext)
    }

    /// Build a simple V3 text/plain message with pre-decoded plaintext.
    fn build_v3_text_message(text: &str, origin_device_id: &str) -> (ClipboardMessage, Vec<u8>) {
        build_v3_message_pre_decoded(
            vec![BinaryRepresentation {
                format_id: "text".to_string(),
                mime: Some("text/plain".to_string()),
                data: text.as_bytes().to_vec(),
            }],
            origin_device_id,
            "msg-1",
        )
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
        Arc<Mutex<Vec<(String, Duration)>>>,
        Arc<AtomicUsize>,
    ) {
        let writes = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let origin_values = Arc::new(Mutex::new(Vec::new()));
        let remote_hash_values = Arc::new(Mutex::new(Vec::new()));
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
                remote_hash_values: remote_hash_values.clone(),
            }),
            Arc::new(MockEncryptionSession { ready }),
            Arc::new(MockEncryption {
                decrypt_calls: decrypt_calls.clone(),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new(local_device_id),
            }),
            Arc::new(TransferPayloadDecryptorAdapter),
        )
        .expect("build inbound usecase");

        (
            usecase,
            writes,
            calls,
            origin_values,
            remote_hash_values,
            decrypt_calls,
        )
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
                remote_hash_values: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(MockEncryptionSession { ready: true }),
            Arc::new(MockEncryption {
                decrypt_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new(local_device_id),
            }),
            Arc::new(TransferPayloadDecryptorAdapter),
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
    async fn valid_v3_inbound_message_applies_text_plain_snapshot() {
        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let (message, plaintext) = build_v3_text_message("hello inbound", "remote-1");
        usecase
            .execute(message, Some(plaintext))
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
                remote_hash_values: Arc::new(Mutex::new(Vec::new())),
            }),
            Arc::new(MockEncryptionSession { ready: true }),
            Arc::new(MockEncryption {
                decrypt_calls: Arc::new(AtomicUsize::new(0)),
            }),
            Arc::new(MockDeviceIdentity {
                id: DeviceId::new("local-1"),
            }),
            Arc::new(TransferPayloadDecryptorAdapter),
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
    async fn remembers_remote_snapshot_hash_before_write() {
        let (usecase, _, calls, origin_values, remote_hash_values, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let (message, plaintext) = build_v3_text_message("hello order", "remote-1");
        usecase
            .execute(message, Some(plaintext))
            .await
            .expect("execute inbound message");

        // V3 inbound does NOT read OS clipboard for dedup (uses message.id dedup)
        assert_eq!(
            calls.lock().expect("calls lock").as_slice(),
            ["remember_remote_snapshot_hash", "write_snapshot"]
        );
        let values = origin_values.lock().expect("origin values lock");
        assert_eq!(values.len(), 0);
        let remote_values = remote_hash_values.lock().expect("remote hash values lock");
        assert_eq!(remote_values.len(), 1);
        assert_eq!(remote_values[0].1, Duration::from_secs(60));
    }

    #[tokio::test]
    async fn ignores_self_origin_messages() {
        let (usecase, writes, calls, _, _, decrypt_calls) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "device-self",
            true,
        );

        let (message, plaintext) = build_v3_text_message("self text", "device-self");
        usecase
            .execute(message, Some(plaintext))
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(decrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn no_op_when_encryption_session_not_ready() {
        let (usecase, writes, calls, _, _, decrypt_calls) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            false,
        );

        let (message, plaintext) = build_v3_text_message("not ready", "remote-1");
        usecase
            .execute(message, Some(plaintext))
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(decrypt_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn passive_mode_persists_without_os_clipboard_calls_and_dedupes_by_message_id() {
        let (usecase, writes, calls, save_calls, insert_calls) = build_passive_usecase(
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
        );

        let (message, plaintext) = build_v3_text_message("passive inbound", "remote-1");
        let plaintext2 = plaintext.clone();
        usecase
            .execute(message.clone(), Some(plaintext))
            .await
            .expect("execute passive inbound message");
        usecase
            .execute(message, Some(plaintext2))
            .await
            .expect("execute duplicated passive inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 0);
        assert_eq!(calls.lock().expect("calls lock").len(), 0);
        assert_eq!(save_calls.load(Ordering::SeqCst), 1);
        assert_eq!(insert_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn execute_with_outcome_marks_duplicate_as_skipped_in_passive_mode() {
        let (usecase, _, _, _, _) = build_passive_usecase(
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
        );
        let (message, plaintext) = build_v3_text_message("passive inbound", "remote-1");
        let plaintext2 = plaintext.clone();

        let first = usecase
            .execute_with_outcome(message.clone(), Some(plaintext))
            .await
            .expect("first passive apply");
        let second = usecase
            .execute_with_outcome(message, Some(plaintext2))
            .await
            .expect("second passive apply");

        assert!(matches!(
            first,
            InboundApplyOutcome::Applied { entry_id: Some(_) }
        ));
        assert_eq!(second, InboundApplyOutcome::Skipped);
    }

    #[tokio::test]
    async fn v3_message_applies_image_representation_with_highest_priority() {
        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let png_bytes = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header
        let (message, plaintext) = build_v3_message_pre_decoded(
            vec![
                BinaryRepresentation {
                    mime: Some("text/plain".to_string()),
                    format_id: "text".to_string(),
                    data: b"hello world".to_vec(),
                },
                BinaryRepresentation {
                    mime: Some("image/png".to_string()),
                    format_id: "public.png".to_string(),
                    data: png_bytes.clone(),
                },
            ],
            "remote-1",
            "msg-v3-image",
        );

        let outcome = usecase
            .execute_with_outcome(message, Some(plaintext))
            .await
            .expect("execute V3 inbound message");

        // Must be Applied
        assert!(
            matches!(outcome, InboundApplyOutcome::Applied { entry_id: None }),
            "expected Applied, got {:?}",
            outcome
        );

        // image/png must be selected (highest priority)
        let snapshots = writes.lock().expect("writes lock");
        assert_eq!(snapshots.len(), 1, "must write exactly one snapshot");
        let snapshot = &snapshots[0];
        assert_eq!(
            snapshot.representations.len(),
            1,
            "write_snapshot requires exactly one representation"
        );
        assert_eq!(
            snapshot.representations[0]
                .mime
                .as_ref()
                .map(|m| m.as_str()),
            Some("image/png"),
            "must select image/png as highest-priority representation"
        );
        assert_eq!(
            snapshot.representations[0].bytes, png_bytes,
            "must write image bytes"
        );
    }

    #[tokio::test]
    async fn v3_message_with_html_and_text_selects_plain_text() {
        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let (message, plaintext) = build_v3_message_pre_decoded(
            vec![
                BinaryRepresentation {
                    mime: Some("text/plain".to_string()),
                    format_id: "text".to_string(),
                    data: b"plain text".to_vec(),
                },
                BinaryRepresentation {
                    mime: Some("text/html".to_string()),
                    format_id: "html".to_string(),
                    data: b"<b>bold</b>".to_vec(),
                },
            ],
            "remote-1",
            "msg-v3-html",
        );

        let outcome = usecase
            .execute_with_outcome(message, Some(plaintext))
            .await
            .expect("execute V3 html message");

        assert!(matches!(outcome, InboundApplyOutcome::Applied { .. }));

        let snapshots = writes.lock().expect("writes lock");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].representations.len(),
            1,
            "write_snapshot requires exactly one representation"
        );
        assert_eq!(
            snapshots[0].representations[0]
                .mime
                .as_ref()
                .map(|m| m.as_str()),
            Some("text/plain"),
            "must prefer text/plain over text/html while only one representation can be written"
        );
    }

    #[tokio::test]
    async fn v3_inbound_with_invalid_pre_decoded_plaintext_returns_err() {
        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let v3_message = ClipboardMessage {
            id: "msg-bad-pre-decoded".to_string(),
            content_hash: "bad-hash".to_string(),
            encrypted_content: vec![],
            timestamp: Utc::now(),
            origin_device_id: "remote-1".to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V3,
        };

        let result = usecase
            .execute_with_outcome(v3_message, Some(b"not valid binary payload".to_vec()))
            .await;

        // Must return Err (decode failure is a real error, not silent skip)
        assert!(
            result.is_err(),
            "invalid pre-decoded plaintext must return Err, got: {:?}",
            result
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed to decode binary payload"),
            "error must mention decode failure, got: {err_msg}"
        );
        assert_eq!(
            writes.lock().expect("writes lock").len(),
            0,
            "must not write to clipboard on invalid pre-decoded plaintext"
        );
    }

    #[tokio::test]
    async fn v3_inbound_with_pre_decoded_plaintext_applies_correctly() {
        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let (message, plaintext) = build_v3_message_pre_decoded(
            vec![BinaryRepresentation {
                mime: Some("text/plain".to_string()),
                format_id: "text".to_string(),
                data: b"pre-decoded text".to_vec(),
            }],
            "remote-1",
            "msg-pre-decoded",
        );

        let outcome = usecase
            .execute_with_outcome(message, Some(plaintext))
            .await
            .expect("pre-decoded V3 message must apply");

        assert!(
            matches!(outcome, InboundApplyOutcome::Applied { entry_id: None }),
            "expected Applied, got {:?}",
            outcome
        );

        let snapshots = writes.lock().expect("writes lock");
        assert_eq!(snapshots.len(), 1, "must write exactly one snapshot");
        assert_eq!(
            snapshots[0].representations[0].bytes, b"pre-decoded text",
            "must apply pre-decoded plaintext content"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn v3_message_with_tampered_content_returns_err() {
        let log_buffer = init_test_tracing();
        let start_len = log_buffer.lock().expect("log buffer lock").len();

        let (usecase, writes, _, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        // Build a valid V3 message then pass tampered encrypted_content for fallback path
        let message = ClipboardMessage {
            id: "msg-v3-tampered".to_string(),
            content_hash: "tampered-hash".to_string(),
            encrypted_content: vec![0xFF; 100], // invalid encrypted content
            timestamp: Utc::now(),
            origin_device_id: "remote-1".to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V3,
        };

        // No pre-decoded plaintext, so it will try the fallback decrypt path
        let result = usecase.execute_with_outcome(message, None).await;

        // Must return Err (decrypt failure is a real error, not silent skip)
        assert!(
            result.is_err(),
            "tampered V3 content must return Err, got: {:?}",
            result
        );
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("failed to decrypt chunked payload"),
            "error must mention decrypt failure, got: {err_msg}"
        );

        // Must not write anything to clipboard
        assert_eq!(
            writes.lock().expect("writes lock").len(),
            0,
            "must not write to clipboard on decrypt failure"
        );

        // Must log an error
        let guard = log_buffer.lock().expect("log buffer lock");
        let (_, new_bytes) = guard.split_at(start_len);
        let output = String::from_utf8_lossy(new_bytes);
        assert!(
            output.contains("V3 inbound")
                || output.contains("failed to decrypt")
                || output.contains("dropping"),
            "must log error for tampered V3 content, got: {output}"
        );
    }
}
