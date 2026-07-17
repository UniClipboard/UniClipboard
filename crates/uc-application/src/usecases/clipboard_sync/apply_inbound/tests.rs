use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use mockall::predicate::*;
use tracing::field::{Field, Visit};
use tracing::Subscriber;
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;

use crate::clipboard_write::ClipboardWriteIntent;
use crate::facade::host_event::{
    ClipboardHostEvent, ClipboardOriginKind, EmitError, HostEvent, HostEventBus,
    HostEventEmitterPort,
};

use uc_core::clipboard::ClipboardRepositoryError;
use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::blob::{BlobDigest, BlobTicket, PlaintextHash};
use uc_core::ports::clipboard::{FindEntryIdBySnapshotHashPort, TouchClipboardEntryPort};
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_observability::FlowId;

use crate::usecases::clipboard_sync::payload_codec::{
    encode_snapshot_to_v3_bytes, encode_snapshot_with_blob_refs_and_file_set_to_v3_bytes,
    encode_snapshot_with_blob_refs_to_v3_bytes, V3BlobRef,
};

use super::materializer::{
    compute_file_set_component, sweep_inbound_staging, FileCacheBlobMaterializer,
    InboundBlobFetcher, InboundBlobMaterializer, InboundFileSetManifest, InboundFileSetMember,
    MaterializeResult, RollbackOutcome,
};
use super::ports::{InboundCapture, InboundSnapshotRebuild, InboundWrite};
use super::usecase::verify_file_set_identity;
use super::usecase::ApplyInboundClipboardUseCase;
use super::{ApplyInboundError, ApplyInboundInput, ApplyOutcome};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use uc_core::ports::atomic_publish::{AtomicPublishPort, PublishError};

/// A publisher with the port's real semantics: it performs the move, and it
/// refuses a destination that already exists.
///
/// The native primitives are exercised against real filesystems in `uc-infra`.
/// This double exists so the orchestration above them — pre-flight, suffix
/// retry, multi-root rollback — can be driven through failures no real
/// filesystem produces on demand.
#[derive(Default)]
struct FakeAtomicPublisher {
    /// 1-based call ordinals that must fail, and how. Covers withdrawals too:
    /// a withdrawal is just another `publish_no_replace`.
    scripted: Mutex<std::collections::HashMap<usize, PublishError>>,
    /// When a scripted `DestinationExists` fires, also create the destination
    /// for real — modelling the racer that actually took the name.
    steal_on_conflict: AtomicBool,
    /// Volume boundaries are not simulable in a temp dir, so "this volume
    /// cannot publish atomically" is expressed as a path prefix.
    unsupported_under: Mutex<Option<PathBuf>>,
    /// Same, for the case where no volume in reach supports the primitive.
    unsupported_anywhere: AtomicBool,
    calls: AtomicUsize,
    probes: AtomicUsize,
}

impl FakeAtomicPublisher {
    fn failing_at(ordinals: &[(usize, PublishError)]) -> Arc<Self> {
        let this = Self::default();
        {
            let mut scripted = this.scripted.lock().unwrap();
            for (n, err) in ordinals {
                scripted.insert(*n, err.clone());
            }
        }
        Arc::new(this)
    }

    fn losing_a_race_at(ordinal: usize) -> Arc<Self> {
        let this = Self::failing_at(&[(ordinal, PublishError::DestinationExists)]);
        this.steal_on_conflict.store(true, Ordering::SeqCst);
        this
    }

    fn unsupported_under(dir: &Path) -> Arc<Self> {
        let this = Self::default();
        *this.unsupported_under.lock().unwrap() = Some(dir.to_path_buf());
        Arc::new(this)
    }

    /// Every volume in reach refuses the no-replace primitive — the exFAT /
    /// NFS install, where there is nowhere better to fall back to.
    fn unsupported_everywhere() -> Arc<Self> {
        let this = Self::default();
        this.unsupported_anywhere.store(true, Ordering::SeqCst);
        Arc::new(this)
    }

    fn publish_count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn probe_count(&self) -> usize {
        self.probes.load(Ordering::SeqCst)
    }

    /// Count this move and hand back the failure scripted for it, if any.
    /// Both publish variants are one sequence: an ordinal names the Nth move,
    /// whichever guarantee it asked for.
    fn next_scripted(&self, destination: &Path) -> Option<PublishError> {
        let ordinal = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let err = self.scripted.lock().unwrap().remove(&ordinal)?;
        if err == PublishError::DestinationExists && self.steal_on_conflict.load(Ordering::SeqCst) {
            std::fs::create_dir_all(destination).expect("racer takes the name");
        }
        Some(err)
    }
}

#[async_trait]
impl AtomicPublishPort for FakeAtomicPublisher {
    async fn publish_no_replace(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError> {
        if let Some(err) = self.next_scripted(destination) {
            return Err(err);
        }
        if destination.exists() {
            return Err(PublishError::DestinationExists);
        }
        std::fs::rename(source, destination).map_err(|e| PublishError::Io(e.to_string()))
    }

    async fn publish_into_free_name(
        &self,
        source: &Path,
        destination: &Path,
    ) -> Result<(), PublishError> {
        if let Some(err) = self.next_scripted(destination) {
            return Err(err);
        }
        // No existence check on purpose: this variant promises nothing about
        // an occupied destination, and mirroring the strict one here would
        // hide a caller that leans on a guarantee it did not ask for.
        std::fs::rename(source, destination).map_err(|e| PublishError::Io(e.to_string()))
    }

    async fn supports_no_replace(&self, probe_dir: &Path) -> bool {
        self.probes.fetch_add(1, Ordering::SeqCst);
        if self.unsupported_anywhere.load(Ordering::SeqCst) {
            return false;
        }
        match self.unsupported_under.lock().unwrap().as_ref() {
            Some(dir) => !probe_dir.starts_with(dir),
            None => true,
        }
    }
}

fn test_materializer(
    fetcher: Arc<dyn InboundBlobFetcher>,
    cache_dir: PathBuf,
) -> FileCacheBlobMaterializer {
    FileCacheBlobMaterializer::new(fetcher, cache_dir, Arc::new(FakeAtomicPublisher::default()))
}

/// Redirects inbound content into `dir` without claiming any name — the
/// directory-publish counterpart to `FakeUserDirReserver`.
struct FakeSaveDir {
    dir: PathBuf,
}

#[async_trait]
impl uc_core::ports::inbound_file_target::ResolveInboundSaveDirPort for FakeSaveDir {
    async fn resolve_save_dir(&self) -> Option<PathBuf> {
        Some(self.dir.clone())
    }
}

/// Names visible directly inside `dir`, excluding hidden staging areas.
fn visible_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read dir")
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| !name.starts_with(".uniclip-"))
        .collect();
    names.sort();
    names
}

// ── mockall: the 3 collaborator surfaces ────────────────────────────

#[derive(Clone)]
struct FlowIdRecordLayer {
    records: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for FlowIdRecordLayer
where
    S: Subscriber,
    S: for<'lookup> LookupSpan<'lookup>,
{
    fn on_record(
        &self,
        _span: &tracing::Id,
        values: &tracing::span::Record<'_>,
        _ctx: Context<'_, S>,
    ) {
        let mut visitor = FlowIdVisitor::default();
        values.record(&mut visitor);
        if let Some(flow_id) = visitor.flow_id {
            self.records.lock().unwrap().push(flow_id);
        }
    }
}

#[derive(Default)]
struct FlowIdVisitor {
    flow_id: Option<String>,
}

impl Visit for FlowIdVisitor {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        if field.name() == "flow.id" {
            self.flow_id = Some(format!("{value:?}"));
        }
    }
}

mockall::mock! {
    pub EntryRepo {}
    #[async_trait]
    impl FindEntryIdBySnapshotHashPort for EntryRepo {
        async fn find_entry_id_by_snapshot_hash(
            &self,
            snapshot_hash: &str,
        ) -> std::result::Result<Option<EntryId>, ClipboardRepositoryError>;
    }
}

mockall::mock! {
    pub Capture {}
    #[async_trait]
    impl InboundCapture for Capture {
        async fn capture(
            &self,
            preset_entry_id: EntryId,
            from_device: DeviceId,
            snapshot: SystemClipboardSnapshot,
        ) -> Result<Option<EntryId>>;
    }
}

mockall::mock! {
    pub Write {}
    #[async_trait]
    impl InboundWrite for Write {
        async fn write(&self, snapshot: SystemClipboardSnapshot, intent: ClipboardWriteIntent) -> Result<()>;
    }
}

mockall::mock! {
    pub SnapshotRebuild {}
    #[async_trait]
    impl InboundSnapshotRebuild for SnapshotRebuild {
        async fn rebuild(&self, entry_id: &EntryId) -> Result<SystemClipboardSnapshot>;
    }
}

mockall::mock! {
    pub TouchEntry {}
    #[async_trait]
    impl TouchClipboardEntryPort for TouchEntry {
        async fn touch_entry(
            &self,
            entry_id: &EntryId,
            active_time_ms: i64,
        ) -> std::result::Result<bool, ClipboardRepositoryError>;
    }
}

mockall::mock! {
    pub BlobMaterializer {}
    #[async_trait]
    impl InboundBlobMaterializer for BlobMaterializer {
        async fn materialize(
            &self,
            from_device: DeviceId,
            receiver_entry_id: EntryId,
            snapshot: SystemClipboardSnapshot,
            blob_refs: Vec<V3BlobRef>,
            file_set_manifest: Option<InboundFileSetManifest>,
        ) -> Result<MaterializeResult>;
    }
}

mockall::mock! {
    pub BlobFetcher {}
    #[async_trait]
    impl InboundBlobFetcher for BlobFetcher {
        async fn fetch_blob(
            &self,
            command: crate::facade::blob_transfer::FetchBlobCommand,
        ) -> Result<crate::facade::blob_transfer::FetchBlobResult>;

        async fn fetch_blob_to_path(
            &self,
            command: crate::facade::blob_transfer::FetchBlobToPathCommand,
        ) -> Result<crate::facade::blob_transfer::FetchBlobToPathResult>;

        async fn record_unfetched_failure(
            &self,
            context: crate::facade::blob_transfer::FetchTransferContext,
            detail: String,
        );
    }
}

// ── helpers ─────────────────────────────────────────────────────────

fn fixture_input(text: &str) -> (ApplyInboundInput, String) {
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            text.as_bytes().to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let (plaintext, snapshot_hash) = encode_snapshot_to_v3_bytes(&snapshot).unwrap();
    (
        ApplyInboundInput {
            from_device: DeviceId::new("peer-x"),
            snapshot_hash: snapshot_hash.clone(),
            plaintext,
            flow_id: None,
            resurface_intent: ClipboardWriteIntent::RemotePush,
        },
        snapshot_hash,
    )
}

#[test]
fn directory_identity_verification_rejects_mismatched_snapshot_hash() {
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: Some([1; 32]),
    };

    let error = verify_file_set_identity(&snapshot, "blake3v1:deadbeef")
        .expect_err("mismatched directory identity must fail");
    assert!(error.to_string().contains("identity mismatch"));
}

fn fixture_input_from_snapshot(snapshot: SystemClipboardSnapshot) -> (ApplyInboundInput, String) {
    let (plaintext, snapshot_hash) = encode_snapshot_to_v3_bytes(&snapshot).unwrap();
    (
        ApplyInboundInput {
            from_device: DeviceId::new("peer-x"),
            snapshot_hash: snapshot_hash.clone(),
            plaintext,
            flow_id: None,
            resurface_intent: ClipboardWriteIntent::RemotePush,
        },
        snapshot_hash,
    )
}

fn build(
    repo: MockEntryRepo,
    capture: MockCapture,
    write: MockWrite,
) -> ApplyInboundClipboardUseCase {
    ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
}

fn build_with_blob_materializer(
    repo: MockEntryRepo,
    capture: MockCapture,
    write: MockWrite,
    materializer: MockBlobMaterializer,
) -> ApplyInboundClipboardUseCase {
    ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_blob_materializer(Arc::new(materializer))
}

// ── verdicts ────────────────────────────────────────────────────────

// ── host-event recording fake ───────────────────────────────────────

