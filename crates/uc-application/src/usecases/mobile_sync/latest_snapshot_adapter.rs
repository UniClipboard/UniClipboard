//! `LatestClipboardSnapshotAdapter` — adapter for the mobile-sync outbound read
//! path.
//!
//! Wires `LatestClipboardSnapshotPort` (`uc-core`) onto the existing clipboard
//! pipeline ports, composing "the paste-priority rep + bytes of the currently
//! active content (the active-clipboard register)".
//!
//! ## Data flow
//!
//! ```text
//! latest_paste_representation()
//!   ↓ active_register_load.load() — get the local entry_id of the active content
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
//! `snapshot_hash` is taken from the active register's current value (the stable
//! cross-device identity), independent of the byte-content hash; the upper layer
//! serializes it to the wire `contentId`.
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
    is_plain_text_mime_or_format, ClipboardEntry, ClipboardSelectionDecision,
    PersistedClipboardRepresentation,
};
use uc_core::ids::{EntryId, EventId, RepresentationId};
use uc_core::mobile_sync::LatestPasteRepresentation;
use uc_core::ports::clipboard::{
    ClipboardPayloadResolverPort, ClipboardSelectionRepositoryPort, GetClipboardEntryPort,
    GetRepresentationPort, LoadActiveClipboardPort, ResolvedClipboardPayload,
};
use uc_core::ports::mobile_sync::{LatestClipboardSnapshotError, LatestClipboardSnapshotPort};
use uc_core::MimeType;

/// Bundle of ports used to construct [`LatestClipboardSnapshotAdapter`].
///
/// Pulled out into its own type so `MobileSyncFacadeDeps` does not hang a whole
/// row of parallel ports directly; the split makes "what this snapshot path
/// needs" obvious at the call site.
///
/// The outbound read is anchored on the active-clipboard register:
/// `active_register_load` gives the local `entry_id` of the "currently active
/// content", `entry_repo` fills in the entry (carrying `event_id`) from it, and
/// the downstream selection / representation / blob materialization chain is
/// source-agnostic.
///
/// `pub` rather than `pub(crate)`: bootstrap uses this struct directly at the
/// facade assembly point, but since this file lives under
/// `pub(crate) mod latest_snapshot_adapter` it is only reachable indirectly via
/// the facade-layer re-export — still honoring the §11.4 boundary.
pub struct MobileSyncSnapshotPorts {
    pub active_register_load: Arc<dyn LoadActiveClipboardPort>,
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

