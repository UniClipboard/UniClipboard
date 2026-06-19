//! `ActiveClipboardPullServeUseCase` — produces the transfer envelope a peer
//! requests on demand for the active-clipboard pull path (issue #1017 PR8).
//!
//! ## Why this reuses the resend crypto chain (D4)
//!
//! Serving a pull is **not** a byte copy of any at-rest encrypted form. The
//! on-disk blob format and the transfer wire format differ, and the receiver
//! hard-rejects anything that is not a fresh transfer envelope. So serving is
//! `decrypt → re-encrypt → frame`, exactly the chain
//! [`ResendEntryUseCase`](super::resend_entry::ResendEntryUseCase) already
//! runs:
//!
//! 1. resolve the local entry by its cross-device `content_hash`;
//! 2. [`reconstruct_snapshot_from_entry`] — read at-rest, decrypt, materialize
//!    plaintext (requires an unlocked session);
//! 3. plan + publish blobs — large/image reps and free-standing files are
//!    published into **this device's** blob store, which issues a fresh
//!    ticket **pinned to this device** (D3: the relay re-signs the ticket so a
//!    downstream fetch dials the holder, not the original provider);
//! 4. [`encode_snapshot_with_blob_refs_to_v3_bytes`] — frame the V3 envelope;
//! 5. [`TransferCipherPort::encrypt`] — wrap it with a fresh transfer identity.
//!
//! A locked session cannot decrypt at step 2, so it cannot serve: the use case
//! returns [`ActiveClipboardPullServeError::NotUnlocked`] without ever touching
//! plaintext. Content not held locally returns
//! [`ActiveClipboardPullServeError::NotAvailable`].
//!
//! The downstream fan-out (`DispatchEntryRunner`) that `ResendEntryUseCase`
//! drives is intentionally **not** part of this chain — a pull serve produces
//! one envelope for one requester, so the use case stops at the encrypted
//! bytes.

use std::sync::Arc;

use async_trait::async_trait;
use tracing::{debug, info, instrument, warn};

use uc_core::clipboard::ClipboardChangeOrigin;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    ActiveClipboardPullServeError, ActiveClipboardPullServePort, FindEntryIdBySnapshotHashPort,
};
use uc_core::ports::security::{TransferCipherError, TransferCipherPort};
use uc_core::ports::SettingsPort;

use crate::facade::clipboard_outbound::{
    extract_file_paths_from_snapshot, publish_file_blob_refs, publish_oversized_inline_blob_refs,
    OutboundBlobPublishGateway,
};
use crate::sync_planner::{FileCandidate, OutboundSyncPlanner};
use crate::usecases::clipboard_sync::payload_codec::encode_snapshot_with_blob_refs_to_v3_bytes;
use crate::usecases::clipboard_sync::snapshot_from_entry::{
    BuildSnapshotError, SnapshotReconstructor,
};

/// Produces the on-demand transfer envelope for a pulled content hash. Wraps
/// the shared resend crypto chain so the pull serve path has a single source
/// of truth for "decrypt → re-encrypt → frame".
pub(crate) struct ActiveClipboardPullServeUseCase {
    entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    reconstructor: SnapshotReconstructor,
    settings: Arc<dyn SettingsPort>,
    blob_publisher: Arc<dyn OutboundBlobPublishGateway>,
    cipher: Arc<dyn TransferCipherPort>,
}

/// Bundled dependencies for [`ActiveClipboardPullServeUseCase`].
pub(crate) struct ActiveClipboardPullServeDeps {
    pub entry_lookup: Arc<dyn FindEntryIdBySnapshotHashPort>,
    pub reconstructor: SnapshotReconstructor,
    pub settings: Arc<dyn SettingsPort>,
    pub blob_publisher: Arc<dyn OutboundBlobPublishGateway>,
    pub cipher: Arc<dyn TransferCipherPort>,
}

impl ActiveClipboardPullServeUseCase {
    pub(crate) fn new(deps: ActiveClipboardPullServeDeps) -> Self {
        Self {
            entry_lookup: deps.entry_lookup,
            reconstructor: deps.reconstructor,
            settings: deps.settings,
            blob_publisher: deps.blob_publisher,
            cipher: deps.cipher,
        }
    }

