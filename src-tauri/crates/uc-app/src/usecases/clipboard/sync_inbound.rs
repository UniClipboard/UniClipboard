use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::usecases::clipboard::ClipboardIntegrationMode;
use anyhow::{Context, Result};
use tokio::sync::Mutex;
use tracing::{debug, error, info, info_span, warn, Instrument};
use uc_core::ids::{EntryId, FormatId, RepresentationId};
use uc_core::network::protocol::{
    ClipboardMultiRepPayloadV2, ClipboardPayloadVersion, ClipboardTextPayloadV1, WireRepresentation,
};
use uc_core::network::ClipboardMessage;
use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort};
use uc_core::ports::{
    ClipboardChangeOriginPort, ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
    ClipboardRepresentationNormalizerPort, DeviceIdentityPort, EncryptionPort,
    EncryptionSessionPort, SelectRepresentationPolicyPort, SystemClipboardPort,
    TransferPayloadDecryptorPort,
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

            // Route to V1 or V2 path based on payload_version
            match message.payload_version {
                ClipboardPayloadVersion::V1 => self.apply_v1_inbound(message).await,
                ClipboardPayloadVersion::V2 => {
                    self.apply_v2_inbound(message, pre_decoded_plaintext).await
                }
            }
        }
        .instrument(span)
        .await
    }

    /// V1 inbound path: decrypt via EncryptionPort, parse ClipboardTextPayloadV1.
    /// This path is unchanged for backward compatibility with old senders.
    async fn apply_v1_inbound(&self, message: ClipboardMessage) -> Result<InboundApplyOutcome> {
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

        self.apply_snapshot_v1(message, snapshot).await
    }

    /// Apply a V1 snapshot: handles passive mode dedup, OS-clipboard dedup, and write.
    async fn apply_snapshot_v1(
        &self,
        message: ClipboardMessage,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<InboundApplyOutcome> {
        if !self.mode.allow_os_read() {
            let message_id = message.id.clone();
            self.prune_recent_ids().await;
            {
                let now = Instant::now();
                let mut recent_ids = self.recent_ids.lock().await;
                let is_duplicate = recent_ids.iter().any(|(id, _)| id == &message_id);
                if is_duplicate {
                    debug!(
                        message_id = %message_id,
                        dedupe_hit = true,
                        "Skipping inbound apply because passive mode already processed this message id"
                    );
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
            debug!(
                incoming_content_hash = %message.content_hash,
                dedupe_hit = true,
                "Skipping inbound apply because local clipboard already matches content hash"
            );
            return Ok(InboundApplyOutcome::Skipped);
        }

        self.clipboard_change_origin
            .set_next_origin(
                ClipboardChangeOrigin::RemotePush,
                Duration::from_millis(REMOTE_PUSH_ORIGIN_TTL_MS),
            )
            .await;

        info!(
            write_attempted = true,
            incoming_content_hash = %message.content_hash,
            "Applying inbound snapshot to system clipboard"
        );
        if let Err(err) = self.local_clipboard.write_snapshot(snapshot) {
            self.clipboard_change_origin
                .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
                .await;
            return Err(err).context("failed to write inbound clipboard snapshot");
        }

        info!(
            write_result = "ok",
            incoming_content_hash = %message.content_hash,
            "Inbound clipboard message applied"
        );

        match self.local_clipboard.read_snapshot() {
            Ok(post_write_snapshot) => {
                let post_write_hash_match =
                    snapshot_matches_content_hash(&post_write_snapshot, &message.content_hash);
                let post_write_text_len = first_text_representation_len(&post_write_snapshot);
                if post_write_hash_match {
                    info!(
                        post_write_hash_match,
                        post_write_text_len,
                        "Inbound clipboard write post-check matched content hash"
                    );
                } else {
                    warn!(
                        post_write_hash_match,
                        post_write_text_len,
                        incoming_content_hash = %message.content_hash,
                        "Inbound clipboard write post-check hash mismatch"
                    );
                }
            }
            Err(err) => {
                warn!(
                    error = %err,
                    incoming_content_hash = %message.content_hash,
                    "Inbound clipboard write post-check read failed"
                );
            }
        }

        Ok(InboundApplyOutcome::Applied { entry_id: None })
    }

    /// V2 inbound path: chunk-decrypt via ChunkedDecoder, select highest-priority representation.
    ///
    /// Dedup strategy: uses recent_ids by message.id only.
    /// Unlike V1, we do NOT read the OS clipboard to compare snapshot_hash.
    /// Rationale: V2 carries a multi-representation payload whose snapshot_hash is computed from ALL
    /// representations. The OS clipboard holds only the highest-priority representation written by a
    /// prior V2 receive. Comparing snapshot_hash against the OS clipboard would require re-reading
    /// the OS clipboard and re-computing a hash, which is expensive and fragile (OS clipboard format
    /// may not round-trip exactly). The recent_ids dedup (by message.id, TTL-bounded) is sufficient
    /// to prevent duplicate processing from the same message broadcast to multiple paths.
    async fn apply_v2_inbound(
        &self,
        message: ClipboardMessage,
        pre_decoded_plaintext: Option<Vec<u8>>,
    ) -> Result<InboundApplyOutcome> {
        // V2 dedup: by message.id only (see rationale above)
        self.prune_recent_ids().await;
        {
            let now = Instant::now();
            let mut recent_ids = self.recent_ids.lock().await;
            let is_duplicate = recent_ids.iter().any(|(id, _)| id == &message.id);
            if is_duplicate {
                debug!(
                    message_id = %message.id,
                    dedupe_hit = true,
                    "Skipping V2 inbound: already processed this message id"
                );
                return Ok(InboundApplyOutcome::Skipped);
            }
            recent_ids.push_back((message.id.clone(), now));
            while recent_ids.len() > RECENT_ID_MAX {
                recent_ids.pop_front();
            }
        }

        // Use pre-decoded plaintext from transport layer when available (streaming decode),
        // otherwise fall back to in-process ChunkedDecoder decode.
        let plaintext = match pre_decoded_plaintext {
            Some(bytes) => bytes,
            None => {
                // Fallback: transport didn't pre-decode — decode in-process
                let master_key = self
                    .encryption_session
                    .get_master_key()
                    .await
                    .map_err(anyhow::Error::from)
                    .context("failed to get master key for V2 inbound")?;
                match self
                    .transfer_decryptor
                    .decrypt(&message.encrypted_content, &master_key)
                {
                    Ok(bytes) => bytes,
                    Err(e) => {
                        error!(
                            error = %e,
                            message_id = %message.id,
                            "V2 inbound: failed to decode chunked payload — dropping message"
                        );
                        self.rollback_recent_id(&message.id).await;
                        return Err(anyhow::anyhow!(
                            "V2 inbound: failed to decode chunked payload for message {}: {e}",
                            message.id
                        ));
                    }
                }
            }
        };

        let v2_payload: ClipboardMultiRepPayloadV2 = match serde_json::from_slice(&plaintext) {
            Ok(p) => p,
            Err(e) => {
                error!(
                    error = %e,
                    message_id = %message.id,
                    "V2 inbound: failed to deserialize ClipboardMultiRepPayloadV2 — dropping"
                );
                self.rollback_recent_id(&message.id).await;
                return Err(anyhow::anyhow!(
                    "V2 inbound: failed to deserialize ClipboardMultiRepPayloadV2 for message {}: {e}",
                    message.id
                ));
            }
        };

        let selected = match select_highest_priority_repr(&v2_payload.representations) {
            Some(r) => r,
            None => {
                warn!(message_id = %message.id, "V2 inbound: no representations — dropping");
                self.rollback_recent_id(&message.id).await;
                return Ok(InboundApplyOutcome::Skipped);
            }
        };

        // Construct MimeType from wire string.
        // MimeType(s.to_string()) is correct — there is no from_str_lossy method.
        let mime: Option<MimeType> = selected.mime.as_deref().map(|s| MimeType(s.to_string()));

        // Build a single-representation snapshot from the selected (highest-priority) repr
        let snapshot = SystemClipboardSnapshot {
            ts_ms: v2_payload.ts_ms,
            representations: vec![ObservedClipboardRepresentation {
                id: RepresentationId::new(),
                format_id: FormatId::from(selected.format_id.as_str()),
                mime,
                bytes: selected.bytes.clone(),
            }],
        };

        // In Full mode: set origin + write to OS clipboard
        if self.mode.allow_os_write() {
            self.clipboard_change_origin
                .set_next_origin(
                    ClipboardChangeOrigin::RemotePush,
                    Duration::from_millis(REMOTE_PUSH_ORIGIN_TTL_MS),
                )
                .await;

            if let Err(err) = self.local_clipboard.write_snapshot(snapshot.clone()) {
                self.clipboard_change_origin
                    .consume_origin_or_default(ClipboardChangeOrigin::LocalCapture)
                    .await;
                self.rollback_recent_id(&message.id).await;
                return Err(err).context("V2 inbound: failed to write snapshot to OS clipboard");
            }
            info!(message_id = %message.id, "V2 inbound clipboard applied");
            return Ok(InboundApplyOutcome::Applied { entry_id: None });
        }

        // In Passive mode (allow_os_read = false): persist via capture use case
        if !self.mode.allow_os_read() {
            let capture = self
                .capture_clipboard
                .as_ref()
                .context("V2 passive inbound: capture dependencies required")?;
            return match capture
                .execute_with_origin(snapshot, ClipboardChangeOrigin::RemotePush)
                .await
            {
                Ok(Some(entry_id)) => {
                    info!(message_id = %message.id, "V2 inbound clipboard persisted (passive)");
                    Ok(InboundApplyOutcome::Applied {
                        entry_id: Some(entry_id),
                    })
                }
                Ok(None) => {
                    self.rollback_recent_id(&message.id).await;
                    Err(anyhow::anyhow!("V2 passive capture skipped persistence"))
                }
                Err(err) => {
                    self.rollback_recent_id(&message.id).await;
                    Err(err).context("V2 passive inbound: capture failed")
                }
            };
        }

        // WriteOnly mode — should not happen in practice for inbound
        info!(mode = ?self.mode, "V2 inbound: mode disallows write — skipped");
        Ok(InboundApplyOutcome::Skipped)
    }
}

const REMOTE_PUSH_ORIGIN_TTL_MS: u64 = 100;

/// Select the highest-priority WireRepresentation from a V2 inbound payload.
///
/// Priority order (highest first): image/* > text/html > text/rtf > text/plain > other.
/// This mirrors the locked decision from CONTEXT.md § "Multi-representation strategy".
fn select_highest_priority_repr(
    representations: &[WireRepresentation],
) -> Option<&WireRepresentation> {
    fn priority(mime: Option<&str>) -> u8 {
        match mime {
            Some(m) if m.to_ascii_lowercase().starts_with("image/") => 4,
            Some(m) if m.eq_ignore_ascii_case("text/html") => 3,
            Some(m) if m.eq_ignore_ascii_case("text/rtf") => 2,
            Some(m) if m.eq_ignore_ascii_case("text/plain") => 1,
            _ => 0,
        }
    }
    representations
        .iter()
        .max_by_key(|r| priority(r.mime.as_deref()))
}

fn is_text_plain_mime(mime: &str) -> bool {
    let normalized = mime.trim();
    normalized.eq_ignore_ascii_case(ClipboardTextPayloadV1::MIME_TEXT_PLAIN)
        || normalized.to_ascii_lowercase().starts_with("text/plain;")
}

fn snapshot_matches_content_hash(snapshot: &SystemClipboardSnapshot, target_hash: &str) -> bool {
    snapshot
        .representations
        .iter()
        .any(|rep| rep.content_hash().to_string() == target_hash)
}

fn first_text_representation_len(snapshot: &SystemClipboardSnapshot) -> Option<usize> {
    snapshot
        .representations
        .iter()
        .find(|rep| {
            rep.mime
                .as_ref()
                .is_some_and(|mime| mime.as_str().eq_ignore_ascii_case("text/plain"))
        })
        .map(|rep| rep.bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;

    use async_trait::async_trait;
    use chrono::Utc;
    use tracing_subscriber::{fmt::MakeWriter, EnvFilter};
    use uc_core::clipboard::{ClipboardSelection, PolicyError, SelectionPolicyVersion};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::network::protocol::{
        ClipboardMultiRepPayloadV2, ClipboardPayloadVersion, ClipboardTextPayloadV1,
        WireRepresentation,
    };
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, EncryptionFormatVersion, KdfParams, Kek,
        MasterKey, Passphrase,
    };
    use uc_core::{
        ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
        DeviceId, MimeType, ObservedClipboardRepresentation, PersistedClipboardRepresentation,
        SystemClipboardSnapshot,
    };
    use uc_infra::clipboard::{ChunkedEncoder, TransferPayloadDecryptorAdapter};

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

    struct SequencedReadClipboard {
        read_results: Arc<Mutex<VecDeque<std::result::Result<SystemClipboardSnapshot, String>>>>,
        writes: Arc<Mutex<Vec<SystemClipboardSnapshot>>>,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    impl SystemClipboardPort for SequencedReadClipboard {
        fn read_snapshot(&self) -> Result<SystemClipboardSnapshot> {
            self.calls.lock().expect("calls lock").push("read_snapshot");
            let next = self
                .read_results
                .lock()
                .expect("read_results lock")
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(SystemClipboardSnapshot {
                        ts_ms: 0,
                        representations: vec![],
                    })
                });
            next.map_err(|msg| anyhow::anyhow!(msg))
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
            payload_version: uc_core::network::protocol::ClipboardPayloadVersion::V1,
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
            Arc::new(TransferPayloadDecryptorAdapter),
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
            .execute(build_message("hello inbound", "remote-1"), None)
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
            .execute(build_message("hello order", "remote-1"), None)
            .await
            .expect("execute inbound message");

        assert_eq!(
            calls.lock().expect("calls lock").as_slice(),
            [
                "read_snapshot",
                "set_origin",
                "write_snapshot",
                "read_snapshot"
            ]
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
            .execute(build_message("same text", "remote-1"), None)
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
            .execute(build_message("self text", "device-self"), None)
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
            .execute(build_message("not ready", "remote-1"), None)
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
            .execute(message.clone(), None)
            .await
            .expect("execute passive inbound message");
        usecase
            .execute(message, None)
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
            .execute_with_outcome(message.clone(), None)
            .await
            .expect("first passive apply");
        let second = usecase
            .execute_with_outcome(message, None)
            .await
            .expect("second passive apply");

        assert!(matches!(
            first,
            InboundApplyOutcome::Applied { entry_id: Some(_) }
        ));
        assert_eq!(second, InboundApplyOutcome::Skipped);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn post_write_verification_logs_hash_mismatch_and_keeps_applied_outcome() {
        let log_buffer = init_test_tracing();
        let start_len = log_buffer.lock().expect("log buffer lock").len();

        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            build_text_snapshot("local-before-and-after"),
            "local-1",
            true,
        );

        let outcome = usecase
            .execute_with_outcome(build_message("remote-value", "remote-1"), None)
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 1);
        assert!(matches!(
            outcome,
            InboundApplyOutcome::Applied { entry_id: None }
        ));

        let guard = log_buffer.lock().expect("log buffer lock");
        let (_, new_bytes) = guard.split_at(start_len);
        let output = String::from_utf8_lossy(new_bytes);
        assert!(
            output.contains("Inbound clipboard write post-check hash mismatch"),
            "log output: {output}"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn post_write_verification_logs_read_failure_and_keeps_applied_outcome() {
        let log_buffer = init_test_tracing();
        let start_len = log_buffer.lock().expect("log buffer lock").len();

        let writes = Arc::new(Mutex::new(Vec::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let read_results = Arc::new(Mutex::new(VecDeque::from(vec![
            Ok(build_text_snapshot("dedupe-miss")),
            Err("simulated post-check read failure".to_string()),
        ])));
        let local_clipboard: Arc<dyn SystemClipboardPort> = Arc::new(SequencedReadClipboard {
            read_results,
            writes: writes.clone(),
            calls,
        });

        let usecase = SyncInboundClipboardUseCase::new(
            ClipboardIntegrationMode::Full,
            local_clipboard,
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
            Arc::new(TransferPayloadDecryptorAdapter),
        )
        .expect("build inbound usecase");

        let outcome = usecase
            .execute_with_outcome(build_message("remote-value", "remote-1"), None)
            .await
            .expect("execute inbound message");

        assert_eq!(writes.lock().expect("writes lock").len(), 1);
        assert!(matches!(
            outcome,
            InboundApplyOutcome::Applied { entry_id: None }
        ));

        let guard = log_buffer.lock().expect("log buffer lock");
        let (_, new_bytes) = guard.split_at(start_len);
        let output = String::from_utf8_lossy(new_bytes);
        assert!(
            output.contains("Inbound clipboard write post-check read failed"),
            "log output: {output}"
        );
    }

    /// Build a V2 ClipboardMessage with the given representations.
    /// Uses ChunkedEncoder::encode_to with the same master key as MockEncryptionSession (MasterKey([3; 32])).
    fn build_v2_message(
        representations: Vec<WireRepresentation>,
        origin_device_id: &str,
        message_id: &str,
    ) -> ClipboardMessage {
        let test_master_key = MasterKey([3; 32]); // matches MockEncryptionSession
        let transfer_id = [0x42u8; 16];
        let v2_payload = ClipboardMultiRepPayloadV2 {
            ts_ms: 1_713_000_000_000,
            representations,
        };
        let plaintext = serde_json::to_vec(&v2_payload).expect("serialize V2 payload");
        let mut encrypted_content = Vec::new();
        ChunkedEncoder::encode_to(
            &mut encrypted_content,
            &test_master_key,
            &transfer_id,
            &plaintext,
        )
        .expect("encode V2 payload");

        ClipboardMessage {
            id: message_id.to_string(),
            content_hash: "v2-snapshot-hash".to_string(),
            encrypted_content,
            timestamp: Utc::now(),
            origin_device_id: origin_device_id.to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V2,
        }
    }

    #[tokio::test]
    async fn v2_message_applies_image_representation_with_highest_priority() {
        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let png_bytes = vec![0x89, 0x50, 0x4E, 0x47]; // PNG header
        let message = build_v2_message(
            vec![
                WireRepresentation {
                    mime: Some("text/plain".to_string()),
                    format_id: "text".to_string(),
                    bytes: b"hello world".to_vec(),
                },
                WireRepresentation {
                    mime: Some("image/png".to_string()),
                    format_id: "public.png".to_string(),
                    bytes: png_bytes.clone(),
                },
            ],
            "remote-1",
            "msg-v2-image",
        );

        let outcome = usecase
            .execute_with_outcome(message, None)
            .await
            .expect("execute V2 inbound message");

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
        assert_eq!(snapshot.representations.len(), 1);
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
    async fn v2_message_with_html_and_text_selects_html() {
        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let message = build_v2_message(
            vec![
                WireRepresentation {
                    mime: Some("text/plain".to_string()),
                    format_id: "text".to_string(),
                    bytes: b"plain text".to_vec(),
                },
                WireRepresentation {
                    mime: Some("text/html".to_string()),
                    format_id: "html".to_string(),
                    bytes: b"<b>bold</b>".to_vec(),
                },
            ],
            "remote-1",
            "msg-v2-html",
        );

        let outcome = usecase
            .execute_with_outcome(message, None)
            .await
            .expect("execute V2 html message");

        assert!(matches!(outcome, InboundApplyOutcome::Applied { .. }));

        let snapshots = writes.lock().expect("writes lock");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].representations[0]
                .mime
                .as_ref()
                .map(|m| m.as_str()),
            Some("text/html"),
            "must prefer text/html over text/plain"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn v2_message_with_tampered_content_returns_skipped() {
        let log_buffer = init_test_tracing();
        let start_len = log_buffer.lock().expect("log buffer lock").len();

        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        // Build a valid V2 message then tamper with the encrypted_content
        let mut message = build_v2_message(
            vec![WireRepresentation {
                mime: Some("text/plain".to_string()),
                format_id: "text".to_string(),
                bytes: b"secret data".to_vec(),
            }],
            "remote-1",
            "msg-v2-tampered",
        );
        // Flip a byte in encrypted_content to cause AEAD auth failure
        if !message.encrypted_content.is_empty() {
            let last = message.encrypted_content.len() - 1;
            message.encrypted_content[last] ^= 0xFF;
        }

        let outcome = usecase
            .execute_with_outcome(message, None)
            .await
            .expect("tampered V2 message must not panic or return Err");

        // Must return Skipped (not panic, not propagate error)
        assert_eq!(
            outcome,
            InboundApplyOutcome::Skipped,
            "tampered V2 content must return Skipped"
        );

        // Must not write anything to clipboard
        assert_eq!(
            writes.lock().expect("writes lock").len(),
            0,
            "must not write to clipboard on decode failure"
        );

        // Must log an error
        let guard = log_buffer.lock().expect("log buffer lock");
        let (_, new_bytes) = guard.split_at(start_len);
        let output = String::from_utf8_lossy(new_bytes);
        assert!(
            output.contains("V2 inbound")
                || output.contains("failed to decode")
                || output.contains("dropping"),
            "must log error for tampered V2 content, got: {output}"
        );
    }

    #[tokio::test]
    async fn v2_inbound_with_pre_decoded_plaintext_applies_correctly() {
        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        // Build a V2 ClipboardMessage with empty encrypted_content (transport already decoded)
        let v2_message = ClipboardMessage {
            id: "msg-pre-decoded".to_string(),
            content_hash: "pre-decoded-hash".to_string(),
            encrypted_content: vec![], // empty — transport already decoded
            timestamp: Utc::now(),
            origin_device_id: "remote-1".to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V2,
        };

        // Build plaintext as transport would provide it
        let v2_payload = ClipboardMultiRepPayloadV2 {
            ts_ms: 1_713_000_000_000,
            representations: vec![WireRepresentation {
                mime: Some("text/plain".to_string()),
                format_id: "text".to_string(),
                bytes: b"pre-decoded text".to_vec(),
            }],
        };
        let plaintext = serde_json::to_vec(&v2_payload).expect("serialize V2 payload");

        let outcome = usecase
            .execute_with_outcome(v2_message, Some(plaintext))
            .await
            .expect("pre-decoded V2 message must apply");

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

    #[tokio::test]
    async fn v2_inbound_with_invalid_pre_decoded_plaintext_returns_skipped() {
        let (usecase, writes, _, _, _) = build_usecase(
            ClipboardIntegrationMode::Full,
            SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            },
            "local-1",
            true,
        );

        let v2_message = ClipboardMessage {
            id: "msg-bad-pre-decoded".to_string(),
            content_hash: "bad-hash".to_string(),
            encrypted_content: vec![],
            timestamp: Utc::now(),
            origin_device_id: "remote-1".to_string(),
            origin_device_name: "peer-device".to_string(),
            payload_version: ClipboardPayloadVersion::V2,
        };

        let outcome = usecase
            .execute_with_outcome(v2_message, Some(b"not valid json".to_vec()))
            .await
            .expect("invalid pre-decoded plaintext must not panic");

        assert_eq!(
            outcome,
            InboundApplyOutcome::Skipped,
            "invalid pre-decoded plaintext must return Skipped"
        );
        assert_eq!(
            writes.lock().expect("writes lock").len(),
            0,
            "must not write to clipboard on invalid pre-decoded plaintext"
        );
    }

    #[tokio::test]
    async fn v1_message_path_unchanged_after_v2_changes() {
        // Verify V1 backward compatibility: existing V1 message still applies correctly
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
            .execute(build_message("hello v1", "remote-1"), None)
            .await
            .expect("V1 message must still apply");

        let snapshots = writes.lock().expect("writes lock");
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].representations[0].bytes, b"hello v1");
    }
}