    /// Step 1+2: resolve the entry of the "currently active content" and its
    /// corresponding selection decision.
    ///
    /// The anchor is the active-clipboard register: `load` gives the local
    /// `entry_id` of the active content (the register advances ⟺ the content was
    /// materialized into a local entry, so this id is always valid), and
    /// `get_entry` fills in the full entry (carrying `event_id`) from it for the
    /// downstream representation lookup.
    ///
    /// Any step yielding empty (register never written / entry no longer exists
    /// / no selection) → `Ok(None)`, consistent with the existing `NotFound`
    /// translation. Port-layer errors are uniformly translated to `Resolution`.
    async fn load_entry_and_selection(
        &self,
    ) -> Result<
        Option<(ClipboardEntry, ClipboardSelectionDecision, String)>,
        LatestClipboardSnapshotError,
    > {
        let state = self
            .ports
            .active_register_load
            .load()
            .await
            .map_err(|e| LatestClipboardSnapshotError::Resolution(e.to_string()))?;
        let Some(state) = state else {
            return Ok(None);
        };

        let entry = self
            .ports
            .entry_repo
            .get_entry(&state.entry_id)
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
        // `snapshot_hash` is this entry's stable cross-device identity, read
        // alongside the active register's current value; it is carried with the
        // materialized result to the upper layer for serialization into the wire
        // `contentId`.
        Ok(Some((entry, decision, state.snapshot_hash)))
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

    async fn latest_plain_text_preferred_representation(
        &self,
    ) -> Result<Option<LatestPasteRepresentation>, LatestClipboardSnapshotError> {
        let Some((entry, decision, snapshot_hash)) = self.load_entry_and_selection().await? else {
            return Ok(None);
        };
        let paste_rep_id = decision.selection.paste_rep_id.clone();

        // 候选顺序: paste 优先(若它本身就是 plaintext, 一次 IO 直接命中);
        // 再依次扫 primary 与 secondary 中其余的 rep。policy v1 下 primary 与
        // paste 同一份, 这里靠去重短路; 但代码不再依赖该等式 —— 未来若 v2
        // 让 primary ≠ paste, 本方法仍能正确扫描全部候选。
        let mut candidates: Vec<RepresentationId> =
            Vec::with_capacity(2 + decision.selection.secondary_rep_ids.len());
        let push_unique = |id: RepresentationId, list: &mut Vec<RepresentationId>| {
            if !list.contains(&id) {
                list.push(id);
            }
        };
        push_unique(paste_rep_id.clone(), &mut candidates);
        push_unique(decision.selection.primary_rep_id.clone(), &mut candidates);
        for sid in &decision.selection.secondary_rep_ids {
            push_unique(sid.clone(), &mut candidates);
        }

        // 扫描时缓存 paste rep —— 找不到 plaintext 时直接复用, 避免二次 IO。
        let mut paste_rep_cached: Option<PersistedClipboardRepresentation> = None;
        for rep_id in &candidates {
            let Some(rep) = self.fetch_representation(&entry.event_id, rep_id).await? else {
                continue;
            };
            if is_plain_text_mime_or_format(rep.mime_type.as_ref(), &rep.format_id) {
                return Ok(Some(
                    self.materialize(entry.entry_id, snapshot_hash, rep).await?,
                ));
            }
            if rep_id == &paste_rep_id {
                paste_rep_cached = Some(rep);
            }
        }

        // 无 plaintext rep —— 回退到 paste rep(可能是 text/rtf / text/html /
        // image 等), 由消费方按 mime 自己处理。
        let Some(paste_rep) = paste_rep_cached else {
            return Ok(None);
        };
        Ok(Some(
            self.materialize(entry.entry_id, snapshot_hash, paste_rep)
                .await?,
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
    //! plaintext-preference (latest_plain_text_preferred_representation)
    //! incremental coverage:
    //!
    //! | input | expected |
    //! |---|---|
    //! | paste is itself text/plain | use paste directly, do not read secondary |
    //! | paste is text/rtf, secondary has text/plain | switch to the plaintext rep |
    //! | paste is text/html, secondary all non-plaintext | fall back to the paste rep |
    //! | paste is image, no secondary | use the paste rep directly |
    //! | format_id=text but mime=None | treated as plaintext (via the format_id fallback) |
    //! | paste rep row missing but secondary has plaintext | return the plaintext secondary |

    use super::*;

    use anyhow::{anyhow, Result as AnyResult};
    use async_trait::async_trait;
    use std::sync::Mutex;

    use uc_core::clipboard::{
        ActiveClipboardState, ClipboardEntry, ClipboardRepositoryError, ClipboardSelection,
        ClipboardSelectionDecision, MimeType, PersistedClipboardRepresentation,
        SelectionPolicyVersion,
    };
    use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, RepresentationId};
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
        load: Mutex<Option<Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError>>>,
        get: Mutex<Option<Result<Option<ClipboardEntry>, ClipboardRepositoryError>>>,
    }
    impl FakeSource {
        fn build(
            load: Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError>,
            get: Result<Option<ClipboardEntry>, ClipboardRepositoryError>,
        ) -> Arc<Self> {
            Arc::new(Self {
                load: Mutex::new(Some(load)),
                get: Mutex::new(Some(get)),
            })
        }
        fn state_for(entry_id: EntryId) -> ActiveClipboardState {
            ActiveClipboardState::new("blake3v1:test", entry_id, 1, DeviceId::new("dev-test"))
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
        /// load succeeds but the entry lookup fails.
        fn get_err(msg: &str) -> Arc<Self> {
            Self::build(
                Ok(Some(Self::state_for(EntryId::from("e1")))),
                Err(ClipboardRepositoryError::Storage(msg.to_string())),
            )
        }
    }
    #[async_trait]
    impl LoadActiveClipboardPort for FakeSource {
        async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
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
        ClipboardEntry::new(EntryId::from(id), EventId::from(event), 1, None, 0)
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
            active_register_load: source.clone(),
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

    // ─── plaintext-preferred path ───────────────────────────────────────
    //
    // 这条路径会反复读 representation_repo / resolver, 单次返回的旧 fake 顶不
    // 住,这里另起一套按 RepresentationId 路由的 fake。每个 rep_id 注册一份
    // (metadata, resolved-payload) 配对, fake 各自挑各自要的部分。

    use std::collections::HashMap;

    /// 按 rep_id 路由的 RepresentationRepo —— 注册哪些 rep 存在 / 各自的
    /// metadata, 调用次数不限。
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

    /// 按 rep.id 路由的 Resolver —— 用 rep_id 找对应的 ResolvedClipboardPayload。
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

    fn selection_with_secondary(
        entry_id: &str,
        paste_rep: &str,
        secondary: &[&str],
    ) -> ClipboardSelectionDecision {
        let paste = RepresentationId::from(paste_rep);
        ClipboardSelectionDecision::new(
            EntryId::from(entry_id),
            ClipboardSelection {
                primary_rep_id: paste.clone(),
                secondary_rep_ids: secondary
                    .iter()
                    .map(|s| RepresentationId::from(*s))
                    .collect(),
                preview_rep_id: paste.clone(),
                paste_rep_id: paste,
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    #[tokio::test]
    async fn plain_text_pref_uses_paste_when_paste_is_plain_text() {
        // paste rep 本身就是 text/plain → 一次命中, 不需要扫 secondary。
        let plain = rep("r-plain", "text", Some("text/plain"));
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-plain",
                &[],
            )))),
            Arc::new(FakeRepRepoById::new(vec![plain])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-plain"),
                ResolvedClipboardPayload::Inline {
                    mime: "text/plain".into(),
                    bytes: b"hello".to_vec(),
                },
            )])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("text"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("text/plain"));
        assert_eq!(out.bytes, b"hello".to_vec());
    }

    #[tokio::test]
    async fn plain_text_pref_swaps_rtf_paste_for_plain_text_secondary() {
        // paste 是 text/rtf, secondary 有 text/plain → 切到 plaintext rep。
        // 这是修复的关键路径: 移动端不再收到 `{\rtf1\ansi...}` 字节流。
        let rtf = rep("r-rtf", "rtf", Some("text/rtf"));
        let plain = rep("r-plain", "text", Some("text/plain"));
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-rtf",
                &["r-plain"],
            )))),
            Arc::new(FakeRepRepoById::new(vec![rtf, plain])),
            Arc::new(FakeResolverById::new(vec![
                (
                    RepresentationId::from("r-rtf"),
                    ResolvedClipboardPayload::Inline {
                        mime: "text/rtf".into(),
                        bytes: b"{\\rtf1\\ansi hello}".to_vec(),
                    },
                ),
                (
                    RepresentationId::from("r-plain"),
                    ResolvedClipboardPayload::Inline {
                        mime: "text/plain".into(),
                        bytes: b"hello".to_vec(),
                    },
                ),
            ])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("text"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("text/plain"));
        assert_eq!(out.bytes, b"hello".to_vec());
    }

    #[tokio::test]
    async fn plain_text_pref_falls_back_to_paste_when_no_plain_text_available() {
        // paste 是 text/html, secondary 也是 text/html (无 plaintext) → 兜底
        // 用 paste rep 本身, 与 latest_paste_representation 行为一致。
        let html_paste = rep("r-html", "html", Some("text/html"));
        let html_alt = rep("r-html-alt", "html", Some("text/html"));
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-html",
                &["r-html-alt"],
            )))),
            Arc::new(FakeRepRepoById::new(vec![html_paste, html_alt])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-html"),
                ResolvedClipboardPayload::Inline {
                    mime: "text/html".into(),
                    bytes: b"<p>hi</p>".to_vec(),
                },
            )])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("html"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("text/html"));
        assert_eq!(out.bytes, b"<p>hi</p>".to_vec());
    }

    #[tokio::test]
    async fn plain_text_pref_keeps_image_paste_when_no_secondary() {
        // paste 是 image, 没 secondary → 直接用 paste rep。Image rep 不会被
        // 误判为 plaintext, 行为与 latest_paste_representation 一致。
        let img = rep("r-img", "image", Some("image/png"));
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-img",
                &[],
            )))),
            Arc::new(FakeRepRepoById::new(vec![img])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-img"),
                ResolvedClipboardPayload::BlobRef {
                    mime: "image/png".into(),
                    blob_id: BlobId::from("blob-img"),
                },
            )])),
            Arc::new(FakeBlobReader::ok(vec![0x89, 0x50, 0x4E, 0x47])),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("image"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("image/png"));
        assert_eq!(out.bytes, vec![0x89, 0x50, 0x4E, 0x47]);
    }

    #[tokio::test]
    async fn plain_text_pref_recognizes_format_id_text_without_mime() {
        // 没有显式 mime, 但 format_id="text" → 走 is_plain_text_mime_or_format
        // 的 format_id 兜底分支, 仍然识别为 plaintext。
        let no_mime = rep("r-text-only", "text", None);
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-text-only",
                &[],
            )))),
            Arc::new(FakeRepRepoById::new(vec![no_mime])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-text-only"),
                ResolvedClipboardPayload::Inline {
                    mime: "".into(),
                    bytes: b"hi".to_vec(),
                },
            )])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("text"));
        assert!(out.mime.is_none());
        assert_eq!(out.bytes, b"hi".to_vec());
    }

    #[tokio::test]
    async fn plain_text_pref_returns_secondary_plain_text_when_paste_rep_row_missing() {
        // 边界: selection 指向的 paste_rep_id 在 representation_repo 里查不到
        // (Ok(None)), 但 secondary 中存在 plaintext rep。
        //
        // 与 latest_paste_representation 的语义差异: 后者一旦 paste rep 查
        // 不到就直接 Ok(None); 而 plaintext 偏好入口的目标是"尽量给出可读
        // 纯文本", 因此即便 paste rep 行缺失, secondary 里有 plaintext 也
        // 应当返回它。该测试锁定这条语义不被无意改回去。
        //
        // 注: FakeRepRepoById 只注册 plaintext rep, 不注册 paste rep ——
        // 模拟 paste 行被外部清理 / 还未落库的场景。
        let plain = rep("r-plain", "text", Some("text/plain"));
        let adapter = build_adapter(
            FakeSource::with_entry(entry("e1", "ev1")),
            Arc::new(FakeSelectionRepo::ok(Some(selection_with_secondary(
                "e1",
                "r-missing-paste",
                &["r-plain"],
            )))),
            Arc::new(FakeRepRepoById::new(vec![plain])),
            Arc::new(FakeResolverById::new(vec![(
                RepresentationId::from("r-plain"),
                ResolvedClipboardPayload::Inline {
                    mime: "text/plain".into(),
                    bytes: b"hello".to_vec(),
                },
            )])),
            dummy_blob_reader(),
        );
        let out = adapter
            .latest_plain_text_preferred_representation()
            .await
            .unwrap()
            .unwrap();
        assert_eq!(out.format_id, FormatId::from("text"));
        assert_eq!(out.mime.as_ref().map(|m| m.as_str()), Some("text/plain"));
        assert_eq!(out.bytes, b"hello".to_vec());
    }
}
