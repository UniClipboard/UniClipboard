//! `LatestClipboardSnapshotAdapter` — adapter for the mobile-sync outbound read
//! path.
//!
//! Wires `LatestClipboardSnapshotPort` (`uc-core`) onto the existing clipboard
//! pipeline ports, composing "the paste-priority rep + bytes of the most recent
//! mobile-consumable content".
//!
//! ## Data flow
//!
//! ```text
//! latest_paste_representation()
//!   ↓ mobile_consumable_load.load_mobile_consumable() — get the fallback-safe content ref
//!   ↓ get_entry(entry_id) — fill in the entry (carries event_id)
//! ClipboardEntry { entry_id, event_id }
//!   ↓ get_selection(entry_id) — get paste_rep_id
//! ClipboardSelectionDecision.selection.paste_rep_id
//!   ↓ get_representation(event_id, paste_rep_id)
//! PersistedClipboardRepresentation { format_id, mime, inline_data | blob_id }
//!   ↓ payload_resolver.resolve(rep)
//! ResolvedClipboardPayload::Inline { mime, bytes } | BlobRef { mime, blob_id }
//!   ↓ (BlobRef branch) blob_reader.get(blob_id)
//! Vec<u8>
//!   ↓
//! LatestPasteRepresentation { entry_id, snapshot_hash, format_id, mime, bytes }
//! ```
//! `snapshot_hash` is taken from the mobile-consumable reference, independent of
//! the byte-content hash; the upper layer serializes it to the wire `contentId`.
//!
//! ## Boundary & error policy
//!
//! - **Any intermediate step yields no data** (no entry / no selection / no
//!   representation) → return `Ok(None)`; the facade translates it to
//!   `NotFound` → route 404.
//! - **An underlying port errors** (repo failure / blob unreadable / corrupt
//!   payload_state) → return `Err(Resolution(...))`, route 500.
//! - This policy matches the existing NotFound-vs-Port split in
//!   [`crate::usecases::mobile_sync::get_latest_doc`] /
//!   [`crate::usecases::mobile_sync::get_file`] — the use-case layer no longer
//!   re-decides "is it None or Err".
//!
//! ## Visibility
//!
//! `pub(crate)`. Per `uc-application/AGENTS.md` §11.4, the adapter is not exposed
//! to external crates; bootstrap passes the ports in via `MobileSyncFacadeDeps`
//! when assembling `MobileSyncFacade`, and the facade constructs this adapter
//! internally to inject into the use case.

use std::sync::Arc;

use async_trait::async_trait;

use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::{
    ClipboardEntry, ClipboardSelectionDecision, PersistedClipboardRepresentation,
};
use uc_core::ids::{EntryId, EventId, RepresentationId};
use uc_core::mobile_sync::LatestPasteRepresentation;
use uc_core::ports::clipboard::{
    ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort, GetClipboardEntryPort,
    GetRepresentationPort, LoadMobileConsumableClipboardPort, ResolvedClipboardPayload,
};
use uc_core::ports::mobile_sync::{LatestClipboardSnapshotError, LatestClipboardSnapshotPort};
use uc_core::MimeType;

/// Bundle of ports used to construct [`LatestClipboardSnapshotAdapter`].
///
/// Pulled out into its own type so `MobileSyncFacadeDeps` does not hang a whole
/// row of parallel ports directly; the split makes "what this snapshot path
/// needs" obvious at the call site.
///
/// The outbound read is anchored on the mobile-consumable reference. It gives
/// the local `entry_id` and stable `snapshot_hash`; the downstream selection,
/// representation, and blob materialization chain is source-agnostic.
///
/// `pub` rather than `pub(crate)`: bootstrap uses this struct directly at the
/// facade assembly point, but since this file lives under
/// `pub(crate) mod latest_snapshot_adapter` it is only reachable indirectly via
/// the facade-layer re-export — still honoring the §11.4 boundary.
pub struct MobileSyncSnapshotPorts {
    pub mobile_consumable_load: Arc<dyn LoadMobileConsumableClipboardPort>,
    pub entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_repo: Arc<dyn GetRepresentationPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_reader: Arc<dyn BlobReaderPort>,
}

pub(crate) struct LatestClipboardSnapshotAdapter {
    ports: MobileSyncSnapshotPorts,
}

impl LatestClipboardSnapshotAdapter {
    pub(crate) fn new(ports: MobileSyncSnapshotPorts) -> Self {
        Self { ports }
    }