/// 录制 emitter:把 emit() 收到的所有 HostEvent 按时间顺序追加到 Vec,
/// 让测试断言事件序列(尤其是 IncomingPending → NewContent 的顺序与
/// 内容)而不是依赖外部 broadcast / WS 链路。
#[derive(Default)]
struct RecordingEmitter {
    events: Mutex<Vec<HostEvent>>,
}

impl RecordingEmitter {
    fn snapshot(&self) -> Vec<HostEvent> {
        self.events.lock().unwrap().clone()
    }
}

impl HostEventEmitterPort for RecordingEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        self.events.lock().unwrap().push(event);
        Ok(())
    }
}

fn build_with_recording_emitter(
    repo: MockEntryRepo,
    capture: MockCapture,
    write: MockWrite,
) -> (ApplyInboundClipboardUseCase, Arc<RecordingEmitter>) {
    let recorder = Arc::new(RecordingEmitter::default());
    let bus = Arc::new(HostEventBus::new());
    bus.register(
        "recorder",
        Arc::clone(&recorder) as Arc<dyn HostEventEmitterPort>,
    );
    let uc = ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_host_event_emitter(bus);
    (uc, recorder)
}

/// Verdict 1 — happy path: dedup miss → decode → capture → write →
/// `Applied { entry_id }`. mockall asserts: dedup query once with
/// the input hash, capture once, write once.
#[tokio::test]
async fn applied_on_new_content() {
    let (input, hash) = fixture_input("hello phase3");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(hash.clone()))
        .times(1)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-new"))));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build(repo, capture, write);
    let outcome = uc.execute(input).await.expect("happy path returns ok");
    assert_eq!(
        outcome,
        ApplyOutcome::Applied {
            entry_id: EntryId::from("entry-new")
        }
    );
}

/// 回归:apply_inbound 必须把 `input.from_device`(推送方)透传给
/// `InboundCapture::capture` 的第二参数,持久化层才能把
/// `ClipboardEvent.source_device` 写成对端 id —— 不然 delivery view 会
/// 把远端推送进来的 entry 误识别为本机产生,详情页显示"来自本机 +
/// 等待同步"(detail 顶部 EntryDeliveryBadge)。
#[tokio::test]
async fn from_device_is_forwarded_to_capture() {
    let (input, _hash) = fixture_input("trace-source");
    let expected_from_device = input.from_device.clone();

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .withf(move |_preset, from_device, _snapshot| from_device == &expected_from_device)
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-remote"))));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build(repo, capture, write);
    uc.execute(input).await.expect("forwarding path ok");
}

/// Store-only paths (pull / CLI) leave resurface unwired: a dedup hit then
/// degrades to the plain skip — no capture, no write. Their write port is a
/// no-op anyway, and the pull path's convergence tail owns the real OS write.
#[tokio::test]
async fn dedup_hit_skips_when_resurface_is_unwired() {
    let (input, hash) = fixture_input("already-here");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(hash.clone()))
        .times(1)
        .returning(|_| Ok(Some(EntryId::from("entry-existing"))));

    // Zero expectations on capture + write — mockall panics on Drop
    // if either gets called. This pins the no-side-effect contract.
    let capture = MockCapture::new();
    let write = MockWrite::new();

    let uc = build(repo, capture, write);
    let outcome = uc.execute(input).await.expect("dedup path ok");
    assert_eq!(
        outcome,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: hash,
            existing_entry_id: EntryId::from("entry-existing"),
        }
    );
}

#[tokio::test]
async fn rapid_duplicate_skipped_even_when_repo_has_not_caught_up() {
    let (input, hash) = fixture_input("rapid-same");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(hash.clone()))
        .times(2)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-first"))));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build(repo, capture, write);
    let first = uc
        .execute(input.clone())
        .await
        .expect("first rapid inbound applies");
    assert_eq!(
        first,
        ApplyOutcome::Applied {
            entry_id: EntryId::from("entry-first")
        }
    );

    let second = uc
        .execute(input)
        .await
        .expect("second rapid inbound is filtered");
    assert_eq!(
        second,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: hash,
            existing_entry_id: EntryId::from("entry-first"),
        }
    );
}

#[tokio::test]
async fn visible_duplicate_skipped_across_channel_representation_expansion() {
    let visible_text = b"same-ui".to_vec();
    let first_snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            visible_text.clone(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let second_snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_250,
        representations: vec![
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                visible_text.clone(),
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("public.utf8-plain-text"),
                None,
                visible_text.clone(),
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("NSStringPboardType"),
                None,
                visible_text,
            ),
        ],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let (first_input, first_hash) = fixture_input_from_snapshot(first_snapshot);
    let (second_input, second_hash) = fixture_input_from_snapshot(second_snapshot);
    assert_ne!(first_hash, second_hash);

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-visible"))));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build(repo, capture, write);
    let first = uc
        .execute(first_input)
        .await
        .expect("first visible content applies");
    assert_eq!(
        first,
        ApplyOutcome::Applied {
            entry_id: EntryId::from("entry-visible")
        }
    );

    tokio::time::sleep(std::time::Duration::from_millis(250)).await;

    let second = uc
        .execute(second_input)
        .await
        .expect("expanded visible duplicate is filtered");
    assert_eq!(
        second,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: second_hash,
            existing_entry_id: EntryId::from("entry-visible"),
        }
    );
}

#[tokio::test]
async fn visible_duplicate_window_expires() {
    let first_snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            b"expires".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let second_snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_003_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.utf8-plain-text"),
            None,
            b"expires".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let (first_input, _) = fixture_input_from_snapshot(first_snapshot);
    let (second_input, _) = fixture_input_from_snapshot(second_snapshot);

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(2)
        .returning(|_, _, _| Ok(Some(EntryId::new())));

    let mut write = MockWrite::new();
    write.expect_write().times(2).returning(|_, _| Ok(()));

    let uc = build(repo, capture, write);
    assert!(
        matches!(
            uc.execute(first_input).await,
            Ok(ApplyOutcome::Applied { .. })
        ),
        "first visible content applies"
    );

    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;

    assert!(
        matches!(
            uc.execute(second_input).await,
            Ok(ApplyOutcome::Applied { .. })
        ),
        "same visible content applies again after the merge window expires"
    );
}

/// Verdict 3 — corrupt envelope returns `DecodeFailed`, no panic, no
/// capture, no write. Daemon's ingest loop keeps running.
#[tokio::test]
async fn decode_failed_on_truncated_envelope() {
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-broken"),
        snapshot_hash: "blake3v1:00".to_string(),
        plaintext: Bytes::from_static(b"not a valid V3 envelope"),
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };

    let mut repo = MockEntryRepo::new();
    // Decode runs before the dedup query, so an undecodable frame is dropped
    // without ever touching the entry store.
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(0)
        .returning(|_| Ok(None));
    let capture = MockCapture::new();
    let write = MockWrite::new();

    let uc = build(repo, capture, write);
    let outcome = uc.execute(input).await.expect("DecodeFailed is Ok variant");
    match outcome {
        ApplyOutcome::DecodeFailed { reason } => {
            assert!(
                reason.contains("decode V3 envelope"),
                "reason should mention V3 decode, got: {reason}"
            );
        }
        other => panic!("expected DecodeFailed, got {other:?}"),
    }
}

#[tokio::test]
async fn execute_records_incoming_flow_id_on_span() {
    let incoming_flow_id = FlowId::generate();
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-flow"),
        snapshot_hash: "blake3v1:11".to_string(),
        plaintext: Bytes::from_static(b"not a valid V3 envelope"),
        flow_id: Some(incoming_flow_id.clone()),
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };

    let mut repo = MockEntryRepo::new();
    // flow_id is recorded on the span before decode; the frame is undecodable,
    // so the dedup query is never reached.
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(0)
        .returning(|_| Ok(None));
    let capture = MockCapture::new();
    let write = MockWrite::new();
    let uc = build(repo, capture, write);

    let records = Arc::new(Mutex::new(Vec::new()));
    let subscriber = tracing_subscriber::registry().with(FlowIdRecordLayer {
        records: Arc::clone(&records),
    });
    let _guard = tracing::subscriber::set_default(subscriber);
    let _ = uc.execute(input).await;

    let recorded = records.lock().unwrap();
    assert!(
        recorded
            .iter()
            .any(|value| value == &incoming_flow_id.to_string()),
        "apply_inbound 应该记录入站 header 传来的 flow_id"
    );
}

/// Verdict 4 — capture returns Ok(None) (shouldn't happen for
/// RemotePush but guard it anyway): mapped to
/// `ApplyInboundError::Internal`. Write must NOT fire.
#[tokio::test]
async fn capture_returning_none_maps_to_internal_error() {
    let (input, _) = fixture_input("orphan");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(None));

    // Zero expectations on write.
    let write = MockWrite::new();

    let uc = build(repo, capture, write);
    let err = uc
        .execute(input)
        .await
        .expect_err("Ok(None) from capture must surface as error");
    match err {
        ApplyInboundError::Internal(msg) => {
            assert!(
                msg.contains("RemotePush"),
                "internal message should reference origin, got: {msg}"
            );
        }
        other => panic!("expected Internal, got {other:?}"),
    }
}

/// Verdict 5 — OS clipboard write is best-effort and runs in the
/// background. Capture has already committed by the time the spawn
/// happens; a write failure in the spawned task must NOT surface as
/// an error on the apply_inbound main path —— that would let the
/// upstream mobile_sync `finalize_transfer_lifecycle` think the
/// transfer failed, when really it succeeded (bytes are in the entry,
/// only the system clipboard write didn't take). Pin this trade-off
/// so a future refactor doesn't re-couple OS write into the critical
/// path.
#[tokio::test]
async fn write_failure_does_not_surface_after_capture_commits() {
    let (input, _) = fixture_input("write-will-fail");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-committed"))));

    // Deterministic synchronization:让 mock 在被调用时 signal,test 主体
    // await 这个 signal,确保 `.times(1)` 期望在 mock Drop 之前一定满足,
    // 而不是赌"10ms 内 spawn 跑完了"。
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let tx = std::sync::Arc::new(std::sync::Mutex::new(Some(tx)));
    let tx_for_mock = std::sync::Arc::clone(&tx);

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(move |_, _| {
        if let Some(tx) = tx_for_mock.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = tx.send(());
        }
        Err(anyhow::anyhow!("OS clipboard locked"))
    });

    let uc = build(repo, capture, write);
    let outcome = uc
        .execute(input)
        .await
        .expect("write failure must NOT surface — capture already committed");
    match outcome {
        ApplyOutcome::Applied { entry_id } => {
            assert_eq!(entry_id.as_ref(), "entry-committed");
        }
        other => panic!("expected Applied, got {other:?}"),
    }
    // 确定性等待 spawn 后台 write 完成(mock 内部 send 信号)。
    rx.await.expect("background write task must run");
}

/// Verdict 6 — dedup query failure surfaces as `DedupQuery`. No
/// decode, no capture, no write — failing closed is the conservative
/// choice (we'd rather lose an inbound frame than risk a corrupt
/// double-write under unknown DB state).
#[tokio::test]
async fn dedup_query_failure_short_circuits() {
    let (input, _) = fixture_input("dedup-broken");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Err(ClipboardRepositoryError::Storage("db down".to_string())));
    let capture = MockCapture::new();
    let write = MockWrite::new();

    let uc = build(repo, capture, write);
    let err = uc.execute(input).await.expect_err("dedup error propagates");
    match err {
        ApplyInboundError::DedupQuery(_) => {}
        other => panic!("expected DedupQuery, got {other:?}"),
    }
}

