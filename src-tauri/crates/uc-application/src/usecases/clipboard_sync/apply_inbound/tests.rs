use std::sync::{Arc, Mutex, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use mockall::predicate::*;

use crate::facade::host_event::{
    ClipboardHostEvent, ClipboardOriginKind, EmitError, HostEvent, HostEventEmitterPort,
};

use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::blob::{BlobDigest, BlobTicket, PlaintextHash};
use uc_core::ports::{ClipboardEntryRepositoryPort, PeerAddressError};
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

use crate::usecases::clipboard_sync::payload_codec::{
    encode_snapshot_to_v3_bytes, encode_snapshot_with_blob_refs_to_v3_bytes, V3BlobRef,
};

use super::materializer::{FileCacheBlobMaterializer, InboundBlobFetcher, InboundBlobMaterializer};
use super::ports::{InboundCapture, InboundWrite};
use super::usecase::ApplyInboundClipboardUseCase;
use super::{ApplyInboundError, ApplyInboundInput, ApplyOutcome};

// ── mockall: the 3 collaborator surfaces ────────────────────────────

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
        ) -> Result<SystemClipboardSnapshot>;
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
    let cell: crate::facade::blob_transfer::SharedHostEventEmitter = Arc::new(RwLock::new(
        Arc::clone(&recorder) as Arc<dyn HostEventEmitterPort>,
    ));
    let uc = ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write))
        .with_host_event_emitter(cell);
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
        .returning(|_, _| Ok(Some(EntryId::from("entry-new"))));

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
        .returning(|_, _| Ok(Some(EntryId::from("entry-first"))));

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
        .returning(|_, _| Ok(Some(EntryId::from("entry-visible"))));

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
        .returning(|_, _| Ok(Some(EntryId::new())));

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
    capture.expect_capture().times(1).returning(|_, _| Ok(None));

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
        .returning(|_, _| Ok(Some(EntryId::from("entry-committed"))));

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
            snapshot.representations[0].bytes == b"file:///sender/original.txt\n"
                && refs == &vec![blob_ref.clone()]
        })
        .returning(|_from_device, _receiver_entry_id, mut snapshot, _| {
            snapshot.representations[0].bytes = b"file:///local/cache/original.txt\n".to_vec();
            Ok(snapshot)
        });

    let assert_local_file = |snapshot: &SystemClipboardSnapshot| {
        snapshot.representations[0].bytes == b"file:///local/cache/original.txt\n"
    };
    let mut capture = MockCapture::new();
    capture
        .expect_capture()
        .withf(move |_preset_entry_id, snapshot| assert_local_file(snapshot))
        .times(1)
        .returning(|_, _| Ok(Some(EntryId::from("entry-new"))));

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

    let uri_list = String::from_utf8(rewritten.representations[0].bytes.clone())
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

    assert_eq!(materialized.representations.len(), 1);
    assert_eq!(materialized.representations[0].bytes, b"\x89PNG\x0d");
    assert_eq!(materialized.representations[0].format_id.as_ref(), "image");

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
        .returning(|preset, _| Ok(Some(preset)));

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
