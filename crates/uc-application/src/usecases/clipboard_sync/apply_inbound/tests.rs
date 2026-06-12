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

use crate::facade::host_event::{
    ClipboardHostEvent, ClipboardOriginKind, EmitError, HostEvent, HostEventBus,
    HostEventEmitterPort,
};

use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::blob::{BlobDigest, BlobTicket, PlaintextHash};
use uc_core::ports::{ClipboardEntryRepositoryPort, PeerAddressError};
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_observability::FlowId;

use crate::usecases::clipboard_sync::payload_codec::{
    encode_snapshot_to_v3_bytes, encode_snapshot_with_blob_refs_to_v3_bytes, V3BlobRef,
};

use super::materializer::{
    FileCacheBlobMaterializer, InboundBlobFetcher, InboundBlobMaterializer, MaterializeResult,
};
use super::ports::{InboundCapture, InboundWrite};
use super::usecase::ApplyInboundClipboardUseCase;
use super::{ApplyInboundError, ApplyInboundInput, ApplyOutcome};

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
    impl ClipboardEntryRepositoryPort for EntryRepo {
        async fn save_entry_and_selection(
            &self,
            entry: &uc_core::ClipboardEntry,
            selection: &uc_core::ClipboardSelectionDecision,
        ) -> Result<()>;
        async fn get_entry(&self, entry_id: &EntryId) -> Result<Option<uc_core::ClipboardEntry>>;
        async fn list_entries(&self, limit: usize, offset: usize) -> Result<Vec<uc_core::ClipboardEntry>>;
        async fn touch_entry(&self, entry_id: &EntryId, active_time_ms: i64) -> Result<bool>;
        async fn delete_entry(&self, entry_id: &EntryId) -> Result<()>;
        async fn find_entry_id_by_snapshot_hash(&self, snapshot_hash: &str) -> Result<Option<EntryId>>;
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
        async fn write(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
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
    };
    let (plaintext, content_hash) = encode_snapshot_to_v3_bytes(&snapshot).unwrap();
    (
        ApplyInboundInput {
            from_device: DeviceId::new("peer-x"),
            content_hash: content_hash.clone(),
            plaintext,
            flow_id: None,
        },
        content_hash,
    )
}