/// Verdict 7 — 入站 blob refs 会先本地化,再进入 capture 和剪贴板写入。
/// capture/write mock 校验收到的是改写后的本机 file URI,不是发送端原始路径。
#[tokio::test]
async fn materializes_blob_refs_before_capture_and_write() {
    let original = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/original.txt\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![9, 8, 7]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("original.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 13,
        representation_index: None,
    };
    let (plaintext, snapshot_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-x"),
        snapshot_hash: snapshot_hash.clone(),
        plaintext,
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(snapshot_hash))
        .times(1)
        .returning(|_| Ok(None));

    let mut materializer = MockBlobMaterializer::new();
    materializer
        .expect_materialize()
        .times(1)
        .withf(
            move |_from_device, _receiver_entry_id, snapshot, refs, manifest| {
                assert!(manifest.is_none());
                snapshot.representations[0].expect_inline_bytes()
                    == b"file:///sender/original.txt\n"
                    && refs == &vec![blob_ref.clone()]
            },
        )
        .returning(|_from_device, _receiver_entry_id, mut snapshot, _, _| {
            snapshot.representations[0]
                .set_inline_bytes(b"file:///local/cache/original.txt\n".to_vec())
                .unwrap();
            Ok(MaterializeResult::complete(snapshot))
        });

    let assert_local_file = |snapshot: &SystemClipboardSnapshot| {
        snapshot.representations[0].expect_inline_bytes() == b"file:///local/cache/original.txt\n"
    };
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .withf(move |_preset_entry_id, _from_device, snapshot| assert_local_file(snapshot))
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-new"))));

    let mut write = MockWrite::new();
    write
        .expect_write()
        .withf(move |snapshot, _| assert_local_file(snapshot))
        .times(1)
        .returning(|_, _| Ok(()));

    let uc = build_with_blob_materializer(repo, capture, write, materializer);
    let outcome = uc.execute(input).await.expect("blob materialize path ok");
    assert_eq!(
        outcome,
        ApplyOutcome::Applied {
            entry_id: EntryId::from("entry-new")
        }
    );
}

/// P5 invariant — partial MaterializeResult **必须**让 apply_inbound:
/// (1) 仍走 capture 把 entry 落库(用户能在列表里看到取消的 entry);
/// (2) **绝不** spawn OS clipboard write —— 否则半残 snapshot 里的
///     `uniclip-missing://` URI 会污染用户的系统剪贴板。
///
/// 这条 invariant 是 OS pasteboard 安全的最后一道闸门。如果未来重构
/// 误把 spawn 块挪出 `if !is_partial`,此测会 fail。
#[tokio::test]
async fn partial_materialize_persists_entry_but_skips_os_write() {
    let original = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/big.iso\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![5, 5, 5]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("big.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 950_000_000,
        representation_index: None,
    };
    let (plaintext, snapshot_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        snapshot_hash,
        plaintext,
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    // materializer 模拟用户中途 cancel:返回 partial,snapshot 里 file-list rep
    // 已被重写为带 uniclip-missing:// 占位,missing 列表非空。
    let mut materializer = MockBlobMaterializer::new();
    materializer.expect_materialize().times(1).returning(
        |_from_device, _receiver_entry_id, mut snapshot, _, _| {
            snapshot.representations[0]
                .set_inline_bytes(
                    b"uniclip-missing:///big.iso?size=950000000&reason=cancelled".to_vec(),
                )
                .unwrap();
            Ok(MaterializeResult {
                snapshot,
                missing: vec![super::materializer::MissingFileRef {
                    filename: "big.iso".to_string(),
                    size_bytes: 950_000_000,
                }],
                partial: true,
                // This fake stands in for the materializer; it publishes nothing.
                publication: None,
            })
        },
    );

    // capture 必须被调:partial entry 也要落库。
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Ok(Some(EntryId::from("entry-partial"))));

    // 核心断言:OS clipboard write 在 partial 分支**绝不**被触发。
    let mut write = MockWrite::new();
    write.expect_write().times(0);

    let uc = build_with_blob_materializer(repo, capture, write, materializer);
    let outcome = uc
        .execute(input)
        .await
        .expect("partial path should produce ApplyOutcome::Applied");
    assert_eq!(
        outcome,
        ApplyOutcome::Applied {
            entry_id: EntryId::from("entry-partial")
        }
    );
    // 注:测试结束时 MockWrite 的 Drop 会校验 times(0) 不被违反;
    // 即便 OS write 被 spawn 到后台 task,mock 在 verify 时也会捕获。
}

/// P5 invariant — partial entry **不能**进 dedup 窗口。否则用户取消后
/// 立即重发同一文件会被 `find_recent_duplicate` 当 dup 直接 skip,陷入
/// "无法恢复"困境。
#[tokio::test]
async fn partial_materialize_does_not_register_dedup_entry() {
    let original = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/retry.iso\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![7, 7, 7]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("retry.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 100,
        representation_index: None,
    };
    let (plaintext, snapshot_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input1 = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        snapshot_hash: snapshot_hash.clone(),
        plaintext: plaintext.clone(),
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };
    let input2 = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        snapshot_hash,
        plaintext,
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(None));

    // 第一次 partial,第二次成功(模拟用户取消后重传成功)。
    let mut materializer = MockBlobMaterializer::new();
    let call_count = std::sync::atomic::AtomicUsize::new(0);
    materializer.expect_materialize().times(2).returning(
        move |_from_device, _receiver_entry_id, mut snapshot, _, _| {
            let n = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n == 0 {
                snapshot.representations[0]
                    .set_inline_bytes(
                        b"uniclip-missing:///retry.iso?size=100&reason=cancelled".to_vec(),
                    )
                    .unwrap();
                Ok(MaterializeResult {
                    snapshot,
                    missing: vec![super::materializer::MissingFileRef {
                        filename: "retry.iso".to_string(),
                        size_bytes: 100,
                    }],
                    partial: true,
                    // This fake stands in for the materializer; it publishes nothing.
                    publication: None,
                })
            } else {
                snapshot.representations[0]
                    .set_inline_bytes(b"file:///local/cache/retry.iso\n".to_vec())
                    .unwrap();
                Ok(MaterializeResult::complete(snapshot))
            }
        },
    );

    // capture 必须被调两次:第一次 partial entry + 第二次 complete entry。
    // 如果 dedup 把第二次 silently skip,capture 只会被调一次,这个 times(2)
    // 会 fail。
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(2)
        .returning(|_, _, _| Ok(Some(EntryId::new())));

    // OS write 只发生在第二次(complete)。
    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build_with_blob_materializer(repo, capture, write, materializer);

    let outcome1 = uc.execute(input1).await.expect("first attempt ok");
    assert!(matches!(outcome1, ApplyOutcome::Applied { .. }));

    // 用户重传同一 envelope。
    let outcome2 = uc.execute(input2).await.expect("retry after cancel ok");
    assert!(
        matches!(outcome2, ApplyOutcome::Applied { .. }),
        "retry must NOT be deduped after a prior partial: {outcome2:?}"
    );
}

/// Verdict 8 — 真实文件缓存 materializer 会拉取 blob 内容,写入接收端缓存目录,
/// 并把 file-list 表示改写为本机 `file://` URI。
#[tokio::test]
async fn file_cache_blob_materializer_writes_file_and_rewrites_file_uri_list() {
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let entry_id = EntryId::from("entry-file");
    let ticket = BlobTicket::from_bytes(vec![1, 2, 3]);
    let blob_ref = V3BlobRef {
        ticket: ticket.clone(),
        entry_id: entry_id.clone(),
        filename: Some("report.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 11,
        representation_index: None,
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/report.txt\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .withf(move |command| command.entry_id == entry_id && command.ticket == ticket)
        .returning(|command| {
            // Mirror the real adapter: write the bytes to target_path so
            // the subsequent `file://` rewrite + `tokio::fs::read` assertion
            // sees the materialized content. GH#487 Phase 2 changed the
            // production path from `fetch_blob -> tokio::fs::write` to
            // `fetch_blob_to_path` (streaming export); the test fake mirrors
            // that contract instead of round-tripping through `Bytes`.
            let payload: &[u8] = b"hello world";
            std::fs::write(&command.target_path, payload).expect("fake write target");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([0; 32]),
                digest: BlobDigest::from_bytes([1; 32]),
                bytes_written: payload.len() as u64,
            })
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let rewritten = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("materialize should succeed");

    assert!(!rewritten.is_partial(), "expected complete materialize");
    let uri_list = String::from_utf8(
        rewritten.snapshot.representations[0]
            .expect_inline_bytes()
            .to_vec(),
    )
    .expect("uri-list should be UTF-8");
    assert!(uri_list.starts_with("file://"));
    assert!(uri_list.ends_with("/report.txt\n"));
    assert!(!uri_list.contains("/sender/"));

    let local_url = url::Url::parse(uri_list.trim()).expect("valid file URL");
    let local_path = local_url.to_file_path().expect("file URL to path");
    let bytes = tokio::fs::read(local_path)
        .await
        .expect("materialized file should exist");
    assert_eq!(bytes, b"hello world");
}

#[tokio::test]
async fn directory_materializer_rebuilds_nested_tree_and_empty_directory() {
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let sender_entry_id = EntryId::from("entry-directory");
    let receiver_entry_id = EntryId::from("receiver-directory");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![4, 5, 6]),
        entry_id: sender_entry_id,
        filename: Some("notes.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 5,
        representation_index: None,
    };
    let manifest = InboundFileSetManifest {
        members: vec![
            InboundFileSetMember {
                root_index: 0,
                root_name: "project".to_string(),
                root_is_file: false,
                relative_path: "docs/notes.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(0),
            },
            InboundFileSetMember {
                root_index: 0,
                root_name: "project".to_string(),
                root_is_file: false,
                relative_path: "empty".to_string(),
                kind: FileSetMemberKind::EmptyDirectory,
                blob_ref_index: None,
            },
        ],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/project\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|command| {
            std::fs::write(&command.target_path, b"hello").expect("write nested file");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([7; 32]),
                digest: BlobDigest::from_bytes([8; 32]),
                bytes_written: 5,
            })
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id.clone(),
            snapshot,
            vec![blob_ref],
            Some(manifest),
        )
        .await
        .expect("directory materialize");

    let root = cache_dir
        .path()
        .join("iroh-blobs")
        .join(receiver_entry_id.as_ref())
        .join("project");
    assert_eq!(
        std::fs::read(root.join("docs/notes.txt")).unwrap(),
        b"hello"
    );
    assert!(root.join("empty").is_dir());
    assert!(result.snapshot.file_set_v1_component.is_some());
    let uri_list = String::from_utf8(
        result.snapshot.representations[0]
            .expect_inline_bytes()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(
        url::Url::parse(uri_list.trim())
            .unwrap()
            .to_file_path()
            .unwrap(),
        root
    );
    assert!(!cache_dir.path().join("iroh-blobs/staging").exists());
}

#[tokio::test]
async fn directory_materializer_preserves_mixed_top_level_file_and_directory() {
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let receiver_entry_id = EntryId::from("receiver-mixed-roots");
    let blob_refs = ["loose.txt", "child.txt"]
        .into_iter()
        .enumerate()
        .map(|(index, filename)| V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![index as u8 + 1]),
            entry_id: EntryId::from(format!("sender-mixed-{index}")),
            filename: Some(filename.to_string()),
            mime: None,
            size_bytes: 5,
            representation_index: None,
        })
        .collect::<Vec<_>>();
    let manifest = InboundFileSetManifest {
        members: vec![
            InboundFileSetMember {
                root_index: 0,
                root_name: "loose.txt".to_string(),
                root_is_file: true,
                relative_path: "loose.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(0),
            },
            InboundFileSetMember {
                root_index: 1,
                root_name: "folder".to_string(),
                root_is_file: false,
                relative_path: "child.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(1),
            },
        ],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(|command| {
            std::fs::write(&command.target_path, b"hello").unwrap();
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([5; 32]),
                digest: BlobDigest::from_bytes([6; 32]),
                bytes_written: 5,
            })
        });
    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());

    materializer
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id.clone(),
            snapshot,
            blob_refs,
            Some(manifest),
        )
        .await
        .expect("mixed roots should materialize");

    let target = cache_dir
        .path()
        .join("iroh-blobs")
        .join(receiver_entry_id.as_ref());
    assert_eq!(std::fs::read(target.join("loose.txt")).unwrap(), b"hello");
    assert_eq!(
        std::fs::read(target.join("folder/child.txt")).unwrap(),
        b"hello"
    );
}