    /// Step 1+2: resolve the most recent mobile-consumable entry and its
    /// corresponding selection decision.
    ///
    /// The anchor is the encrypted mobile-consumable reference. `get_entry`
    /// fills in the full entry (carrying `event_id`) for downstream lookup.
    ///
    /// Any step yielding empty (no consumable reference / entry removed / no
    /// selection) returns `Ok(None)`. A locked reference also returns `Ok(None)`;
    /// other port failures are translated to `Resolution`.
    async fn load_entry_and_selection(
        &self,
    ) -> Result<
        Option<(ClipboardEntry, ClipboardSelectionDecision, String)>,
        LatestClipboardSnapshotError,
    > {
        let reference = match self
            .ports
            .mobile_consumable_load
            .load_mobile_consumable()
            .await
        {
            Ok(reference) => reference,
            Err(uc_core::ports::clipboard::ActiveClipboardRegisterError::NotUnlocked) => {
                return Ok(None);
            }
            Err(err) => {
                return Err(LatestClipboardSnapshotError::Resolution(err.to_string()));
            }
        };
        let Some(reference) = reference else {
            return Ok(None);
        };

        let entry = self
            .ports
            .entry_repo
            .get_entry(&reference.entry_id)
            .await
            .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))?;
        let Some(entry) = entry else {
            return Ok(None);
        };

        let selection = self
            .ports
            .selection_repo
            .get_selection(&entry.entry_id)
            .await
            .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))?;
        let Some(decision) = selection else {
            return Ok(None);
        };
        // Carry the stable identity from the same fallback-safe reference used
        // to resolve the local entry.
        Ok(Some((entry, decision, reference.snapshot_hash)))
    }

    /// Step 3:按 (event_id, rep_id) 取出 representation,把 port 错统一翻成
    /// `Resolution`。
    async fn fetch_representation(
        &self,
        event_id: &EventId,
        rep_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>, LatestClipboardSnapshotError> {
        self.ports
            .representation_repo
            .get_representation(event_id, rep_id)
            .await
            .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))
    }

    /// Step 4-6:把 representation 解析成 `LatestPasteRepresentation`(物化
    /// bytes、推断 mime)。
    ///
    /// resolver 给空串 mime → `MimeType::None`,与 representation row
    /// `mime_type=NULL` 语义一致。
    async fn materialize(
        &self,
        entry_id: EntryId,
        snapshot_hash: String,
        rep: PersistedClipboardRepresentation,
    ) -> Result<LatestPasteRepresentation, LatestClipboardSnapshotError> {
        let format_id = rep.format_id.clone();

        let resolved = self
            .ports
            .payload_resolver
            .resolve(&rep)
            .await
            .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))?;
        let (mime_string, bytes) = match resolved {
            ResolvedClipboardPayload::Inline { mime, bytes } => (mime, bytes),
            ResolvedClipboardPayload::BlobRef { mime, blob_id } => {
                let bytes = self
                    .ports
                    .blob_reader
                    .get(&blob_id)
                    .await
                    .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))?;
                (mime, bytes)
            }
        };

        let mime = if mime_string.is_empty() {
            None
        } else {
            Some(MimeType(mime_string))
        };

        Ok(LatestPasteRepresentation {
            entry_id,
            snapshot_hash,
            format_id,
            mime,
            bytes,
        })
    }
}