fn fixture_input_from_snapshot(snapshot: SystemClipboardSnapshot) -> (ApplyInboundInput, String) {
    let (plaintext, content_hash) = encode_snapshot_to_v3_bytes(&snapshot).unwrap();
    (
        ApplyInboundInput {
            from_device: DeviceId::new("peer-x"),
            content_hash: content_hash.clone(),
            plaintext,
            flow_id: None,
        },
        content_hash,
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
    write.expect_write().times(1).returning(|_| Ok(()));

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
    write.expect_write().times(1).returning(|_| Ok(()));

    let uc = build(repo, capture, write);
    uc.execute(input).await.expect("forwarding path ok");
}

/// Verdict 2 — dedup hit: returns `DuplicateSkipped` and **does
/// not** call capture or write. Critical correctness property —
/// repeated dispatches from a peer must not double-write the user's
/// OS clipboard (Phase 3 acceptance #4).
#[tokio::test]
async fn duplicate_skipped_when_hash_already_local() {
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
            content_hash: hash,
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
    write.expect_write().times(1).returning(|_| Ok(()));

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
            content_hash: hash,
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
    write.expect_write().times(1).returning(|_| Ok(()));

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
            content_hash: second_hash,
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
    };
    let second_snapshot = SystemClipboardSnapshot {
        ts_ms: 1_700_000_003_000,
        representations: vec![ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("public.utf8-plain-text"),
            None,
            b"expires".to_vec(),
        )],
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
    write.expect_write().times(2).returning(|_| Ok(()));

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
        content_hash: "blake3v1:00".to_string(),
        plaintext: Bytes::from_static(b"not a valid V3 envelope"),
        flow_id: None,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
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
        content_hash: "blake3v1:11".to_string(),
        plaintext: Bytes::from_static(b"not a valid V3 envelope"),
        flow_id: Some(incoming_flow_id.clone()),
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
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
    write.expect_write().times(1).returning(move |_| {
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
        .returning(|_| {
            Err(anyhow::Error::from(PeerAddressError::Internal(
                "db down".to_string(),
            )))
        });
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
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![9, 8, 7]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("original.txt".to_string()),
        mime: Some("text/plain".to_string()),
        size_bytes: 13,
        representation_index: None,
    };
    let (plaintext, content_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-x"),
        content_hash: content_hash.clone(),
        plaintext,
        flow_id: None,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .with(eq(content_hash))
        .times(1)
        .returning(|_| Ok(None));

    let mut materializer = MockBlobMaterializer::new();
    materializer
        .expect_materialize()
        .times(1)
        .withf(move |_from_device, _receiver_entry_id, snapshot, refs| {
            snapshot.representations[0].expect_inline_bytes() == b"file:///sender/original.txt\n"
                && refs == &vec![blob_ref.clone()]
        })
        .returning(|_from_device, _receiver_entry_id, mut snapshot, _| {
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
        .withf(move |snapshot| assert_local_file(snapshot))
        .times(1)
        .returning(|_| Ok(()));

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
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![5, 5, 5]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("big.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 950_000_000,
        representation_index: None,
    };
    let (plaintext, content_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        content_hash,
        plaintext,
        flow_id: None,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(1)
        .returning(|_| Ok(None));

    // materializer 模拟用户中途 cancel:返回 partial,snapshot 里 file-list rep
    // 已被重写为带 uniclip-missing:// 占位,missing 列表非空。
    let mut materializer = MockBlobMaterializer::new();
    materializer.expect_materialize().times(1).returning(
        |_from_device, _receiver_entry_id, mut snapshot, _| {
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
    };
    let blob_ref = V3BlobRef {
        ticket: BlobTicket::from_bytes(vec![7, 7, 7]),
        entry_id: EntryId::from("entry-remote"),
        filename: Some("retry.iso".to_string()),
        mime: Some("application/octet-stream".to_string()),
        size_bytes: 100,
        representation_index: None,
    };
    let (plaintext, content_hash) =
        encode_snapshot_with_blob_refs_to_v3_bytes(&original, &[blob_ref.clone()]).unwrap();
    let input1 = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        content_hash: content_hash.clone(),
        plaintext: plaintext.clone(),
        flow_id: None,
    };
    let input2 = ApplyInboundInput {
        from_device: DeviceId::new("peer-sender"),
        content_hash,
        plaintext,
        flow_id: None,
    };

    let mut repo = MockEntryRepo::new();
    repo.expect_find_entry_id_by_snapshot_hash()
        .times(2)
        .returning(|_| Ok(None));

    // 第一次 partial,第二次成功(模拟用户取消后重传成功)。
    let mut materializer = MockBlobMaterializer::new();
    let call_count = std::sync::atomic::AtomicUsize::new(0);
    materializer.expect_materialize().times(2).returning(
        move |_from_device, _receiver_entry_id, mut snapshot, _| {
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
    write.expect_write().times(1).returning(|_| Ok(()));

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

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let rewritten = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
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

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let materialized = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
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

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let err = materializer
        .materialize(
            DeviceId::new("peer-x"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
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

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref_ok, blob_ref_cancel],
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
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob_to_path()
        .times(1)
        .returning(|_| Err(anyhow::Error::from(BlobTransferError::Cancelled)));

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
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
    };

    let mut fetcher = MockBlobFetcher::new();
    fetcher
        .expect_fetch_blob()
        .times(1)
        .returning(|_| Err(anyhow::Error::from(BlobTransferError::Cancelled)));

    let materializer =
        FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
    let result = materializer
        .materialize(
            DeviceId::new("peer-sender"),
            EntryId::from("entry-receiver"),
            snapshot,
            vec![blob_ref],
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
    write.expect_write().times(1).returning(|_| Ok(()));

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