#[tokio::test]
async fn apply_inbound_materializes_and_commits_mixed_directory_payload() {
    use std::collections::BTreeMap;

    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let blob_refs = ["loose.txt", "child.txt"]
        .into_iter()
        .enumerate()
        .map(|(index, filename)| V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![index as u8 + 1]),
            entry_id: EntryId::from(format!("sender-full-flow-{index}")),
            filename: Some(filename.to_string()),
            mime: Some("text/plain".to_string()),
            size_bytes: 5,
            representation_index: None,
        })
        .collect::<Vec<_>>();
    let manifest = InboundFileSetManifest {
        members: vec![
            InboundFileSetMember {
                root_index: 0,
                root_name: "loose.txt".to_string(),
                root_is_file: true,
                relative_path: "loose.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(0),
            },
            InboundFileSetMember {
                root_index: 1,
                root_name: "folder".to_string(),
                root_is_file: false,
                relative_path: "child.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(1),
            },
        ],
    };
    let member_digests = BTreeMap::from([(0, [5; 32]), (1, [6; 32])]);
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/loose.txt\r\nfile:///sender/folder\r\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: Some(
            compute_file_set_component(&manifest, &member_digests).expect("file-set component"),
        ),
    };
    let (plaintext, snapshot_hash) =
        encode_snapshot_with_blob_refs_and_file_set_to_v3_bytes(&snapshot, &blob_refs, &manifest)
            .expect("encode directory payload");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(snapshot_hash.clone()))
        .times(1)
        .returning(|_| Ok(None));

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(|command| {
            let index = usize::from(command.ticket.as_bytes()[0] - 1);
            std::fs::write(&command.target_path, b"hello").expect("write fetched member");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes(if index == 0 {
                    [5; 32]
                } else {
                    [6; 32]
                }),
                digest: BlobDigest::from_bytes([7; 32]),
                bytes_written: 5,
            })
        });

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .withf(|_, _, snapshot| snapshot_paths_exist(snapshot))
        .returning(|entry_id, _, _| Ok(Some(entry_id)));

    let write_completed = Arc::new(tokio::sync::Notify::new());
    let mut write = MockWrite::new();
    write
        .expect_write()
        .times(1)
        .withf(|s, _| snapshot_paths_exist(s))
        .returning({
            let write_completed = Arc::clone(&write_completed);
            move |_, _| {
                write_completed.notify_one();
                Ok(())
            }
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let uc = ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_blob_materializer(Arc::new(materializer));
    let outcome = uc
        .execute(ApplyInboundInput {
            from_device: DeviceId::new("peer-full-flow"),
            snapshot_hash,
            plaintext,
            flow_id: None,
            resurface_intent: ClipboardWriteIntent::RemotePush,
        })
        .await
        .expect("directory payload should apply");
    assert!(matches!(outcome, ApplyOutcome::Applied { .. }));
    tokio::time::timeout(
        std::time::Duration::from_secs(1),
        write_completed.notified(),
    )
    .await
    .expect("clipboard write should complete");
}

fn snapshot_paths_exist(snapshot: &SystemClipboardSnapshot) -> bool {
    let Some(rep) = snapshot.representations.iter().find(|rep| {
        rep.mime
            .as_ref()
            .is_some_and(|mime| mime.0 == "text/uri-list")
    }) else {
        return false;
    };
    let Ok(uri_list) = std::str::from_utf8(rep.expect_inline_bytes()) else {
        return false;
    };
    let paths = uri_list
        .lines()
        .filter_map(|line| url::Url::parse(line).ok())
        .filter_map(|url| url.to_file_path().ok())
        .collect::<Vec<_>>();
    paths.len() == 2
        && paths.iter().any(|path| path.is_file())
        && paths.iter().any(|path| path.join("child.txt").is_file())
}

#[tokio::test]
async fn directory_materializer_removes_staging_and_exposes_nothing_on_failure() {
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let receiver_entry_id = EntryId::from("receiver-failed-directory");
    let blob_refs = ["first.txt", "second.txt", "third.txt"]
        .into_iter()
        .enumerate()
        .map(|(index, name)| V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![index as u8 + 1]),
            entry_id: EntryId::from(format!("sender-{index}")),
            filename: Some(name.to_string()),
            mime: None,
            size_bytes: 1,
            representation_index: None,
        })
        .collect::<Vec<_>>();
    let manifest = InboundFileSetManifest {
        members: blob_refs
            .iter()
            .enumerate()
            .map(|(index, blob_ref)| InboundFileSetMember {
                root_index: 0,
                root_name: "folder".to_string(),
                root_is_file: false,
                relative_path: blob_ref.filename.clone().unwrap(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(index as u32),
            })
            .collect(),
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    let mut call = 0usize;
    let transfer_ids = Arc::new(Mutex::new(Vec::new()));
    let recorded_ids = Arc::clone(&transfer_ids);
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(move |command| {
            let context = command.transfer_context.as_ref().expect("transfer context");
            assert_eq!(context.entry_id, "receiver-failed-directory");
            assert!(context.individual_lifecycle);
            recorded_ids
                .lock()
                .unwrap()
                .push(context.transfer_id.clone());
            call += 1;
            if call == 2 {
                anyhow::bail!("network failed");
            }
            std::fs::write(&command.target_path, b"a").expect("write first staged file");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([1; 32]),
                digest: BlobDigest::from_bytes([2; 32]),
                bytes_written: 1,
            })
        });
    fetcher
        .expect_record_unfetched_failure()
        .times(1)
        .withf(|context, detail| {
            context.entry_id == "receiver-failed-directory"
                && context.filename == "third.txt"
                && context.individual_lifecycle
                && detail.contains("network failed")
        })
        .return_const(());

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let error = materializer
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id.clone(),
            snapshot,
            blob_refs,
            Some(manifest),
        )
        .await
        .expect_err("directory failure must fail the whole entry");

    assert!(error.to_string().contains("network failed"));
    let transfer_ids = transfer_ids.lock().unwrap();
    assert_eq!(transfer_ids.len(), 2);
    assert_ne!(transfer_ids[0], transfer_ids[1]);
    assert!(!cache_dir
        .path()
        .join("iroh-blobs")
        .join(receiver_entry_id.as_ref())
        .join("folder")
        .exists());
    assert!(!cache_dir.path().join("iroh-blobs/staging").exists());
}

#[tokio::test]
async fn directory_materializer_suffixes_existing_and_same_paste_root_names() {
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let receiver_entry_id = EntryId::from("receiver-collisions");
    let destination = cache_dir
        .path()
        .join("iroh-blobs")
        .join(receiver_entry_id.as_ref());
    std::fs::create_dir_all(destination.join("folder")).unwrap();
    let manifest = InboundFileSetManifest {
        members: vec![
            InboundFileSetMember {
                root_index: 0,
                root_name: "folder".to_string(),
                root_is_file: false,
                relative_path: "empty-a".to_string(),
                kind: FileSetMemberKind::EmptyDirectory,
                blob_ref_index: None,
            },
            InboundFileSetMember {
                root_index: 1,
                root_name: "folder".to_string(),
                root_is_file: false,
                relative_path: "empty-b".to_string(),
                kind: FileSetMemberKind::EmptyDirectory,
                blob_ref_index: None,
            },
        ],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let materializer = test_materializer(
        Arc::new(MockBlobFetcher::new()),
        cache_dir.path().to_path_buf(),
    );
    let result = materializer
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id,
            snapshot,
            Vec::new(),
            Some(manifest),
        )
        .await
        .expect("empty directory set should materialize");

    assert!(destination.join("folder (1)/empty-a").is_dir());
    assert!(destination.join("folder (2)/empty-b").is_dir());
    let uri_list = String::from_utf8(
        result.snapshot.representations[0]
            .expect_inline_bytes()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(uri_list.lines().count(), 2);
}

#[cfg(unix)]
#[tokio::test]
async fn directory_materializer_restores_executable_permission() {
    use std::os::unix::fs::PermissionsExt;
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let receiver_entry_id = EntryId::from("receiver-executable");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![3]),
        entry_id: EntryId::from("sender-executable"),
        filename: Some("run.sh".to_string()),
        mime: None,
        size_bytes: 9,
        representation_index: None,
    };
    let manifest = InboundFileSetManifest {
        members: vec![InboundFileSetMember {
            root_index: 0,
            root_name: "scripts".to_string(),
            root_is_file: false,
            relative_path: "run.sh".to_string(),
            kind: FileSetMemberKind::Executable,
            blob_ref_index: Some(0),
        }],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|command| {
            std::fs::write(&command.target_path, b"#!/bin/sh").unwrap();
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([3; 32]),
                digest: BlobDigest::from_bytes([4; 32]),
                bytes_written: 9,
            })
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    materializer
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id.clone(),
            snapshot,
            vec![blob_ref],
            Some(manifest),
        )
        .await
        .unwrap();

    let mode = std::fs::metadata(
        cache_dir
            .path()
            .join("iroh-blobs")
            .join(receiver_entry_id.as_ref())
            .join("scripts/run.sh"),
    )
    .unwrap()
    .permissions()
    .mode();
    assert_ne!(mode & 0o111, 0);
}

#[cfg(windows)]
#[tokio::test]
async fn directory_materializer_keeps_executable_member_as_regular_windows_file() {
    use uc_core::clipboard::FileSetMemberKind;

    let cache_dir = tempfile::tempdir().expect("cache dir");
    let receiver_entry_id = EntryId::from("receiver-windows-executable");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![3]),
        entry_id: EntryId::from("sender-windows-executable"),
        filename: Some("run.cmd".to_string()),
        mime: None,
        size_bytes: 7,
        representation_index: None,
    };
    let manifest = InboundFileSetManifest {
        members: vec![InboundFileSetMember {
            root_index: 0,
            root_name: "scripts".to_string(),
            root_is_file: false,
            relative_path: "run.cmd".to_string(),
            kind: FileSetMemberKind::Executable,
            blob_ref_index: Some(0),
        }],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: Vec::new(),
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|command| {
            std::fs::write(&command.target_path, b"echo ok").unwrap();
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([3; 32]),
                digest: BlobDigest::from_bytes([4; 32]),
                bytes_written: 7,
            })
        });

    test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf())
        .materialize(
            DeviceId::new("peer-x"),
            receiver_entry_id.clone(),
            snapshot,
            vec![blob_ref],
            Some(manifest),
        )
        .await
        .unwrap();

    let path = cache_dir
        .path()
        .join("iroh-blobs")
        .join(receiver_entry_id.as_ref())
        .join("scripts/run.cmd");
    assert_eq!(std::fs::read(&path).unwrap(), b"echo ok");
    assert!(!std::fs::metadata(path).unwrap().permissions().readonly());
}

/// Fake reserver: redirects every inbound file flat into `dir`.
struct FakeUserDirReserver {
    dir: std::path::PathBuf,
}

#[async_trait]
impl uc_core::ports::inbound_file_target::ReserveInboundFileTargetPort for FakeUserDirReserver {
    async fn reserve_target(&self, file_name: &str) -> Option<std::path::PathBuf> {
        // Mirror the real adapter: atomically create an empty placeholder to
        // claim the name, so a failed fetch leaves an orphan the materializer
        // is expected to clean up.
        let path = self.dir.join(file_name);
        std::fs::File::create(&path).expect("create reservation placeholder");
        Some(path)
    }
}

/// With a reserver injected that returns a user directory, the file is written
/// there (not into the managed cache layout) and the rewritten `file://` URI
/// points at the user directory.
#[tokio::test]
async fn file_cache_blob_materializer_redirects_to_reserved_user_dir() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let user_dir = tempfile::tempdir().expect("user dir");
    let entry_id = EntryId::from("entry-file");
    let ticket = BlobTicket::from_bytes(vec![4, 5, 6]);
    let blob_ref = V3BlobRef {
        ticket: ticket.clone(),
        entry_id: entry_id.clone(),
        filename: Some("report.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 11,
        representation_index: None,
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/report.txt\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|command| {
            let payload: &[u8] = b"hello world";
            std::fs::write(&command.target_path, payload).expect("fake write target");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([0; 32]),
                digest: BlobDigest::from_bytes([1; 32]),
                bytes_written: payload.len() as u64,
            })
        });

    let user_dir_path = user_dir.path().to_path_buf();
    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf())
        .with_target_reserver(Arc::new(FakeUserDirReserver {
            dir: user_dir_path.clone(),
        }));
    let rewritten = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("materialize should succeed");

    let uri_list = String::from_utf8(
        rewritten.snapshot.representations[0]
            .expect_inline_bytes()
            .to_vec(),
    )
    .expect("uri-list should be UTF-8");
    let local_path = url::Url::parse(uri_list.trim())
        .expect("valid file URL")
        .to_file_path()
        .expect("file URL to path");

    // File landed in the user directory, flat — not the managed cache layout.
    assert_eq!(local_path, user_dir_path.join("report.txt"));
    assert_eq!(tokio::fs::read(&local_path).await.unwrap(), b"hello world");
    assert!(
        !cache_dir.path().join("iroh-blobs").exists(),
        "managed cache layout must not be used when redirecting"
    );
}