#[async_trait]
impl LatestClipboardSnapshotPort for LatestClipboardSnapshotAdapter {
    async fn latest_paste_representation(
        &self,
    ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError> {
        let Some((entry, decision, snapshot_hash)) = self.load_entry_and_selection().await? else {
            return Ok(None);
        };
        let paste_rep_id = decision.selection.paste_rep_id.clone();

        let Some(rep) = self
            .fetch_representation(&entry.event_id, &paste_rep_id)
            .await?
        else {
            return Ok(None);
        };

        Ok(Some(
            self.materialize(entry.entry_id, snapshot_hash, rep).await?,
        ))
    }

    async fn latest_preview_representation(
        &self,
    ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError> {
        let Some((entry, decision, snapshot_hash)) = self.load_entry_and_selection().await? else {
            return Ok(None);
        };

        let Some(rep) = self
            .fetch_representation(&entry.event_id, &decision.selection.preview_rep_id)
            .await?
        else {
            return Ok(None);
        };

        Ok(Some(
            self.materialize(entry.entry_id, snapshot_hash, rep).await?,
        ))
    }
}

#[cfg(test)]
mod tests {
    //! Hand-written fake unit tests (avoiding mockall's awkward diagnostics for
    //! trait signatures carrying `&'_ T`).
    //!
    //! Coverage matrix:
    //!
    //! | input | expected |
    //! |---|---|
    //! | register empty (load None) | Ok(None) |
    //! | register points at a missing entry (get_entry None) | Ok(None) |
    //! | entry present + selection empty | Ok(None) |
    //! | entry present + selection present + rep missing | Ok(None) |
    //! | inline branch | Ok(Some(...)) |
    //! | blob_ref branch + reader success | Ok(Some(...)) |
    //! | inline mime empty string | Ok(Some(.., mime=None)) |
    //! | register load error | Err(Resolution) |
    //! | get_entry error | Err(Resolution) |
    //! | resolver error | Err(Resolution) |
    //! | blob_reader error | Err(Resolution) |
    //!
    //! latest_preview_representation incremental coverage:
    //!
    //! | input | expected |
    //! |---|---|
    //! | preview_rep_id differs from paste_rep_id (e.g. paste=html, preview=image) | resolve preview_rep_id, never touch paste_rep_id |
    //! | preview_rep_id row missing | Ok(None) |
    //!
    //! The relative ranking of representations (files > plain text > image >
    //! rich text > uri > unknown) is `SelectRepresentationPolicyV1`'s
    //! responsibility (`uc-core::clipboard::policy::v1`), not this adapter's —
    //! it is covered there.

    use super::*;

    use anyhow::{anyhow, Result as AnyResult};
    use async_trait::async_trait;
    use std::sync::Mutex;

    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelection, ClipboardSelectionDecision,
        MimeType, MobileConsumableRef, PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::{ActiveClipboardRegisterError, PayloadResolveError};
    use uc_core::BlobId;

    // ── Fake source: active register load + entry get ────────────────────
    //
    // The outbound read is anchored on the active register: `load()` gives the
    // `entry_id` of the currently active content, and `get_entry()` fills in the
    // entry (carrying `event_id`) from it. Both steps are merged into one fake,
    // configured once by "is there active content / is the entry fetchable"; each
    // method is consumed only once, and a second call panics to expose an
    // unexpected duplicate read.
    struct FakeSource {
        load: Mutex<Option<Result<Option<MobileConsumableRef>, ActiveClipboardRegisterError>>>,
        get: Mutex<Option<Result<Option<ClipboardEntry>, ClipboardRepositoryError>>>,
    }
    impl FakeSource {
        fn build(
            load: Result<Option<MobileConsumableRef>, ActiveClipboardRegisterError>,
            get: Result<Option<ClipboardEntry>, ClipboardRepositoryError>,
        ) -> Arc<Self> {
            Arc::new(Self {
                load: Mutex::new(Some(load)),
                get: Mutex::new(Some(get)),
            })
        }
        fn state_for(entry_id: EntryId) -> MobileConsumableRef {
            MobileConsumableRef::new("blake3v1:test", entry_id)
        }
        /// Active content exists and the entry is fetchable: `load` points at
        /// `e.entry_id` and `get_entry` returns that entry.
        fn with_entry(e: ClipboardEntry) -> Arc<Self> {
            let state = Self::state_for(e.entry_id.clone());
            Self::build(Ok(Some(state)), Ok(Some(e)))
        }
        /// Register was never written → `load` returns None.
        fn empty() -> Arc<Self> {
            Self::build(Ok(None), Ok(None))
        }
        /// Register points at an entry that no longer exists → `load` Some,
        /// `get_entry` None.
        fn entry_missing() -> Arc<Self> {
            Self::build(Ok(Some(Self::state_for(EntryId::from("e1")))), Ok(None))
        }
        /// The register load itself fails.
        fn load_err(msg: &str) -> Arc<Self> {
            Self::build(
                Err(ActiveClipboardRegisterError::Storage(msg.to_string())),
                Ok(None),
            )
        }
        fn locked() -> Arc<Self> {
            Self::build(Err(ActiveClipboardRegisterError::NotUnlocked), Ok(None))
        }
        /// load succeeds but the entry lookup fails.
        fn get_err(msg: &str) -> Arc<Self> {
            Self::build(
                Ok(Some(Self::state_for(EntryId::from("e1")))),
                Err(ClipboardRepositoryError::Storage(msg.to_string())),
            )
        }
    }
    #[async_trait]
    impl LoadMobileConsumableClipboardPort for FakeSource {
        async fn load_mobile_consumable(
            &self,
        ) -> Result<Option<MobileConsumableRef>, ActiveClipboardRegisterError> {
            self.load.lock().unwrap().take().expect("load 被调用多次")
        }
    }
    #[async_trait]
    impl GetClipboardEntryPort for FakeSource {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            self.get
                .lock()
                .unwrap()
                .take()
                .expect("get_entry 被调用多次")
        }
    }

    // ── Fake SelectionRepo ───────────────────────────────────────────────
    #[derive(Default)]
    struct FakeSelectionRepo {
        next: Mutex<Option<AnyResult<Option<ClipboardSelectionDecision>>>>,
    }
    impl FakeSelectionRepo {
        fn ok(decision: Option<ClipboardSelectionDecision>) -> Self {
            Self {
                next: Mutex::new(Some(Ok(decision))),
            }
        }
    }
    #[async_trait]
    impl ClipboardSelectionRepositoryPort for FakeSelectionRepo {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> AnyResult<Option<ClipboardSelectionDecision>> {
            self.next.lock().unwrap().take().expect("调用多次")
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> AnyResult<()> {
            unimplemented!()
        }
    }

    // ── Fake RepresentationRepo ──────────────────────────────────────────
    #[derive(Default)]
    struct FakeRepRepo {
        next: Mutex<
            Option<Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError>>,
        >,
    }
    impl FakeRepRepo {
        fn ok(rep: Option<PersistedClipboardRepresentation>) -> Self {
            Self {
                next: Mutex::new(Some(Ok(rep))),
            }
        }
    }
    #[async_trait]
    impl GetRepresentationPort for FakeRepRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            self.next.lock().unwrap().take().expect("调用多次")
        }
    }