    /// Build the transfer envelope for `content_hash`. See the module docs for
    /// the chain. Returns [`ActiveClipboardPullServeError::NotAvailable`] when
    /// the content is not held / not materializable, and
    /// [`ActiveClipboardPullServeError::NotUnlocked`] when the session is
    /// locked.
    #[instrument(name = "active_state.serve_pull", skip_all, fields(content_hash = %content_hash))]
    pub(crate) async fn serve(
        &self,
        content_hash: &str,
    ) -> Result<Vec<u8>, ActiveClipboardPullServeError> {
        // 1. Resolve the local entry by cross-device content hash. No match →
        //    not available (the observing peer should ask another holder).
        let entry_id = match self
            .entry_lookup
            .find_entry_id_by_snapshot_hash(content_hash)
            .await
        {
            Ok(Some(id)) => id,
            Ok(None) => {
                debug!("pull serve: content not held locally");
                return Err(ActiveClipboardPullServeError::NotAvailable);
            }
            Err(err) => {
                warn!(error = %err, "pull serve: entry lookup failed");
                return Err(ActiveClipboardPullServeError::Internal(format!(
                    "entry lookup: {err}"
                )));
            }
        };

        // 2. Reconstruct the snapshot (decrypt + materialize plaintext). A
        //    reconstruction failure means the payload is lost / unavailable →
        //    NotAvailable, so the requester treats this holder as unable to
        //    serve rather than retrying a permanently-gone payload.
        let snapshot = match self.reconstructor.reconstruct(&entry_id).await {
            Ok(s) => s,
            Err(err) => return Err(map_reconstruct_error(err, &entry_id)),
        };

        // 3. Plan + publish blobs. Planning with `Resend` origin treats this as
        //    a user-initiated outbound (bypassing the `max_file_size` capture
        //    guard, like resend). Publishing re-issues blob tickets pinned to
        //    THIS device (D3) so a downstream fetch dials the holder.
        let resolved_paths = extract_file_paths_from_snapshot(&snapshot);
        let extracted_paths_count = resolved_paths.len();
        let mut file_candidates = Vec::with_capacity(resolved_paths.len());
        for path in resolved_paths {
            match tokio::fs::metadata(&path).await {
                Ok(meta) => file_candidates.push(FileCandidate {
                    path,
                    size: meta.len(),
                }),
                Err(err) => warn!(
                    error = %err,
                    "pull serve: excluding clipboard file whose metadata could not be read"
                ),
            }
        }

        let planner = OutboundSyncPlanner::new(Arc::clone(&self.settings));
        let plan = planner
            .plan(
                snapshot,
                ClipboardChangeOrigin::Resend,
                file_candidates,
                extracted_paths_count,
            )
            .await;
        let Some(mut clipboard_intent) = plan.clipboard else {
            // The only branch that drops `clipboard` for a Resend-origin plan
            // is `all_files_excluded` (every referenced file gone). Treat as
            // not available.
            debug!("pull serve: all referenced files excluded; content not available");
            return Err(ActiveClipboardPullServeError::NotAvailable);
        };

        let mut blob_refs = match publish_file_blob_refs(
            self.blob_publisher.as_ref(),
            &plan.files,
            &entry_id,
        )
        .await
        {
            Ok(refs) => refs,
            Err(err) => {
                warn!(error = %err, "pull serve: file blob publish failed");
                return Err(ActiveClipboardPullServeError::Internal(format!(
                    "publish file blobs: {err}"
                )));
            }
        };
        let mut image_blob_refs = match publish_oversized_inline_blob_refs(
            self.blob_publisher.as_ref(),
            &mut clipboard_intent.snapshot,
            &entry_id,
        )
        .await
        {
            Ok(refs) => refs,
            Err(err) => {
                warn!(error = %err, "pull serve: inline blob publish failed");
                return Err(ActiveClipboardPullServeError::Internal(format!(
                    "publish inline blobs: {err}"
                )));
            }
        };
        blob_refs.append(&mut image_blob_refs);

        // 4. Encode the V3 envelope (snapshot + blob refs trailer).
        let (plaintext, _content_hash) = match encode_snapshot_with_blob_refs_to_v3_bytes(
            &clipboard_intent.snapshot,
            &blob_refs,
        ) {
            Ok(encoded) => encoded,
            Err(err) => {
                warn!(error = %err, "pull serve: V3 envelope encode failed");
                return Err(ActiveClipboardPullServeError::Internal(format!(
                    "encode envelope: {err}"
                )));
            }
        };

        // 5. Encrypt with a fresh transfer identity. A locked session surfaces
        //    here as NotUnlocked — the holder cannot serve while locked.
        match self.cipher.encrypt(&plaintext).await {
            Ok(ciphertext) => {
                info!(
                    envelope_len = ciphertext.len(),
                    blob_ref_count = blob_refs.len(),
                    "pull serve: produced transfer envelope"
                );
                Ok(ciphertext)
            }
            Err(TransferCipherError::NotUnlocked) => {
                debug!("pull serve: session locked; cannot encrypt");
                Err(ActiveClipboardPullServeError::NotUnlocked)
            }
            Err(err) => {
                warn!(error = %err, "pull serve: transfer cipher failed");
                Err(ActiveClipboardPullServeError::Internal(format!(
                    "encrypt: {err}"
                )))
            }
        }
    }
}