/// A fetch that fails after the name was reserved must not leave the reserved
/// placeholder behind in the user's save directory. Unlike the managed cache
/// layout, that directory is never reclaimed by retention or entry deletion, so
/// an orphan would linger forever and force the next same-named file to a
/// ` (n)` suffix. Covers the non-cancel error branch (network drop / export
/// failure).
#[tokio::test]
async fn file_cache_blob_materializer_removes_reserved_placeholder_on_fetch_error() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let user_dir = tempfile::tempdir().expect("user dir");
    let entry_id = EntryId::from("entry-file");
    let ticket = BlobTicket::from_bytes(vec![9, 9, 9]);
    let blob_ref = V3BlobRef {
        ticket: ticket.clone(),
        entry_id: entry_id.clone(),
        filename: Some("report.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 11,
        representation_index: None,
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/report.txt\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        // Non-cancel failure before the store-export step: the reserved
        // placeholder was created but never overwritten. `is_cancel_error`
        // returns false for a plain anyhow error, so this hits the
        // non-cancel `Err(e)` arm.
        .returning(|_command| Err(anyhow::anyhow!("network dropped mid-transfer")));

    let user_dir_path = user_dir.path().to_path_buf();
    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf())
        .with_target_reserver(Arc::new(FakeUserDirReserver {
            dir: user_dir_path.clone(),
        }));
    let result = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("partial materialize should succeed (fetch error is soft)");

    assert!(result.is_partial(), "failed fetch must mark partial");
    assert!(
        !user_dir_path.join("report.txt").exists(),
        "reserved placeholder must be removed after a failed fetch"
    );
    let leftovers = std::fs::read_dir(&user_dir_path)
        .expect("read user dir")
        .count();
    assert_eq!(leftovers, 0, "user save directory must be left clean");
}

/// Verdict 9 —— representation_index 路径：blob ref 携带索引时,materializer
/// 把 fetched bytes 灌回 envelope 主体里对应索引的 rep,而不是写到 cache_dir
/// 当 file 处理。这条路径是 oversized image 跨设备同步的关键。
#[tokio::test]
async fn file_cache_blob_materializer_inlines_representation_bound_blob_into_rep() {
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let entry_id = EntryId::from("entry-img");
    let ticket = BlobTicket::from_bytes(vec![7, 7, 7]);
    let blob_ref = V3BlobRef {
        ticket: ticket.clone(),
        entry_id: entry_id.clone(),
        filename: None,
        mime: Some("image/png".to_string()),
        size_bytes: 5,
        representation_index: Some(0),
    };
    // Sender drained `bytes` to empty when publishing — receiver decode
    // mirrors that empty-rep state until materialization runs.
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            Vec::new(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob()
        .times(1)
        .withf(move |command| command.entry_id == entry_id && command.ticket == ticket)
        .returning(|command| {
            Ok(crate::facade::blob_transfer::FetchBlobResult {
                plaintext: Bytes::from_static(b"\x89PNG\x0d"),
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([0; 32]),
                digest: BlobDigest::from_bytes([1; 32]),
            })
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let materialized = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("representation-bound materialize should succeed");

    assert!(!materialized.is_partial(), "expected complete materialize");
    assert_eq!(materialized.snapshot.representations.len(), 1);
    assert_eq!(
        materialized.snapshot.representations[0].expect_inline_bytes(),
        b"\x89PNG\x0d"
    );
    assert_eq!(
        materialized.snapshot.representations[0].format_id.as_ref(),
        "image"
    );

    let mut entries = tokio::fs::read_dir(cache_dir.path())
        .await
        .expect("read cache_dir");
    assert!(
        entries.next_entry().await.expect("read entry").is_none(),
        "cache_dir must be empty for representation-bound refs"
    );
}

/// Verdict 10 —— representation_index 越界时,materializer 必须显式报错而不是
/// silently 落到 file 路径或 panic。这个 guard 防止协议不一致的对端把消息
/// 灌进错误的 rep slot。
#[tokio::test]
async fn file_cache_blob_materializer_rejects_out_of_bounds_representation_index() {
    let cache_dir = tempfile::tempdir().expect("tempdir");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![1]),
        entry_id: EntryId::from("entry-bad"),
        filename: None,
        mime: Some("image/png".to_string()),
        size_bytes: 1,
        representation_index: Some(5),
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            Vec::new(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher.expect_fetch_blob().times(1).returning(|command| {
        Ok(crate::facade::blob_transfer::FetchBlobResult {
            plaintext: Bytes::from_static(b"x"),
            entry_id: command.entry_id,
            plaintext_hash: PlaintextHash::from_bytes([0; 32]),
            digest: BlobDigest::from_bytes([1; 32]),
        })
    });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let err = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect_err("out-of-bounds index should fail");
    assert!(
        err.to_string().contains("out of bounds"),
        "error must mention out-of-bounds context: {err}"
    );
}

/// P5 回归 —— 多文件 batch 在中途被 user cancel:已落地的 file:// + 未完成的
/// uniclip-missing:// 共存在重写后的 file-list rep 中,`MaterializeResult` 标
/// is_partial=true,missing 字段列出未完成 file 的元数据,稍后调用方可据此跳过
/// OS clipboard write 防止把占位 URI 推到系统剪贴板。
#[tokio::test]
async fn file_cache_blob_materializer_partial_on_cancel_mid_batch() {
    use crate::facade::blob_transfer::BlobTransferError;

    let cache_dir = tempfile::tempdir().expect("tempdir");
    let ok_ticket = BlobTicket::from_bytes(vec![1, 1, 1]);
    let cancel_ticket = BlobTicket::from_bytes(vec![2, 2, 2]);

    let blob_ref_ok = V3BlobRef {
        ticket: ok_ticket.clone(),
        entry_id: EntryId::from("entry-ok"),
        filename: Some("first.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 5,
        representation_index: None,
    };
    let blob_ref_cancel = V3BlobRef {
        ticket: cancel_ticket.clone(),
        entry_id: EntryId::from("entry-cancel"),
        filename: Some("second.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 950_000,
        representation_index: None,
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/first.txt\r\nfile:///sender/second.iso\r\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(move |command| {
            if command.ticket == ok_ticket {
                let payload: &[u8] = b"hello";
                std::fs::write(&command.target_path, payload).expect("fake write target");
                Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                    entry_id: command.entry_id,
                    plaintext_hash: PlaintextHash::from_bytes([0; 32]),
                    digest: BlobDigest::from_bytes([1; 32]),
                    bytes_written: payload.len() as u64,
                })
            } else {
                Err(anyhow::Error::from(BlobTransferError::Cancelled))
            }
        });

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref_ok, blob_ref_cancel],
            None,
        )
        .await
        .expect("partial materialize should succeed (no real error)");

    assert!(result.is_partial(), "missing file should mark partial");
    assert_eq!(result.missing.len(), 1, "exactly one missing file");
    assert_eq!(result.missing[0].filename, "second.iso");
    assert_eq!(result.missing[0].size_bytes, 950_000);

    let uri_list = String::from_utf8(
        result.snapshot.representations[0]
            .expect_inline_bytes()
            .to_vec(),
    )
    .expect("uri-list should be UTF-8");
    assert!(
        uri_list.contains("file://") && uri_list.contains("/first.txt"),
        "completed file:// should be present in uri-list: {uri_list:?}"
    );
    assert!(
        uri_list.contains("uniclip-missing:///second.iso"),
        "cancelled file placeholder should use uniclip-missing scheme: {uri_list:?}"
    );
    assert!(
        uri_list.contains("reason=cancelled"),
        "missing URI should carry reason metadata: {uri_list:?}"
    );
}

/// P5 回归 —— 整批 file_refs 在第一个 fetch 上就被 cancel,无任何已落地文件。
/// MaterializeResult.missing 应当列出全部 file_refs 的元数据,file-list rep
/// 全部用 uniclip-missing:// URI 占位。
#[tokio::test]
async fn file_cache_blob_materializer_partial_on_cancel_first_file() {
    use crate::facade::blob_transfer::BlobTransferError;

    let cache_dir = tempfile::tempdir().expect("tempdir");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![9]),
        entry_id: EntryId::from("entry-cancel"),
        filename: Some("nothing.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 100,
        representation_index: None,
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/nothing.iso\r\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|_| Err(anyhow::Error::from(BlobTransferError::Cancelled)));

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("first-file cancel still yields Ok(partial)");

    assert!(result.is_partial());
    assert_eq!(result.missing.len(), 1);
    assert_eq!(result.missing[0].filename, "nothing.iso");
    // snapshot 必须仍有 supported rep,否则 capture pipeline 会短路 Ok(None)
    // 不落 entry,与用户契约"取消后保留 entry"相悖。
    assert!(!result.snapshot.representations.is_empty());
}

/// 回归 pin —— rep-bound blob 阶段被 cancel,envelope 不带任何 file_refs。
/// 这条路径下 `missing` 必然为空(missing 只列 file_refs),但 snapshot 里
/// 那条未完成的 image rep 已经被删除 —— 半残 snapshot 不能写 OS 剪贴板,
/// 也不能进 dedup 表。修复前 `is_partial` 只看 `!missing.is_empty()`
/// 会把这种情况误判为 complete,把占位状态当真相落库。
#[tokio::test]
async fn file_cache_blob_materializer_partial_on_rep_cancel_no_files() {
    use crate::facade::blob_transfer::BlobTransferError;

    let cache_dir = tempfile::tempdir().expect("tempdir");
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![7]),
        entry_id: EntryId::from("entry-img-cancel"),
        filename: None,
        mime: Some("image/png".to_string()),
        size_bytes: 1024,
        representation_index: Some(0),
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType("image/png".to_string())),
            Vec::new(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob()
        .times(1)
        .returning(|_| Err(anyhow::Error::from(BlobTransferError::Cancelled)));

    let materializer = test_materializer(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
            None,
        )
        .await
        .expect("rep-only cancel yields Ok(partial)");

    assert!(
        result.is_partial(),
        "rep-only cancel must mark partial even though `missing` is empty (file_refs absent)"
    );
    assert!(
        result.missing.is_empty(),
        "rep cancel does not produce MissingFileRef entries (those describe file_refs only)"
    );
}

/// 回归 pin —— 2026-05-08 移动端图片回归暴露:apply_inbound 流程入口 emit
/// `IncomingPending` 后,**末尾必须 emit `NewContent`**,否则前端
/// `useClipboardEventStream.ts:122` 的 `removePendingEntry()` 永远收不到
/// 信号,"正在接收"占位卡片永驻直到用户 reload。
///
/// 检查两件事:
///   1. emitter 收到的事件序列是 [IncomingPending, NewContent](顺序固定)。
///   2. 两条事件 entry_id 相同 —— 前端靠 entry_id 把占位卡片下线、用真实
///      entry 替换;两边 id 不一致就等于占位卡片没被清。
#[tokio::test]
async fn happy_path_emits_incoming_pending_then_new_content() {
    let (input, hash) = fixture_input("regress: placeholder must be cleared");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(hash))
        .times(1)
        .returning(|_| Ok(None));

    // capture 把流程入口预生成的 receiver_entry_id 原样回填 —— 这是真实
    // capture 路径的契约(materializer / capture 都共享同一 entry_id)。
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|preset, _, _| Ok(Some(preset)));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let (uc, recorder) = build_with_recording_emitter(repo, capture, write);
    let outcome = uc.execute(input).await.expect("happy path");

    let applied_entry_id = match outcome {
        ApplyOutcome::Applied { entry_id } => entry_id,
        other => panic!("expected Applied, got {other:?}"),
    };

    let events = recorder.snapshot();
    assert_eq!(
        events.len(),
        2,
        "happy path must emit exactly 2 host events (IncomingPending + NewContent), got {} events: {:?}",
        events.len(),
        events
    );

    match &events[0] {
        HostEvent::Clipboard(ClipboardHostEvent::IncomingPending { entry_id, .. }) => {
            assert_eq!(
                entry_id,
                applied_entry_id.as_ref(),
                "IncomingPending entry_id must match the eventual Applied entry_id"
            );
        }
        other => panic!("event[0] must be IncomingPending, got {other:?}"),
    }

    match &events[1] {
        HostEvent::Clipboard(ClipboardHostEvent::NewContent {
            entry_id, origin, ..
        }) => {
            assert_eq!(
                entry_id,
                applied_entry_id.as_ref(),
                "NewContent entry_id must match — front end keys placeholder eviction by entry_id"
            );
            assert!(
                matches!(origin, ClipboardOriginKind::Remote),
                "inbound NewContent must carry origin=Remote, got {origin:?}"
            );
        }
        other => panic!("event[1] must be NewContent, got {other:?}"),
    }
}