    // ── Fake Resolver ────────────────────────────────────────────────────
    #[derive(Default)]
    struct FakeResolver {
        next: Mutex<Option<Result<ResolvedClipboardPayload, PayloadResolveError>>>,
    }
    impl FakeResolver {
        fn ok(payload: ResolvedClipboardPayload) -> Self {
            Self {
                next: Mutex::new(Some(Ok(payload))),
            }
        }
        fn err(msg: &str) -> Self {
            Self {
                next: Mutex::new(Some(Err(PayloadResolveError::Integrity {
                    rep_id: RepresentationId::from("test"),
                    reason: msg.to_string(),
                }))),
            }
        }
    }
    #[async_trait]
    impl ClipboardPayloadResolverPort for FakeResolver {
        async fn resolve(
            &self,
            _representation: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            self.next.lock().unwrap().take().expect("调用多次")
        }
    }

    // ── Fake BlobReader ──────────────────────────────────────────────────
    #[derive(Default)]
    struct FakeBlobReader {
        next: Mutex<Option<AnyResult<Vec<u8>>>>,
    }
    impl FakeBlobReader {
        fn ok(bytes: Vec<u8>) -> Self {
            Self {
                next: Mutex::new(Some(Ok(bytes))),
            }
        }
        fn err(msg: &str) -> Self {
            Self {
                next: Mutex::new(Some(Err(anyhow!("{}", msg.to_string())))),
            }
        }
    }
    #[async_trait]
    impl BlobReaderPort for FakeBlobReader {
        async fn get(&self, _blob_id: &BlobId) -> AnyResult<Vec<u8>> {
            self.next.lock().unwrap().take().expect("调用多次")
        }
    }

    // ── helpers ──────────────────────────────────────────────────────────
    fn entry(id: &str, event: &str) -> ClipboardEntry {
        ClipboardEntry::new(EntryId::from(id), EventId::from(event), 1, 0)
            .with_delivery_tracked(false)
    }