#[async_trait]
impl ActiveClipboardPullServePort for ActiveClipboardPullServeUseCase {
    async fn serve(&self, content_hash: &str) -> Result<Vec<u8>, ActiveClipboardPullServeError> {
        ActiveClipboardPullServeUseCase::serve(self, content_hash).await
    }
}

/// Map a snapshot-reconstruction failure to the serve error surface. Every
/// "cannot materialize" variant becomes `NotAvailable` (the payload is gone /
/// unresolvable on this holder); only a raw repository error is `Internal`.
fn map_reconstruct_error(
    err: BuildSnapshotError,
    entry_id: &EntryId,
) -> ActiveClipboardPullServeError {
    match err {
        BuildSnapshotError::Repository(inner) => {
            warn!(error = %inner, entry_id = %entry_id, "pull serve: snapshot reconstruct repository error");
            ActiveClipboardPullServeError::Internal(inner.to_string())
        }
        other => {
            debug!(error = %other, entry_id = %entry_id, "pull serve: content not materializable");
            ActiveClipboardPullServeError::NotAvailable
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use uc_core::blob::ports::BlobReaderPort;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelection, ClipboardSelectionDecision,
        MimeType, PayloadAvailability, PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::{
        ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort, GetClipboardEntryPort,
        GetRepresentationPort, PayloadResolveError, ProcessingUpdateOutcome,
        ResolvedClipboardPayload, UpdateRepresentationProcessingResultPort,
    };
    use uc_core::settings::model::Settings;
    use uc_core::BlobId;

    use crate::facade::{
        BlobTransferError, PublishBlobCommand, PublishBlobPathCommand, PublishBlobResult,
    };

    // ── fakes ────────────────────────────────────────────────────────────

    struct FixedLookup(Option<EntryId>);
    #[async_trait]
    impl FindEntryIdBySnapshotHashPort for FixedLookup {
        async fn find_entry_id_by_snapshot_hash(
            &self,
            _hash: &str,
        ) -> Result<Option<EntryId>, ClipboardRepositoryError> {
            Ok(self.0.clone())
        }
    }

    struct FakeEntryRepo {
        entry: Option<ClipboardEntry>,
    }
    #[async_trait]
    impl GetClipboardEntryPort for FakeEntryRepo {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(self.entry.clone())
        }
    }

    struct FakeSelectionRepo {
        selection: Option<ClipboardSelectionDecision>,
    }
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for FakeSelectionRepo {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            Ok(self.selection.clone())
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    struct StaticRepRepo {
        reps: Vec<PersistedClipboardRepresentation>,
    }
    #[async_trait]
    impl GetRepresentationPort for StaticRepRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self
                .reps
                .iter()
                .find(|r| r.id == *representation_id)
                .cloned())
        }
    }

    struct StubProcessingRepo;
    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for StubProcessingRepo {
        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            Ok(ProcessingUpdateOutcome::StateMismatch)
        }
    }

    enum ResolveBehavior {
        Inline(Vec<u8>),
        Lost,
    }
    struct StubResolver(ResolveBehavior);
    #[async_trait]
    impl ClipboardPayloadResolverPort for StubResolver {
        async fn resolve(
            &self,
            rep: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            match &self.0 {
                ResolveBehavior::Inline(bytes) => Ok(ResolvedClipboardPayload::Inline {
                    mime: rep
                        .mime_type
                        .as_ref()
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default(),
                    bytes: bytes.clone(),
                }),
                ResolveBehavior::Lost => Err(PayloadResolveError::Lost {
                    rep_id: rep.id.clone(),
                    reason: "synthetic lost".to_string(),
                }),
            }
        }
    }

    struct UnusedBlobStore;
    #[async_trait]
    impl BlobReaderPort for UnusedBlobStore {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!("blob store get must not be called for text-only snapshots")
        }
    }

    struct StubSettings;
    #[async_trait]
    impl SettingsPort for StubSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            Ok(Settings::default())
        }
        async fn save(&self, _s: &Settings) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    struct UnusedPublishGateway;
    #[async_trait]
    impl OutboundBlobPublishGateway for UnusedPublishGateway {
        async fn publish_blob(
            &self,
            _command: PublishBlobCommand,
        ) -> Result<PublishBlobResult, BlobTransferError> {
            unreachable!("publish_blob must not be called for a small text snapshot")
        }
        async fn publish_blob_path(
            &self,
            _command: PublishBlobPathCommand,
        ) -> Result<PublishBlobResult, BlobTransferError> {
            unreachable!("publish_blob_path must not be called for a text snapshot")
        }
    }

    /// Cipher that records the plaintext it was handed and returns a canned
    /// result, so a test can assert "encrypt ran on the freshly-encoded V3
    /// envelope" and exercise the NotUnlocked branch.
    struct StubCipher {
        result: std::sync::Mutex<Option<Result<Vec<u8>, TransferCipherError>>>,
        seen_plaintext: std::sync::Mutex<Option<Vec<u8>>>,
    }
    impl StubCipher {
        fn new(result: Result<Vec<u8>, TransferCipherError>) -> Arc<Self> {
            Arc::new(Self {
                result: std::sync::Mutex::new(Some(result)),
                seen_plaintext: std::sync::Mutex::new(None),
            })
        }
    }
    #[async_trait]
    impl TransferCipherPort for StubCipher {
        async fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>, TransferCipherError> {
            *self.seen_plaintext.lock().unwrap() = Some(plaintext.to_vec());
            self.result
                .lock()
                .unwrap()
                .take()
                .expect("encrypt called more than once")
        }
        async fn decrypt(&self, _encrypted: &[u8]) -> Result<Vec<u8>, TransferCipherError> {
            unreachable!("serve never decrypts")
        }
    }

    // ── helpers ──────────────────────────────────────────────────────────

    fn text_rep(id: &str, bytes: &[u8]) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            bytes.len() as i64,
            Some(bytes.to_vec()),
            None,
        )
    }

    fn entry_with_event(entry_id: &EntryId, event_id: &EventId) -> ClipboardEntry {
        ClipboardEntry::new(entry_id.clone(), event_id.clone(), 0, None, 0)
    }

    fn selection_for(entry_id: &EntryId, paste_rep_id: &str) -> ClipboardSelectionDecision {
        let paste = RepresentationId::from(paste_rep_id);
        ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: paste.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: paste.clone(),
                paste_rep_id: paste,
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    /// Build a serve use case whose local entry resolves to a text snapshot,
    /// with an injectable resolver behavior + cipher.
    fn build_uc(
        lookup: Option<EntryId>,
        resolve: ResolveBehavior,
        cipher: Arc<dyn TransferCipherPort>,
    ) -> ActiveClipboardPullServeUseCase {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("evt-1");
        let reconstructor = SnapshotReconstructor::new(
            Arc::new(FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            }),
            Arc::new(FakeSelectionRepo {
                selection: Some(selection_for(&entry_id, "rep-1")),
            }),
            Arc::new(StaticRepRepo {
                reps: vec![text_rep("rep-1", b"hello pull")],
            }),
            Arc::new(StubProcessingRepo),
            Arc::new(StubResolver(resolve)),
            Arc::new(UnusedBlobStore),
        );
        ActiveClipboardPullServeUseCase::new(ActiveClipboardPullServeDeps {
            entry_lookup: Arc::new(FixedLookup(lookup)),
            reconstructor,
            settings: Arc::new(StubSettings),
            blob_publisher: Arc::new(UnusedPublishGateway),
            cipher,
        })
    }

    // ── verdicts ─────────────────────────────────────────────────────────

    /// V1 — content held + unlocked: the chain reconstructs, encodes a V3
    /// envelope, and encrypts it; the returned bytes are the cipher output and
    /// the cipher was handed a non-empty V3 plaintext.
    #[tokio::test]
    async fn serves_envelope_for_held_content() {
        let cipher = StubCipher::new(Ok(b"CIPHERTEXT".to_vec()));
        let uc = build_uc(
            Some(EntryId::from("entry-1")),
            ResolveBehavior::Inline(b"hello pull".to_vec()),
            Arc::clone(&cipher) as _,
        );

        let envelope = uc.serve("blake3v1:whatever").await.expect("serve ok");
        assert_eq!(envelope, b"CIPHERTEXT");

        // The cipher was handed the V3 envelope *plaintext* (the
        // `ClipboardBinaryPayload` encoding the inbound decode path accepts) —
        // the `UC3\0` chunked-transfer magic is added inside the real cipher,
        // not here. Prove the plaintext is a decodable V3 envelope carrying the
        // original text rep, i.e. the chain ran decrypt → re-encode → encrypt.
        let seen = cipher.seen_plaintext.lock().unwrap().clone().unwrap();
        let snapshot =
            crate::usecases::clipboard_sync::payload_codec::decode_v3_bytes_to_snapshot(&seen)
                .expect("encrypted plaintext must be a valid V3 envelope");
        assert_eq!(snapshot.representations.len(), 1);
        assert_eq!(
            snapshot.representations[0].inline_bytes(),
            Some(b"hello pull".as_slice())
        );
    }

    /// V2 — content not held: lookup miss → NotAvailable, the cipher is never
    /// reached.
    #[tokio::test]
    async fn unheld_content_is_not_available() {
        let cipher = StubCipher::new(Ok(b"unused".to_vec()));
        let uc = build_uc(
            None,
            ResolveBehavior::Inline(vec![]),
            Arc::clone(&cipher) as _,
        );

        let err = uc
            .serve("blake3v1:missing")
            .await
            .expect_err("missing content must not serve");
        assert!(matches!(err, ActiveClipboardPullServeError::NotAvailable));
        assert!(cipher.seen_plaintext.lock().unwrap().is_none());
    }

    /// V3 — locked session: the transfer cipher reports NotUnlocked → the use
    /// case surfaces NotUnlocked (never leaks plaintext, never panics).
    #[tokio::test]
    async fn locked_session_surfaces_not_unlocked() {
        let cipher = StubCipher::new(Err(TransferCipherError::NotUnlocked));
        let uc = build_uc(
            Some(EntryId::from("entry-1")),
            ResolveBehavior::Inline(b"hello pull".to_vec()),
            Arc::clone(&cipher) as _,
        );

        let err = uc
            .serve("blake3v1:whatever")
            .await
            .expect_err("locked session must not serve");
        assert!(matches!(err, ActiveClipboardPullServeError::NotUnlocked));
    }

    /// V4 — payload lost (resolver returns Lost): reconstruct fails → the use
    /// case maps it to NotAvailable rather than Internal.
    #[tokio::test]
    async fn lost_payload_is_not_available() {
        let cipher = StubCipher::new(Ok(b"unused".to_vec()));
        let uc = build_uc(
            Some(EntryId::from("entry-1")),
            ResolveBehavior::Lost,
            Arc::clone(&cipher) as _,
        );

        let err = uc
            .serve("blake3v1:whatever")
            .await
            .expect_err("lost payload must not serve");
        assert!(matches!(err, ActiveClipboardPullServeError::NotAvailable));
        assert!(cipher.seen_plaintext.lock().unwrap().is_none());
    }
}