// ── Layer ② upgrade routing: partial → complete in place ─────────────────

/// Availability fake reporting a fixed verdict for any entry id.
struct FakeAvailability {
    available: bool,
}

#[async_trait]
impl uc_core::ports::clipboard::CheckEntryAvailabilityPort for FakeAvailability {
    async fn is_entry_available(
        &self,
        _entry_id: &EntryId,
    ) -> std::result::Result<bool, ClipboardRepositoryError> {
        Ok(self.available)
    }
}

/// One inbound file delivery (uri-list rep + a single free-file blob ref).
fn file_blob_input() -> ApplyInboundInput {
    let original = SystemClipboardSnapshot {
        ts_ms: 1_700_000_000_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/payload.bin\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![1, 2, 3]),
        entry_id: EntryId::from("sender-entry"),
        filename: Some("payload.bin".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 10,
        representation_index: None,
    };
    let (plaintext, snapshot_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref]).unwrap();
    ApplyInboundInput {
        from_device: DeviceId::new("peer-up"),
        snapshot_hash,
        plaintext,
        flow_id: None,
        resurface_intent: ClipboardWriteIntent::RemotePush,
    }
}

/// Core of the three-entry fix: a hash match against a *partial* entry plus a
/// *complete* delivery upgrades the existing entry in place — capture is driven
/// under the partial's id (not a fresh one), and the outcome reuses that id.
#[tokio::test]
async fn partial_match_is_upgraded_in_place_by_complete_delivery() {
    let input = file_blob_input();
    let partial_id = EntryId::from("partial-entry");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning({
            let id = partial_id.clone();
            move |_| Ok(Some(id.clone()))
        });

    let mut materializer = MockBlobMaterializer::new();
    materializer.expect_materialize().times(1).returning(
        |_from_device, _receiver_entry_id, mut snapshot, _, _| {
            snapshot.representations[0]
                .set_inline_bytes(b"file:///local/cache/payload.bin\n".to_vec())
                .unwrap();
            Ok(MaterializeResult::complete(snapshot))
        },
    );

    // The replace route drives capture under the existing partial id (via the
    // `replace_with_identity` default → `capture`); the mock echoes that id back
    // so the outcome reuses it.
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .withf({
            let id = partial_id.clone();
            move |preset, _from_device, _snapshot| *preset == id
        })
        .times(1)
        .returning(|preset, _, _| Ok(Some(preset)));

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let uc = build_with_blob_materializer(repo, capture, write, materializer)
        .with_check_entry_availability(Arc::new(FakeAvailability { available: false }));
    let outcome = uc.execute(input).await.expect("upgrade path ok");
    assert_eq!(
        outcome,
        ApplyOutcome::Applied {
            entry_id: partial_id
        },
        "a completing delivery must upgrade the partial entry in place, reusing its id"
    );
}

/// A hash match that is fully available is skipped *before* any download —
/// neither materialize nor capture runs.
#[tokio::test]
async fn available_match_is_skipped_before_download() {
    let input = file_blob_input();
    let held_id = EntryId::from("held-entry");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning({
            let id = held_id.clone();
            move |_| Ok(Some(id.clone()))
        });

    // Materializer present but must never run (skip happens pre-download).
    let mut materializer = MockBlobMaterializer::new();
    materializer.expect_materialize().times(0);
    let capture = MockCapture::new(); // capture must not be called
    let mut write = MockWrite::new();
    write.expect_write().times(0);

    let uc = build_with_blob_materializer(repo, capture, write, materializer)
        .with_check_entry_availability(Arc::new(FakeAvailability { available: true }));
    let outcome = uc.execute(input).await.expect("skip path ok");
    assert_eq!(
        outcome,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: file_blob_input().snapshot_hash,
            existing_entry_id: held_id,
        }
    );
}

/// A partial delivery must NOT replace an existing partial entry (no thrashing
/// between two placeholders): the existing one is kept and the delivery skipped.
#[tokio::test]
async fn partial_delivery_does_not_replace_existing_partial() {
    let input = file_blob_input();
    let partial_id = EntryId::from("partial-entry");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning({
            let id = partial_id.clone();
            move |_| Ok(Some(id.clone()))
        });

    let mut materializer = MockBlobMaterializer::new();
    materializer.expect_materialize().times(1).returning(
        |_from_device, _receiver_entry_id, mut snapshot, _, _| {
            snapshot.representations[0]
                .set_inline_bytes(
                    b"uniclip-missing:///payload.bin?size=10&reason=cancelled".to_vec(),
                )
                .unwrap();
            Ok(MaterializeResult {
                snapshot,
                missing: vec![super::materializer::MissingFileRef {
                    filename: "payload.bin".to_string(),
                    size_bytes: 10,
                }],
                partial: true,
                // This fake stands in for the materializer; it publishes nothing.
                publication: None,
            })
        },
    );

    let capture = MockCapture::new(); // must not be called
    let mut write = MockWrite::new();
    write.expect_write().times(0);

    let uc = build_with_blob_materializer(repo, capture, write, materializer)
        .with_check_entry_availability(Arc::new(FakeAvailability { available: false }));
    let outcome = uc.execute(input).await.expect("partial-vs-partial skip ok");
    assert_eq!(
        outcome,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: file_blob_input().snapshot_hash,
            existing_entry_id: partial_id,
        }
    );
}

// ── resurface: a peer re-copying an older clip ──────────────────────
//
// Regression cover for the A → B → A bug: peer copies `abc`, then `xyz`, then
// `abc` again. The third delivery's content is already held here, so the old
// code dropped it and left the pasteboard on `xyz` until the periodic
// active-state broadcast repaired it up to a minute later.

fn resurface_uc(
    repo: MockEntryRepo,
    capture: MockCapture,
    write: MockWrite,
    rebuild: MockSnapshotRebuild,
    touch: MockTouchEntry,
) -> ApplyInboundClipboardUseCase {
    ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_resurface_ports(Arc::new(rebuild), Arc::new(touch))
}

/// The held entry is rebuilt from local storage and written to the OS
/// clipboard — never re-downloaded, never duplicated as a new row.
#[tokio::test]
async fn dedup_hit_rewrites_held_entry_to_os_clipboard() {
    let (input, hash) = fixture_input("abc");
    let existing = EntryId::from("entry-abc");

    let mut repo = MockEntryRepo::new();
    let existing_for_repo = existing.clone();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(hash.clone()))
        .times(1)
        .returning(move |_| Ok(Some(existing_for_repo.clone())));

    // No new row: capture must never be called on this path.
    let capture = MockCapture::new();

    let mut rebuild = MockSnapshotRebuild::new();
    let existing_for_rebuild = existing.clone();
    rebuild
        .expect_rebuild()
        .withf(move |id| id == &existing_for_rebuild)
        .times(1)
        .returning(|_| {
            Ok(SystemClipboardSnapshot {
                // The rebuilt snapshot carries the entry's *original* creation
                // time — deliberately different from the inbound activation
                // time asserted below.
                ts_ms: 1_600_000_000_000,
                representations: vec![ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("text"),
                    Some(MimeType("text/plain".to_string())),
                    b"abc".to_vec(),
                )],
                file_content_digests: Vec::new(),
                file_set_v1_component: None,
            })
        });

    let mut write = MockWrite::new();
    write
        .expect_write()
        .withf(|snapshot, intent| {
            *intent == ClipboardWriteIntent::RemotePush
                && snapshot.representations[0].inline_bytes() == Some(b"abc".as_slice())
        })
        .times(1)
        .returning(|_, _| Ok(()));

    let mut touch = MockTouchEntry::new();
    touch
        .expect_touch_entry()
        .times(1)
        .returning(|_, _| Ok(true));

    let uc = resurface_uc(repo, capture, write, rebuild, touch);
    let outcome = uc.execute(input).await.expect("resurface path ok");

    assert_eq!(
        outcome,
        ApplyOutcome::Resurfaced {
            snapshot_hash: hash,
            existing_entry_id: existing,
            os_write_succeeded: true,
        }
    );
}

/// The history bump must be stamped with the *inbound* activation time, not the
/// rebuilt snapshot's `created_at_ms`. Getting this wrong stamps the entry with
/// when the content was first seen, which loses the LWW comparison against the
/// clip it is meant to supersede.
#[tokio::test]
async fn resurface_stamps_the_inbound_activation_time_not_the_entrys_creation_time() {
    let (input, _hash) = fixture_input("abc");
    // `fixture_input` stamps the inbound snapshot with this observation time.
    let inbound_ts = 1_700_000_000_000i64;

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(Some(EntryId::from("entry-abc"))));

    let mut rebuild = MockSnapshotRebuild::new();
    rebuild.expect_rebuild().times(1).returning(|_| {
        Ok(SystemClipboardSnapshot {
            ts_ms: 1_600_000_000_000, // entry.created_at_ms — the wrong source
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"abc".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        })
    });

    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));

    let mut touch = MockTouchEntry::new();
    touch
        .expect_touch_entry()
        .withf(move |_, active_time_ms| *active_time_ms == inbound_ts)
        .times(1)
        .returning(|_, _| Ok(true));

    let uc = resurface_uc(repo, MockCapture::new(), write, rebuild, touch);
    uc.execute(input).await.expect("resurface path ok");
}

/// A rebuild failure (payload lost / blob gone) must not fabricate an
/// activation: fall back to the plain skip rather than writing a broken
/// snapshot to the pasteboard.
#[tokio::test]
async fn resurface_degrades_to_skip_when_rebuild_fails() {
    let (input, hash) = fixture_input("abc");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(Some(EntryId::from("entry-abc"))));

    let mut rebuild = MockSnapshotRebuild::new();
    rebuild
        .expect_rebuild()
        .times(1)
        .returning(|_| Err(anyhow::anyhow!("payload lost")));

    // Nothing may reach the clipboard or history.
    let write = MockWrite::new();
    let mut touch = MockTouchEntry::new();
    touch.expect_touch_entry().never();

    let uc = resurface_uc(repo, MockCapture::new(), write, rebuild, touch);
    let outcome = uc.execute(input).await.expect("degraded path ok");

    assert_eq!(
        outcome,
        ApplyOutcome::DuplicateSkipped {
            snapshot_hash: hash,
            existing_entry_id: EntryId::from("entry-abc"),
        }
    );
}

/// A failed OS write reports `os_write_succeeded: false` and skips the history
/// bump — callers use that flag to avoid broadcasting an activation this device
/// does not actually hold.
#[tokio::test]
async fn resurface_reports_failed_os_write_and_skips_history_bump() {
    let (input, _hash) = fixture_input("abc");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(Some(EntryId::from("entry-abc"))));

    let mut rebuild = MockSnapshotRebuild::new();
    rebuild.expect_rebuild().times(1).returning(|_| {
        Ok(SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"abc".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        })
    });

    let mut write = MockWrite::new();
    write
        .expect_write()
        .times(1)
        .returning(|_, _| Err(anyhow::anyhow!("clipboard busy")));

    let mut touch = MockTouchEntry::new();
    touch.expect_touch_entry().never();

    let uc = resurface_uc(repo, MockCapture::new(), write, rebuild, touch);
    let outcome = uc.execute(input).await.expect("failed-write path still ok");

    assert!(matches!(
        outcome,
        ApplyOutcome::Resurfaced {
            os_write_succeeded: false,
            ..
        }
    ));
}