    fn selection(entry_id: &str, paste_rep: &str) -> ClipboardSelectionDecision {
        let rep = RepresentationId::from(paste_rep);
        ClipboardSelectionDecision::new(
            EntryId::from(entry_id),
            ClipboardSelection {
                primary_rep_id: rep.clone(),
                secondary_rep_ids: vec![],
                preview_rep_id: rep.clone(),
                paste_rep_id: rep,
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    fn rep(rep_id: &str, format: &str, mime: Option<&str>) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(rep_id),
            FormatId::from(format),
            mime.map(|s| MimeType(s.to_string())),
            0,
            Some(vec![0u8]),
            None,
        )
    }

    fn build_adapter(
        source: Arc<FakeSource>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
        blob_reader: Arc<dyn BlobReaderPort>,
    ) -> LatestClipboardSnapshotAdapter {
        // FakeSource doubles as both the register-load and entry-get ports.
        LatestClipboardSnapshotAdapter::new(MobileSyncSnapshotPorts {
            mobile_consumable_load: source.clone(),
            entry_repo: source,
            selection_repo,
            representation_repo,
            payload_resolver,
            blob_reader,
        })
    }

    fn dummy_blob_reader() -> Arc<dyn BlobReaderPort> {
        // 不应被调用 —— 用 default fake (next=None) 一旦被读 panic on take()。
        Arc::new(FakeBlobReader::default())
    }

    fn dummy_resolver() -> Arc<dyn ClipboardPayloadResolverPort> {
        Arc::new(FakeResolver::default())
    }

    fn dummy_rep_repo() -> Arc<dyn GetRepresentationPort> {
        Arc::new(FakeRepRepo::default())
    }

    fn dummy_selection_repo() -> Arc<dyn ClipboardSelectionRepositoryPort> {
        Arc::new(FakeSelectionRepo::default())
    }

    // ── tests ────────────────────────────────────────────────────────────
    #[tokio::test]
    async fn empty_register_returns_none() {
        // active register never written → load() == None → NotFound upstream.
        let adapter = build_adapter(
            FakeSource::empty(),
            dummy_selection_repo(),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn locked_register_returns_none() {
        let adapter = build_adapter(
            FakeSource::locked(),
            dummy_selection_repo(),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn entry_missing_returns_none() {
        // register points at an entry that no longer exists (get_entry == None)
        // → None, not an error.
        let adapter = build_adapter(
            FakeSource::entry_missing(),
            dummy_selection_repo(),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn register_load_error_propagates_as_resolution() {
        let adapter = build_adapter(
            FakeSource::load_err("simulated register read failure"),
            dummy_selection_repo(),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        let err = adapter.latest_paste_representation().await.unwrap_err();
        assert!(matches!(err, LatestClipboardSnapshotError::Resolution(_)));
    }

    #[tokio::test]
    async fn missing_selection_returns_none() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(None)),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn missing_representation_returns_none() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(None)),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn inline_path_round_trips_bytes_and_mime() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(Some(rep("r1", "text", Some("text/plain"))))),
            Arc::new(FakeResolver::ok(ResolvedClipboardPayload::Inline {
                mime: "text/plain".into(),
                bytes: b"hello".to_vec(),
            })),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.entry_id, EntryId::from("e1"));
        assert_eq!(out.format_id, FormatId::from("text"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("text/plain"));
        assert_eq!(out.bytes, b"hello".to_vec());
        // Stable identity comes from the active register's current value
        // (FakeSource::state_for).
        assert_eq!(out.snapshot_hash, "blake3v1:test");
    }

    #[tokio::test]
    async fn blob_ref_path_calls_reader_and_round_trips_bytes() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(Some(rep("r1", "image", Some("image/png"))))),
            Arc::new(FakeResolver::ok(ResolvedClipboardPayload::BlobRef {
                mime: "image/png".into(),
                blob_id: BlobId::from("blob-123"),
            })),
            Arc::new(FakeBlobReader::ok(vec![0x89, 0x50, 0x4E, 0x47])),
        );
        let out = adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("image"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("image/png"));
        assert_eq!(out.bytes, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[tokio::test]
    async fn empty_mime_string_falls_back_to_none() {
        // resolver 给空串 mime → 视作"无 mime",mapping 层走 Text 兜底。
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(Some(rep("r1", "text", None)))),
            Arc::new(FakeResolver::ok(ResolvedClipboardPayload::Inline {
                mime: "".into(),
                bytes: b"x".to_vec(),
            })),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_paste_representation()
            .await
            .unwrap()
            .unwrap();
        assert!(out.mime.is_none());
    }

    #[tokio::test]
    async fn entry_repo_error_propagates_as_resolution() {
        let adapter = build_adapter(
            FakeSource::get_err("sqlite simulated failure"),
            dummy_selection_repo(),
            dummy_rep_repo(),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        let err = adapter.latest_paste_representation().await.unwrap_err();
        assert!(matches!(err, LatestClipboardSnapshotError::Resolution(_)));
    }

    #[tokio::test]
    async fn resolver_error_propagates_as_resolution() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(Some(rep("r1", "text", Some("text/plain"))))),
            Arc::new(FakeResolver::err("payload state lost")),
            dummy_blob_reader(),
        );
        let err = adapter.latest_paste_representation().await.unwrap_err();
        assert!(matches!(err, LatestClipboardSnapshotError::Resolution(_)));
    }

    #[tokio::test]
    async fn blob_reader_error_propagates_as_resolution() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(Some(rep("r1", "image", Some("image/png"))))),
            Arc::new(FakeResolver::ok(ResolvedClipboardPayload::BlobRef {
                mime: "image/png".into(),
                blob_id: BlobId::from("blob-x"),
            })),
            Arc::new(FakeBlobReader::err("blob fs gone")),
        );
        let err = adapter.latest_paste_representation().await.unwrap_err();
        assert!(matches!(err, LatestClipboardSnapshotError::Resolution(_)));
    }

    // ─── preview (UiPreview-selected) representation ────────────────────
    //
    // This path resolves by `RepresentationId`, not a single fixed rep — the
    // one-shot fakes above can't route by id, so it gets its own fakes.

    use std::collections::HashMap;

    /// Routes `get_representation` by rep id — register whichever reps exist
    /// for the scenario, callable any number of times.
    struct FakeRepRepoById {
        reps: HashMap<RepresentationId, PersistedClipboardRepresentation>,
    }
    impl FakeRepRepoById {
        fn new(reps: Vec<PersistedClipboardRepresentation>) -> Self {
            Self {
                reps: reps.into_iter().map(|r| (r.id.clone(), r)).collect(),
            }
        }
    }
    #[async_trait]
    impl GetRepresentationPort for FakeRepRepoById {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self.reps.get(representation_id).cloned())
        }
    }

    /// Routes `resolve` by rep id to a registered `ResolvedClipboardPayload`.
    struct FakeResolverById {
        payloads: HashMap<RepresentationId, ResolvedClipboardPayload>,
    }
    impl FakeResolverById {
        fn new(payloads: Vec<(RepresentationId, ResolvedClipboardPayload)>) -> Self {
            Self {
                payloads: payloads.into_iter().collect(),
            }
        }
    }
    #[async_trait]
    impl ClipboardPayloadResolverPort for FakeResolverById {
        async fn resolve(
            &self,
            representation: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            self.payloads
                .get(&representation.id)
                .cloned()
                .ok_or_else(|| PayloadResolveError::Integrity {
                    rep_id: representation.id.clone(),
                    reason: "no payload registered for rep".into(),
                })
        }
    }

    #[tokio::test]
    async fn preview_resolves_preview_rep_id_not_paste_rep_id() {
        // Regression for issue #1210: a browser image copy leaves
        // `paste_rep_id` pointing at the DefaultPaste-selected text/html
        // markup (RichText outranks Image there, to preserve formatting on
        // local paste), while `preview_rep_id` — the same choice already
        // used to render the desktop UI — correctly points at the image/png
        // representation (UiPreview ranks Image above RichText). This method
        // must follow `preview_rep_id`.
        //
        // The html rep is deliberately left unregistered in
        // `FakeRepRepoById`: if the adapter regresses to reading
        // `paste_rep_id` again, the lookup returns `None` and this test
        // fails loudly instead of silently passing.
        let image = rep("r-image", "image", Some("image/png"));
        let selection = ClipboardSelectionDecision::new(
            EntryId::from("e1"),
            ClipboardSelection {
                primary_rep_id: RepresentationId::from("r-html"),
                secondary_rep_ids: vec![RepresentationId::from("r-image")],
                preview_rep_id: RepresentationId::from("r-image"),
                paste_rep_id: RepresentationId::from("r-html"),
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection))),
            Arc::new(FakeRepRepoById::new(vec![image])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-image"),
                ResolvedClipboardPayload::Inline {
                    mime: "image/png".into(),
                    bytes: vec![0x89, 0x50, 0x4E, 0x47],
                },
            )])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_preview_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("image"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("image/png"));
        assert_eq!(out.bytes, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[tokio::test]
    async fn preview_returns_none_when_representation_row_missing() {
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection("e1", "r1")))),
            Arc::new(FakeRepRepo::ok(None)),
            dummy_resolver(),
            dummy_blob_reader(),
        );
        assert!(adapter
            .latest_preview_representation()
            .await
            .unwrap()
            .is_none());
    }
}