/// A retried frame (or the same clip arriving over a second channel) must not
/// re-write the pasteboard again: only the first re-activation acts.
#[tokio::test]
async fn rapid_re_delivery_of_held_entry_resurfaces_only_once() {
    let (input, _hash) = fixture_input("abc");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(Some(EntryId::from("entry-abc"))));

    // Rebuild + write happen exactly once across the two deliveries.
    let mut rebuild = MockSnapshotRebuild::new();
    rebuild.expect_rebuild().times(1).returning(|_| {
        Ok(SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"abc".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        })
    });
    let mut write = MockWrite::new();
    write.expect_write().times(1).returning(|_, _| Ok(()));
    let mut touch = MockTouchEntry::new();
    touch
        .expect_touch_entry()
        .times(1)
        .returning(|_, _| Ok(true));

    let uc = resurface_uc(repo, MockCapture::new(), write, rebuild, touch);

    let first = uc.execute(input.clone()).await.expect("first ok");
    assert!(matches!(first, ApplyOutcome::Resurfaced { .. }));

    let second = uc.execute(input).await.expect("second ok");
    assert!(
        matches!(second, ApplyOutcome::DuplicateSkipped { .. }),
        "a rapid re-delivery must not touch the pasteboard again, got {second:?}"
    );
}

/// The rapid guard must not swallow a genuine repeat copy: once the window
/// lapses, the same content re-activates again (peer copies abc → xyz → abc).
#[tokio::test]
async fn repeat_copy_after_window_resurfaces_again() {
    let (input, _hash) = fixture_input("abc");

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(Some(EntryId::from("entry-abc"))));

    // Both deliveries act: this is the A → B → A case the fix exists for.
    let mut rebuild = MockSnapshotRebuild::new();
    rebuild.expect_rebuild().times(2).returning(|_| {
        Ok(SystemClipboardSnapshot {
            ts_ms: 1,
            representations: vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                b"abc".to_vec(),
            )],
            file_content_digests: Vec::new(),
            file_set_v1_component: None,
        })
    });
    let mut write = MockWrite::new();
    write.expect_write().times(2).returning(|_, _| Ok(()));
    let mut touch = MockTouchEntry::new();
    touch
        .expect_touch_entry()
        .times(2)
        .returning(|_, _| Ok(true));

    let uc = resurface_uc(repo, MockCapture::new(), write, rebuild, touch);

    let first = uc.execute(input.clone()).await.expect("first ok");
    assert!(matches!(first, ApplyOutcome::Resurfaced { .. }));

    // Past RAPID_DUPLICATE_WINDOW (200ms), matching how the sibling
    // visible-window test waits it out.
    tokio::time::sleep(std::time::Duration::from_millis(300)).await;

    let second = uc.execute(input).await.expect("second ok");
    assert!(
        matches!(second, ApplyOutcome::Resurfaced { .. }),
        "a deliberate repeat copy must re-activate, got {second:?}"
    );
}

// ---------------------------------------------------------------------------
// Directory publication: hidden staging, no-replace publish, multi-root
// compensating rollback (#1328).
// ---------------------------------------------------------------------------

/// Two independent roots, `alpha/one.txt` and `beta/two.txt` — the shape that
/// makes publication multi-step and therefore rollback-able.
fn two_root_payload() -> (
    Vec<V3BlobRef>,
    InboundFileSetManifest,
    SystemClipboardSnapshot,
) {
    use uc_core::clipboard::FileSetMemberKind;

    let blob_refs = ["one.txt", "two.txt"]
        .into_iter()
        .enumerate()
        .map(|(index, filename)| V3BlobRef {
            ticket: BlobTicket::from_bytes(vec![index as u8 + 1]),
            entry_id: EntryId::from(format!("sender-root-{index}")),
            filename: Some(filename.to_string()),
            mime: Some("text/plain".to_string()),
            size_bytes: 5,
            representation_index: None,
        })
        .collect::<Vec<_>>();
    let manifest = InboundFileSetManifest {
        members: vec![
            InboundFileSetMember {
                root_index: 0,
                root_name: "alpha".to_string(),
                root_is_file: false,
                relative_path: "one.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(0),
            },
            InboundFileSetMember {
                root_index: 1,
                root_name: "beta".to_string(),
                root_is_file: false,
                relative_path: "two.txt".to_string(),
                kind: FileSetMemberKind::File,
                blob_ref_index: Some(1),
            },
        ],
    };
    let snapshot = SystemClipboardSnapshot {
        ts_ms: 1,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("files"),
            Some(MimeType("text/uri-list".to_string())),
            b"file:///sender/alpha\r\nfile:///sender/beta\r\n".to_vec(),
        )],
        file_content_digests: Vec::new(),
        file_set_v1_component: None,
    };
    (blob_refs, manifest, snapshot)
}

fn writing_fetcher(times: usize) -> MockBlobFetcher {
    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(times)
        .returning(|command| {
            std::fs::write(&command.target_path, b"hello").expect("write member");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([5; 32]),
                digest: BlobDigest::from_bytes([7; 32]),
                bytes_written: 5,
            })
        });
    fetcher
}

fn directory_materializer(
    fetcher: MockBlobFetcher,
    cache_dir: &Path,
    save_dir: &Path,
    publisher: Arc<FakeAtomicPublisher>,
) -> FileCacheBlobMaterializer {
    FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.to_path_buf(), publisher)
        .with_save_dir_resolver(Arc::new(FakeSaveDir {
            dir: save_dir.to_path_buf(),
        }))
}

async fn materialize_two_roots(
    materializer: &FileCacheBlobMaterializer,
    entry_id: &str,
) -> Result<MaterializeResult> {
    let (blob_refs, manifest, snapshot) = two_root_payload();
    materializer
        .materialize(
            DeviceId::new("peer-pub"),
            EntryId::from(entry_id),
            snapshot,
            blob_refs,
            Some(manifest),
        )
        .await
}

/// AC: no intermediate artifact — not even an empty directory — appears at the
/// final path before publication succeeds.
#[tokio::test]
async fn directory_publication_leaves_the_final_path_empty_until_it_completes() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let observed: Arc<Mutex<Vec<Vec<String>>>> = Arc::new(Mutex::new(Vec::new()));

    let mut fetcher = MockBlobFetcher::new();
    let save_path = save_dir.path().to_path_buf();
    let recorder = Arc::clone(&observed);
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(move |command| {
            // Snapshot what the user would see mid-transfer.
            recorder.lock().unwrap().push(visible_names(&save_path));
            std::fs::write(&command.target_path, b"hello").expect("write member");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([5; 32]),
                digest: BlobDigest::from_bytes([7; 32]),
                bytes_written: 5,
            })
        });

    let materializer = directory_materializer(
        fetcher,
        cache_dir.path(),
        save_dir.path(),
        Arc::new(FakeAtomicPublisher::default()),
    );
    let mut result = materialize_two_roots(&materializer, "entry-invisible")
        .await
        .expect("materialize");

    for seen in observed.lock().unwrap().iter() {
        assert!(
            seen.is_empty(),
            "user saw {seen:?} in the save dir before publication completed"
        );
    }
    // Only after publication do both roots appear, together.
    assert_eq!(visible_names(save_dir.path()), vec!["alpha", "beta"]);
    result
        .take_publication()
        .expect("publication")
        .commit()
        .await;
    assert_eq!(visible_names(save_dir.path()), vec!["alpha", "beta"]);
    // Staging is gone once committed.
    assert_eq!(
        std::fs::read_dir(save_dir.path()).unwrap().count(),
        2,
        "staging area should be gone after commit"
    );
}

/// AC: never overwrite existing content; take the next name instead.
#[tokio::test]
async fn directory_publication_takes_the_next_name_rather_than_replacing_a_folder() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    // The user already has an `alpha/` holding their own work.
    std::fs::create_dir(save_dir.path().join("alpha")).unwrap();
    std::fs::write(save_dir.path().join("alpha/precious.txt"), b"mine").unwrap();

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::new(FakeAtomicPublisher::default()),
    );
    let mut result = materialize_two_roots(&materializer, "entry-collide")
        .await
        .expect("materialize");
    result
        .take_publication()
        .expect("publication")
        .commit()
        .await;

    assert_eq!(
        visible_names(save_dir.path()),
        vec!["alpha", "alpha (1)", "beta"]
    );
    // The user's folder is untouched and nothing merged into it.
    assert_eq!(
        std::fs::read(save_dir.path().join("alpha/precious.txt")).unwrap(),
        b"mine"
    );
    assert!(!save_dir.path().join("alpha/one.txt").exists());
    assert_eq!(
        std::fs::read(save_dir.path().join("alpha (1)/one.txt")).unwrap(),
        b"hello"
    );
}

/// AC: a name lost between pre-flight and publish costs a bounded retry, not
/// the other writer's data.
#[tokio::test]
async fn directory_publication_retries_onto_a_free_name_after_losing_a_race() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = FakeAtomicPublisher::losing_a_race_at(1);

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );
    let mut result = materialize_two_roots(&materializer, "entry-race")
        .await
        .expect("materialize");
    result
        .take_publication()
        .expect("publication")
        .commit()
        .await;

    // The racer holds `alpha`; our root stepped aside to `alpha (1)`.
    assert_eq!(
        visible_names(save_dir.path()),
        vec!["alpha", "alpha (1)", "beta"]
    );
    assert_eq!(
        std::fs::read(save_dir.path().join("alpha (1)/one.txt")).unwrap(),
        b"hello"
    );
    // 2 roots + 1 lost race.
    assert_eq!(publisher.publish_count(), 3);
}

/// AC: a mid-sequence failure withdraws the roots that already landed, leaving
/// the final location exactly as it was.
#[tokio::test]
async fn directory_publication_withdraws_earlier_roots_when_a_later_one_fails() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    std::fs::write(save_dir.path().join("unrelated.txt"), b"keep").unwrap();

    // Call 1 publishes `alpha`; call 2 fails on `beta`.
    let publisher = FakeAtomicPublisher::failing_at(&[(2, PublishError::Io("disk gone".into()))]);
    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );

    let err = materialize_two_roots(&materializer, "entry-rollback")
        .await
        .expect_err("a failed root must fail the receive");

    assert!(
        !format!("{err:#}").contains("partial_publication"),
        "rollback succeeded, so no partial publication should be reported: {err:#}"
    );
    // Zero residue: the published root is gone and the user's file survives.
    assert_eq!(visible_names(save_dir.path()), vec!["unrelated.txt"]);
    // Including the hidden staging area.
    assert_eq!(std::fs::read_dir(save_dir.path()).unwrap().count(), 1);
}

/// AC: when rollback itself fails, the receive still fails, and what stays
/// visible is reported as a count — never as names.
#[tokio::test]
async fn directory_publication_reports_a_count_when_rollback_cannot_withdraw() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");

    // 1: `alpha` publishes. 2: `beta` fails. 3: withdrawing `alpha` fails.
    let publisher = FakeAtomicPublisher::failing_at(&[
        (2, PublishError::Io("disk gone".into())),
        (3, PublishError::Io("still gone".into())),
    ]);
    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );

    let err = materialize_two_roots(&materializer, "entry-stuck")
        .await
        .expect_err("a failed root must fail the receive");

    let rendered = format!("{err:#}");
    assert!(
        rendered.contains("partial_publication: 1 root(s) left visible"),
        "expected a non-sensitive partial-publication marker, got: {rendered}"
    );
    // The stuck root is still there — that is the honest outcome being reported.
    assert_eq!(visible_names(save_dir.path()), vec!["alpha"]);
    assert!(
        !rendered.contains("alpha"),
        "root name leaked into the error: {rendered}"
    );
}

/// AC: root names and paths never reach a plaintext error string, on any
/// publication failure path.
#[tokio::test]
async fn directory_publication_failures_never_name_the_roots() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = FakeAtomicPublisher::failing_at(&[(1, PublishError::Io("nope".into()))]);
    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        publisher,
    );

    let err = materialize_two_roots(&materializer, "entry-redact")
        .await
        .expect_err("publication failed");

    let rendered = format!("{err:#}");
    for secret in ["alpha", "beta", "one.txt", "two.txt"] {
        assert!(
            !rendered.contains(secret),
            "`{secret}` leaked into a plaintext error: {rendered}"
        );
    }
    assert!(
        !rendered.contains(&save_dir.path().display().to_string()),
        "destination path leaked into a plaintext error: {rendered}"
    );
}

/// A save-dir volume that cannot refuse a replace is not used at all: the
/// content still lands, in managed storage, rather than risking the user's
/// files.
#[tokio::test]
async fn directory_publication_falls_back_to_managed_storage_when_the_volume_cannot_refuse() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = FakeAtomicPublisher::unsupported_under(save_dir.path());

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        publisher,
    );
    let mut result = materialize_two_roots(&materializer, "entry-fallback")
        .await
        .expect("materialize should still succeed via managed storage");
    result
        .take_publication()
        .expect("publication")
        .commit()
        .await;

    // Nothing was written to the user's folder, not even a probe leftover.
    assert_eq!(std::fs::read_dir(save_dir.path()).unwrap().count(), 0);
    let managed = cache_dir.path().join("iroh-blobs").join("entry-fallback");
    assert_eq!(
        std::fs::read(managed.join("alpha/one.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        std::fs::read(managed.join("beta/two.txt")).unwrap(),
        b"hello"
    );
}

/// The install where no volume in reach honors the primitive — a portable
/// build on exFAT, a home on NFS. Managed storage is ours and scoped to one
/// receive, so the guarantee protects nothing there and is not demanded:
/// directory sync keeps working instead of failing outright.
#[tokio::test]
async fn directory_publication_still_lands_when_no_volume_supports_no_replace() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = FakeAtomicPublisher::unsupported_everywhere();

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );
    let mut result = materialize_two_roots(&materializer, "entry-nofs")
        .await
        .expect("a volume without the primitive must not cost the receive");
    result
        .take_publication()
        .expect("publication")
        .commit()
        .await;

    // The user's folder was declined — its volume cannot protect what is in
    // it — and the content landed in managed storage regardless.
    assert_eq!(std::fs::read_dir(save_dir.path()).unwrap().count(), 0);
    let managed = cache_dir.path().join("iroh-blobs").join("entry-nofs");
    assert_eq!(
        std::fs::read(managed.join("alpha/one.txt")).unwrap(),
        b"hello"
    );
    assert_eq!(
        std::fs::read(managed.join("beta/two.txt")).unwrap(),
        b"hello"
    );
}

/// The same install, taken all the way through rollback: withdrawal runs on
/// the same volume that could not honor the primitive, so it must not lean on
/// it either.
#[tokio::test]
async fn rollback_withdraws_on_a_volume_without_no_replace() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = FakeAtomicPublisher::unsupported_everywhere();

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );
    let mut result = materialize_two_roots(&materializer, "entry-nofs-rb")
        .await
        .expect("materialize");

    let outcome = result
        .take_publication()
        .expect("publication")
        .rollback()
        .await;

    assert_eq!(outcome, RollbackOutcome::Clean);
    let managed = cache_dir.path().join("iroh-blobs").join("entry-nofs-rb");
    assert_eq!(visible_names(&managed), Vec::<String>::new());
}

/// Support is a property of the volume, not of the receive. Asking once per
/// paste is wasted work — several network round trips on a NAS save dir.
#[tokio::test]
async fn the_no_replace_probe_is_not_repeated_per_receive() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let publisher = Arc::new(FakeAtomicPublisher::default());

    let materializer = directory_materializer(
        writing_fetcher(4),
        cache_dir.path(),
        save_dir.path(),
        Arc::clone(&publisher),
    );
    for entry_id in ["entry-probe-a", "entry-probe-b"] {
        let mut result = materialize_two_roots(&materializer, entry_id)
            .await
            .expect("materialize");
        result
            .take_publication()
            .expect("publication")
            .commit()
            .await;
    }

    assert_eq!(
        publisher.probe_count(),
        1,
        "the save dir's volume was probed more than once"
    );
}

/// AC: crash debris lives only in a recognizable hidden area, and a sweep
/// reclaims it without touching anything else.
#[tokio::test]
async fn sweep_reclaims_hidden_staging_and_leaves_everything_else_alone() {
    let dir = tempfile::tempdir().expect("dir");
    // Debris from an interrupted receive.
    let debris = dir.path().join(".uniclip-incoming-entry-crashed");
    std::fs::create_dir_all(debris.join("0-alpha")).unwrap();
    std::fs::write(debris.join("0-alpha/half.txt"), b"partial").unwrap();
    // The user's own content, including an unrelated dotfile.
    std::fs::create_dir(dir.path().join("alpha")).unwrap();
    std::fs::write(dir.path().join("alpha/mine.txt"), b"mine").unwrap();
    std::fs::write(dir.path().join(".hidden-but-theirs"), b"theirs").unwrap();

    let swept = sweep_inbound_staging(&[dir.path().to_path_buf()]).await;

    assert_eq!(swept, 1);
    assert!(!debris.exists());
    assert_eq!(
        std::fs::read(dir.path().join("alpha/mine.txt")).unwrap(),
        b"mine"
    );
    assert!(dir.path().join(".hidden-but-theirs").exists());
}

#[tokio::test]
async fn sweep_tolerates_a_missing_directory() {
    let dir = tempfile::tempdir().expect("dir");
    assert_eq!(sweep_inbound_staging(&[dir.path().join("absent")]).await, 0);
}

/// Build a full two-root inbound payload plus the wire hash the sender would
/// have advertised for it.
fn two_root_wire_payload() -> (Bytes, String) {
    use std::collections::BTreeMap;

    let (blob_refs, manifest, mut snapshot) = two_root_payload();
    // The sender advertises an identity derived from the content it sent.
    snapshot.file_set_v1_component = Some(
        compute_file_set_component(&manifest, &BTreeMap::from([(0, [5; 32]), (1, [5; 32])]))
            .expect("file-set component"),
    );
    encode_snapshot_with_blob_refs_and_file_set_to_v3_bytes(&snapshot, &blob_refs, &manifest)
        .expect("encode directory payload")
}

/// AC: publication succeeding is not enough — content that fails verification
/// after landing is withdrawn again.
#[tokio::test]
async fn apply_inbound_withdraws_published_roots_when_identity_verification_fails() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let (plaintext, snapshot_hash) = two_root_wire_payload();

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    // The bytes that arrive hash to something other than what was advertised.
    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(2)
        .returning(|command| {
            std::fs::write(&command.target_path, b"tampered").expect("write member");
            Ok(crate::facade::blob_transfer::FetchBlobToPathResult {
                entry_id: command.entry_id,
                plaintext_hash: PlaintextHash::from_bytes([9; 32]),
                digest: BlobDigest::from_bytes([7; 32]),
                bytes_written: 8,
            })
        });

    let mut capture = MockCapture::new();
    capture.expect_capture().never();
    let mut write = MockWrite::new();
    write.expect_write().never();

    let materializer = directory_materializer(
        fetcher,
        cache_dir.path(),
        save_dir.path(),
        Arc::new(FakeAtomicPublisher::default()),
    );
    let uc = ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_blob_materializer(Arc::new(materializer));

    let err = uc
        .execute(ApplyInboundInput {
            from_device: DeviceId::new("peer-verify-fail"),
            snapshot_hash,
            plaintext,
            flow_id: None,
            resurface_intent: ClipboardWriteIntent::RemotePush,
        })
        .await
        .expect_err("identity mismatch must fail the delivery");

    assert!(matches!(err, ApplyInboundError::Internal(_)), "{err:?}");
    // The roots were published, then taken back: zero residue.
    assert!(
        visible_names(save_dir.path()).is_empty(),
        "content that failed verification stayed visible: {:?}",
        visible_names(save_dir.path())
    );
    assert_eq!(std::fs::read_dir(save_dir.path()).unwrap().count(), 0);
}

/// AC: the commit point is a durable receipt — if persistence fails, the roots
/// go back, so the user never keeps a folder no entry knows about.
#[tokio::test]
async fn apply_inbound_withdraws_published_roots_when_persistence_fails() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let (plaintext, snapshot_hash) = two_root_wire_payload();

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .times(1)
        .returning(|_, _, _| Err(anyhow::anyhow!("database is locked")));
    let mut write = MockWrite::new();
    write.expect_write().never();

    let materializer = directory_materializer(
        writing_fetcher(2),
        cache_dir.path(),
        save_dir.path(),
        Arc::new(FakeAtomicPublisher::default()),
    );
    let uc = ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_blob_materializer(Arc::new(materializer));

    let err = uc
        .execute(ApplyInboundInput {
            from_device: DeviceId::new("peer-persist-fail"),
            snapshot_hash,
            plaintext,
            flow_id: None,
            resurface_intent: ClipboardWriteIntent::RemotePush,
        })
        .await
        .expect_err("a failed receipt must fail the delivery");

    assert!(matches!(err, ApplyInboundError::Capture(_)), "{err:?}");
    assert!(
        visible_names(save_dir.path()).is_empty(),
        "roots outlived the entry that failed to persist: {:?}",
        visible_names(save_dir.path())
    );
    assert_eq!(std::fs::read_dir(save_dir.path()).unwrap().count(), 0);
}

/// Records every field value and message of every event, so a test can assert
/// on what actually reached the logs.
#[derive(Clone, Default)]
struct RecordEverythingLayer {
    lines: Arc<Mutex<Vec<String>>>,
}

impl<S> Layer<S> for RecordEverythingLayer
where
    S: Subscriber,
    S: for<'lookup> LookupSpan<'lookup>,
{
    fn on_event(&self, event: &tracing::Event<'_>, _ctx: Context<'_, S>) {
        #[derive(Default)]
        struct Sink(String);
        impl Visit for Sink {
            fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!(" {}={value:?}", field.name()));
            }
            fn record_str(&mut self, field: &Field, value: &str) {
                self.0.push_str(&format!(" {}={value}", field.name()));
            }
        }
        let mut sink = Sink::default();
        event.record(&mut sink);
        self.lines.lock().unwrap().push(sink.0);
    }
}

/// AC: root names and paths appear in no plaintext log, on the worst path —
/// a failed publication whose rollback also fails.
#[tokio::test]
async fn directory_publication_logs_never_carry_root_names_or_paths() {
    let cache_dir = tempfile::tempdir().expect("cache dir");
    let save_dir = tempfile::tempdir().expect("save dir");
    let lines = Arc::new(Mutex::new(Vec::new()));

    {
        let subscriber = tracing_subscriber::registry().with(RecordEverythingLayer {
            lines: Arc::clone(&lines),
        });
        let _guard = tracing::subscriber::set_default(subscriber);

        // `beta` fails to publish, then withdrawing `alpha` fails too — the
        // path that has the most to say and the most to leak.
        let publisher = FakeAtomicPublisher::failing_at(&[
            (2, PublishError::Io("disk gone".into())),
            (3, PublishError::Io("still gone".into())),
        ]);
        let materializer = directory_materializer(
            writing_fetcher(2),
            cache_dir.path(),
            save_dir.path(),
            publisher,
        );
        let _ = materialize_two_roots(&materializer, "entry-log-redact").await;
    }

    let logged = lines.lock().unwrap().join("\n");
    assert!(
        logged.contains("could not be withdrawn"),
        "expected the failed withdrawal to be logged at all: {logged}"
    );
    for secret in ["alpha", "beta", "one.txt", "two.txt"] {
        assert!(
            !logged.contains(secret),
            "`{secret}` reached a plaintext log: {logged}"
        );
    }
    assert!(
        !logged.contains(&save_dir.path().display().to_string()),
        "a destination path reached a plaintext log: {logged}"
    );
    // What the plaintext side is allowed to carry: a count, not an identity.
    assert!(
        logged.contains("visible_roots=1"),
        "expected a non-sensitive count marker: {logged}"
    );
}
