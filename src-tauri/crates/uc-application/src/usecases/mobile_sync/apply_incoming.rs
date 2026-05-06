//! `ApplyIncomingMobileClipUseCase` —— mobile sync 入站剪贴板的应用层入口。
//!
//! 把 iPhone 经 SyncClipboard 协议 (`PUT /SyncClipboard.json` +
//! `PUT /file/{name}`) 上传的 text / image / file 翻成项目内部的
//! [`SystemClipboardSnapshot`], 编码成 V3 envelope, 喂给已有的
//! [`ApplyInboundClipboardUseCase`] 复用整套入站管线 ——
//! dedup / capture / OS 写回 + 60s 写回环防御。
//!
//! ## 两步 PUT 协议
//!
//! SyncClipboard EX 在上传非纯文本时, 客户端先 `PUT /file/{dataName}` 把
//! 字节 buffer 上来, 再 `PUT /SyncClipboard.json` 提交元数据(含 `dataName`
//! 指向那条 file)。daemon 在第一步必须把字节缓存住 —— 这就是
//! [`IncomingMobileBuffer`] 的角色, 不是 stub, 而是协议必要部件。
//! 第二步触发时再从 buffer 取出, 拼 `SystemClipboardSnapshot` 走 capture。
//!
//! ## v1 范围 (P5a.3)
//!
//! - **Text** ✅ 直接编码成 `text/plain` rep
//! - **Image** ✅ 从 buffer 取 `(mime, bytes)` 拼 `image/*` rep
//! - **File** ⏭ 返回 `DecodeFailed { reason: "v1 暂不支持" }`
//!   原因: file-list rep 需要文件系统路径(URI list 形态), 接收端必须把
//!   裸字节物化到磁盘后才能构成 `file:///path/foo.pdf` 形态的 rep。这一
//!   条 staging port + adapter 的引入推到 P5a.3.5 单独子步骤。
//! - **Group** ⏭ 返回 `DecodeFailed`(SyncClipboard 协议本身保留语义,
//!   shortcut EX 客户端不会发, 不在 v1 实现)
//!
//! ## 写回环防御复用
//!
//! 不需要在本 use case 里做 hash guard / next-origin override —— 这些
//! 在 [`ApplyInboundClipboardUseCase`] 内部 `InboundWrite` (调
//! `ClipboardWriteCoordinator::write(.., RemotePush)`) 已经备齐, 60s
//! 防御链对 mobile sync 入站同样自动生效。
//!
//! ## 命名空间隔离
//!
//! 写回 `clipboard_event.from_device` 的伪 DeviceId 是
//! `mobile_sync:<device_id>` 形态, 让日志 / UI 一眼能区分"这条不是 P2P
//! 来的"。不会与现有 P2P DeviceId(libp2p PeerId 派生)冲突。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use thiserror::Error;
use tracing::{debug, info, instrument, warn};

use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::mobile_sync::MobileDeviceId;
use uc_core::ports::ClockPort;
use uc_core::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};

use crate::usecases::clipboard_sync::apply_inbound::{
    ApplyInboundClipboardUseCase, ApplyInboundError, ApplyInboundInput, ApplyOutcome,
};
use crate::usecases::clipboard_sync::payload_codec::encode_snapshot_to_v3_bytes;
use crate::usecases::mobile_sync::clipboard_doc::SyncClipboardItemType;

// ─── Public types (pub(crate) per AGENTS.md §11.4) ──────────────────────

/// Input to [`ApplyIncomingMobileClipUseCase::execute`].
#[derive(Debug, Clone)]
pub(crate) struct ApplyIncomingMobileClipInput {
    /// Authenticated mobile device id (registered iPhone). Becomes the
    /// suffix of the pseudo `DeviceId` written into the clipboard event.
    pub source_device_id: MobileDeviceId,
    /// What kind of incoming event this is. See [`IncomingMobileClipEvent`].
    pub event: IncomingMobileClipEvent,
}

/// Two-shape input — one variant per HTTP route that calls into us.
#[derive(Debug, Clone)]
pub(crate) enum IncomingMobileClipEvent {
    /// Triggered by `PUT /SyncClipboard.json`. Commits to apply the clip
    /// into the local clipboard (capture + OS write).
    SyncDoc {
        item_type: SyncClipboardItemType,
        /// `meta.text` from SyncClipboard wire schema. For Text type
        /// it carries the actual content; for Image / File it's the
        /// original filename (purely informational, not used by us).
        text: String,
        /// `meta.dataName` from SyncClipboard wire schema. Required
        /// for Image / File types — points to the file-buffer entry.
        data_name: Option<String>,
    },
    /// Triggered by `PUT /file/{data_name}`. Stages bytes in the buffer,
    /// returns immediately with `Buffered` outcome — the caller must
    /// still respond HTTP 200.
    BufferFile {
        data_name: String,
        mime: String,
        bytes: Vec<u8>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ApplyIncomingMobileClipOutcome {
    /// New content — persisted via capture + OS clipboard written.
    Applied { entry_id: EntryId },
    /// `content_hash` already exists locally — no persist, no OS write.
    /// Mirrors `ApplyOutcome::DuplicateSkipped`.
    DuplicateSkipped {
        content_hash: String,
        existing_entry_id: EntryId,
    },
    /// Decode-time / contract failure (unsupported type, missing
    /// `data_name`, buffer miss, ...). Routes layer maps to HTTP 400.
    DecodeFailed { reason: String },
    /// Bytes buffered, awaiting sibling `PUT /SyncClipboard.json`.
    /// Routes layer responds HTTP 200 — this is the protocol's expected
    /// shape, not an error.
    Buffered,
}

#[derive(Debug, Error)]
pub(crate) enum ApplyIncomingMobileClipError {
    /// Underlying [`ApplyInboundClipboardUseCase`] failed in a way that
    /// is **not** expressible as a domain outcome (DB error, capture
    /// pipeline crash, OS write coord failure).
    #[error("inbound apply failed: {0}")]
    Inbound(#[from] ApplyInboundError),
    /// V3 envelope encode failed.
    #[error("V3 envelope encode failed: {0}")]
    EncodeFailed(String),
    /// Catch-all for use-case-internal logic errors.
    #[error("internal: {0}")]
    Internal(String),
}

// ─── IncomingMobileBuffer ────────────────────────────────────────────────

const MAX_BUFFERED_FILES: usize = 16;

#[derive(Debug, Clone)]
struct BufferedFile {
    mime: String,
    bytes: Bytes,
}

/// In-process staging buffer for `PUT /file/{name}` bytes between the
/// two-step PUT (file → json) protocol.
///
/// **Bounded** by [`MAX_BUFFERED_FILES`] entries — when full and an
/// unseen `data_name` arrives, one existing entry is dropped to keep
/// the bound.
///
/// v1 trade-off: orphaned entries (a `PUT /file` with no follow-up
/// `PUT /json`) leak until the size cap kicks in or daemon restarts.
/// P5a.10 真机回归会确认是否需要加 TTL sweep —— 在那之前, 16 条上限
/// 给"快速连续上传几张图片"留够余量, 又不至于让 OOM 风险窜起来。
pub(crate) struct IncomingMobileBuffer {
    inner: Mutex<HashMap<String, BufferedFile>>,
}

impl IncomingMobileBuffer {
    pub(crate) fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    fn store(&self, data_name: String, mime: String, bytes: Vec<u8>) {
        let mut guard = self.inner.lock().unwrap();
        if guard.len() >= MAX_BUFFERED_FILES && !guard.contains_key(&data_name) {
            // Cap reached + new key. Drop one arbitrary existing entry
            // to make room. v1: HashMap iter order; if real-world traffic
            // shows orphan accumulation we'll switch to LRU in P5a.3.5+.
            if let Some(victim) = guard.keys().next().cloned() {
                warn!(
                    victim = %victim,
                    cap = MAX_BUFFERED_FILES,
                    "mobile_sync IncomingMobileBuffer full; dropping oldest entry"
                );
                guard.remove(&victim);
            }
        }
        guard.insert(
            data_name,
            BufferedFile {
                mime,
                bytes: Bytes::from(bytes),
            },
        );
    }

    fn take(&self, data_name: &str) -> Option<BufferedFile> {
        self.inner.lock().unwrap().remove(data_name)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.lock().unwrap().len()
    }
}

impl Default for IncomingMobileBuffer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Use Case ────────────────────────────────────────────────────────────

pub(crate) struct ApplyIncomingMobileClipUseCase {
    inbound: Arc<ApplyInboundClipboardUseCase>,
    buffer: Arc<IncomingMobileBuffer>,
    clock: Arc<dyn ClockPort>,
}

impl ApplyIncomingMobileClipUseCase {
    pub(crate) fn new(
        inbound: Arc<ApplyInboundClipboardUseCase>,
        buffer: Arc<IncomingMobileBuffer>,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        Self {
            inbound,
            buffer,
            clock,
        }
    }

    #[instrument(
        name = "mobile_sync.apply_incoming",
        skip_all,
        fields(source_device_id = %input.source_device_id),
    )]
    pub(crate) async fn execute(
        &self,
        input: ApplyIncomingMobileClipInput,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        match input.event {
            IncomingMobileClipEvent::BufferFile {
                data_name,
                mime,
                bytes,
            } => {
                let bytes_len = bytes.len();
                self.buffer.store(data_name.clone(), mime.clone(), bytes);
                info!(
                    data_name = %data_name,
                    mime = %mime,
                    bytes = bytes_len,
                    "mobile_sync apply_incoming: buffered file"
                );
                Ok(ApplyIncomingMobileClipOutcome::Buffered)
            }
            IncomingMobileClipEvent::SyncDoc {
                item_type,
                text,
                data_name,
            } => {
                let snapshot_result = match item_type {
                    SyncClipboardItemType::Text => self.build_text_snapshot(text),
                    SyncClipboardItemType::Image => self.build_image_snapshot(data_name),
                    SyncClipboardItemType::File => Err(
                        "File 类型 v1 暂不支持 (P5a.3.5 待实现 file-list staging port)".to_string(),
                    ),
                    SyncClipboardItemType::Group => {
                        Err("Group 类型不在 v1 范围内 (SyncClipboard 协议保留)".to_string())
                    }
                };

                let snapshot = match snapshot_result {
                    Ok(s) => s,
                    Err(reason) => {
                        warn!(
                            item_type = ?item_type,
                            reason = %reason,
                            "mobile_sync apply_incoming: decode failed"
                        );
                        return Ok(ApplyIncomingMobileClipOutcome::DecodeFailed { reason });
                    }
                };

                self.dispatch_inbound(input.source_device_id, snapshot)
                    .await
            }
        }
    }

    fn build_text_snapshot(&self, text: String) -> Result<SystemClipboardSnapshot, String> {
        if text.is_empty() {
            return Err("Text item with empty body".into());
        }
        let bytes = text.into_bytes();
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            bytes,
        );
        Ok(SystemClipboardSnapshot {
            ts_ms: self.clock.now_ms(),
            representations: vec![rep],
        })
    }

    fn build_image_snapshot(
        &self,
        data_name: Option<String>,
    ) -> Result<SystemClipboardSnapshot, String> {
        let name = data_name.ok_or_else(|| "Image item without dataName".to_string())?;
        let buffered = self.buffer.take(&name).ok_or_else(|| {
            format!(
                "file buffer miss for `{}` (PUT /file may have arrived late or never)",
                name
            )
        })?;
        let rep = ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from("image"),
            Some(MimeType(buffered.mime)),
            buffered.bytes.to_vec(),
        );
        Ok(SystemClipboardSnapshot {
            ts_ms: self.clock.now_ms(),
            representations: vec![rep],
        })
    }

    async fn dispatch_inbound(
        &self,
        source_device_id: MobileDeviceId,
        snapshot: SystemClipboardSnapshot,
    ) -> Result<ApplyIncomingMobileClipOutcome, ApplyIncomingMobileClipError> {
        let (plaintext, content_hash) = encode_snapshot_to_v3_bytes(&snapshot)
            .map_err(|e| ApplyIncomingMobileClipError::EncodeFailed(e.to_string()))?;

        // 伪 DeviceId: `mobile_sync:<id>` 前缀让日志 / clipboard_event.from_device
        // 一眼看出"这条不是 P2P 来的", 不污染真实 P2P DeviceId 命名空间。
        let pseudo_from = DeviceId::new(format!("mobile_sync:{}", source_device_id));

        debug!(
            content_hash = %content_hash,
            plaintext_len = plaintext.len(),
            from_device = %pseudo_from,
            "mobile_sync apply_incoming: dispatching to ApplyInbound"
        );

        let outcome = self
            .inbound
            .execute(ApplyInboundInput {
                from_device: pseudo_from,
                content_hash: content_hash.clone(),
                plaintext,
            })
            .await?;

        Ok(match outcome {
            ApplyOutcome::Applied { entry_id } => {
                info!(entry_id = %entry_id, "mobile_sync apply_incoming: applied");
                ApplyIncomingMobileClipOutcome::Applied { entry_id }
            }
            ApplyOutcome::DuplicateSkipped {
                content_hash: hash,
                existing_entry_id,
            } => {
                debug!(
                    content_hash = %hash,
                    existing_entry_id = %existing_entry_id,
                    "mobile_sync apply_incoming: dedup hit, skipping"
                );
                ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                    content_hash: hash,
                    existing_entry_id,
                }
            }
            ApplyOutcome::DecodeFailed { reason } => {
                // 我们刚 encode 出来的 envelope 又被 inbound decode 失败 ——
                // 几乎不可能, 但为了类型完备保留这条路径 + warn 日志。
                warn!(
                    reason = %reason,
                    "mobile_sync apply_incoming: inbound decode failed (unexpected — we just encoded it)"
                );
                ApplyIncomingMobileClipOutcome::DecodeFailed { reason }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    //! Use-case 单测。
    //!
    //! 用 mockall 把 [`ApplyInboundClipboardUseCase`] 的 3 个内部 collaborator
    //! (entry repo / capture / write) mock 掉, 这样我们就能 assert "本 use
    //! case 在每个分支上正确驱动了 inbound 管线", 不必拉真实 sqlite + OS
    //! clipboard。
    //!
    //! 覆盖矩阵:
    //!
    //! | 分支 | 期望 outcome | inbound 调用 |
    //! |---|---|---|
    //! | Text(non-empty) | Applied | dedup miss + capture + write |
    //! | Text(empty) | DecodeFailed | 不进 inbound |
    //! | BufferFile | Buffered | 不进 inbound (buffer 增量 1) |
    //! | Image(buffer hit) | Applied | 上面同 happy path |
    //! | Image(buffer miss) | DecodeFailed | 不进 inbound |
    //! | Image(没 dataName) | DecodeFailed | 不进 inbound |
    //! | File | DecodeFailed("v1 暂不支持") | 不进 inbound |
    //! | Group | DecodeFailed | 不进 inbound |
    //! | Text dedup hit | DuplicateSkipped | dedup hit + 无 capture/write |

    use super::*;

    use anyhow::Result as AnyResult;
    use async_trait::async_trait;
    use mockall::predicate::*;

    use uc_core::ports::ClipboardEntryRepositoryPort;

    use crate::usecases::clipboard_sync::apply_inbound::{InboundCapture, InboundWrite};

    // ── mockall: same 3 collaborator surfaces apply_inbound's tests use ──

    mockall::mock! {
        EntryRepo {}
        #[async_trait]
        impl ClipboardEntryRepositoryPort for EntryRepo {
            async fn save_entry_and_selection(
                &self,
                entry: &uc_core::ClipboardEntry,
                selection: &uc_core::ClipboardSelectionDecision,
            ) -> AnyResult<()>;
            async fn get_entry(&self, entry_id: &EntryId) -> AnyResult<Option<uc_core::ClipboardEntry>>;
            async fn list_entries(&self, limit: usize, offset: usize) -> AnyResult<Vec<uc_core::ClipboardEntry>>;
            async fn touch_entry(&self, entry_id: &EntryId, active_time_ms: i64) -> AnyResult<bool>;
            async fn delete_entry(&self, entry_id: &EntryId) -> AnyResult<()>;
            async fn find_entry_id_by_snapshot_hash(&self, snapshot_hash: &str) -> AnyResult<Option<EntryId>>;
        }
    }

    mockall::mock! {
        Capture {}
        #[async_trait]
        impl InboundCapture for Capture {
            async fn capture(
                &self,
                preset_entry_id: EntryId,
                snapshot: SystemClipboardSnapshot,
            ) -> AnyResult<Option<EntryId>>;
        }
    }

    mockall::mock! {
        Write {}
        #[async_trait]
        impl InboundWrite for Write {
            async fn write(&self, snapshot: SystemClipboardSnapshot) -> AnyResult<()>;
        }
    }

    struct FixedClock;
    impl ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            1_700_000_000_000
        }
    }

    /// Build a use case where the inner ApplyInbound expects exactly:
    /// dedup-miss + 1 capture + 1 write, returning the supplied entry id.
    /// Used by the "happy path → Applied" tests.
    fn build_uc_expect_applied(entry_id: &str) -> ApplyIncomingMobileClipUseCase {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _| Ok(Some(id_for_capture.clone())));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            Arc::new(FixedClock),
        )
    }

    /// Build a use case where the inner ApplyInbound is **never** called
    /// (zero expectations on all 3 collaborators). Used by branches that
    /// short-circuit via DecodeFailed / Buffered.
    fn build_uc_expect_no_inbound() -> ApplyIncomingMobileClipUseCase {
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            Arc::new(FixedClock),
        )
    }

    /// Build a use case with a shared buffer + an inbound expecting
    /// 1 happy path. Returns (use_case, buffer) so the test can pre-seed.
    fn build_uc_with_buffer_expect_applied(
        entry_id: &str,
    ) -> (ApplyIncomingMobileClipUseCase, Arc<IncomingMobileBuffer>) {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        let id_for_capture = EntryId::from(entry_id);
        capture
            .expect_capture()
            .times(1)
            .returning(move |_, _| Ok(Some(id_for_capture.clone())));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            Arc::new(FixedClock),
        );
        (uc, buffer)
    }

    fn input_sync_doc(
        item_type: SyncClipboardItemType,
        text: &str,
        data_name: Option<&str>,
    ) -> ApplyIncomingMobileClipInput {
        ApplyIncomingMobileClipInput {
            source_device_id: MobileDeviceId::new("did_seed"),
            event: IncomingMobileClipEvent::SyncDoc {
                item_type,
                text: text.to_string(),
                data_name: data_name.map(|s| s.to_string()),
            },
        }
    }

    fn input_buffer_file(
        data_name: &str,
        mime: &str,
        bytes: Vec<u8>,
    ) -> ApplyIncomingMobileClipInput {
        ApplyIncomingMobileClipInput {
            source_device_id: MobileDeviceId::new("did_seed"),
            event: IncomingMobileClipEvent::BufferFile {
                data_name: data_name.to_string(),
                mime: mime.to_string(),
                bytes,
            },
        }
    }

    // ── verdicts ────────────────────────────────────────────────────────

    /// Text happy path: dedup miss → capture → write → `Applied`.
    #[tokio::test]
    async fn text_applied_on_new_content() {
        let uc = build_uc_expect_applied("entry-text-1");
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "hello", None))
            .await
            .expect("text happy path returns Ok");
        assert_eq!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied {
                entry_id: EntryId::from("entry-text-1")
            }
        );
    }

    /// Text with empty body → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn text_decode_failed_on_empty_body() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "", None))
            .await
            .expect("empty text returns Ok with DecodeFailed variant");
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
    }

    /// `BufferFile` event → `Buffered`, no inbound calls. Verify the
    /// buffer actually grew by 1.
    #[tokio::test]
    async fn buffer_file_returns_buffered_and_grows_buffer() {
        // build_uc_expect_no_inbound ignores buffer size — make our own
        // so we can assert on it.
        let repo = MockEntryRepo::new();
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let buffer = Arc::new(IncomingMobileBuffer::new());
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            buffer.clone(),
            Arc::new(FixedClock),
        );
        assert_eq!(buffer.len(), 0);

        let outcome = uc
            .execute(input_buffer_file(
                "photo.png",
                "image/png",
                vec![0xDE, 0xAD, 0xBE, 0xEF],
            ))
            .await
            .expect("buffer-file path returns Ok");

        assert_eq!(outcome, ApplyIncomingMobileClipOutcome::Buffered);
        assert_eq!(buffer.len(), 1, "buffer should now have the staged file");
    }

    /// Image happy path: pre-seed via BufferFile, then SyncDoc Image →
    /// `Applied`. Verifies the two-step PUT protocol's contract.
    #[tokio::test]
    async fn image_applied_after_buffer_then_sync_doc() {
        let (uc, buffer) = build_uc_with_buffer_expect_applied("entry-image-1");

        // step 1: PUT /file/{name}
        let buf_outcome = uc
            .execute(input_buffer_file(
                "photo.png",
                "image/png",
                vec![0x89, 0x50, 0x4E, 0x47],
            ))
            .await
            .unwrap();
        assert_eq!(buf_outcome, ApplyIncomingMobileClipOutcome::Buffered);
        assert_eq!(buffer.len(), 1);

        // step 2: PUT /SyncClipboard.json with type=Image, dataName=photo.png
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                Some("photo.png"),
            ))
            .await
            .unwrap();

        assert_eq!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied {
                entry_id: EntryId::from("entry-image-1")
            }
        );
        // buffer should be drained after take()
        assert_eq!(
            buffer.len(),
            0,
            "image branch should consume the buffered entry"
        );
    }

    /// Image without prior PUT /file → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn image_decode_failed_on_buffer_miss() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                Some("photo.png"),
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("file buffer miss")
        ));
    }

    /// Image without `dataName` field → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn image_decode_failed_on_missing_data_name() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Image,
                "photo.png",
                None,
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("without dataName")
        ));
    }

    /// File type → `DecodeFailed("v1 暂不支持")`, no inbound calls.
    /// This documents the P5a.3.5 follow-up boundary.
    #[tokio::test]
    async fn file_decode_failed_in_v1() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::File,
                "doc.pdf",
                Some("doc.pdf"),
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { reason } if reason.contains("v1 暂不支持")
        ));
    }

    /// Group type → `DecodeFailed`, no inbound calls.
    #[tokio::test]
    async fn group_decode_failed() {
        let uc = build_uc_expect_no_inbound();
        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Group, "", None))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DecodeFailed { .. }
        ));
    }

    /// Dedup hit: text body whose hash matches an existing local entry →
    /// `DuplicateSkipped` propagated, capture + write **not** called.
    /// Pins the "remote re-PUT doesn't double-write" property.
    #[tokio::test]
    async fn duplicate_skipped_when_text_hash_already_local() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(Some(EntryId::from("entry-existing"))));

        // Zero expectations on capture + write — mockall panics on Drop
        // if either gets called.
        let capture = MockCapture::new();
        let write = MockWrite::new();
        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            Arc::new(FixedClock),
        );

        let outcome = uc
            .execute(input_sync_doc(
                SyncClipboardItemType::Text,
                "already-here",
                None,
            ))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::DuplicateSkipped {
                existing_entry_id,
                ..
            } if existing_entry_id == EntryId::from("entry-existing")
        ));
    }

    /// `mobile_sync:<id>` 伪 DeviceId 的 plumbing: 通过 capture mock 的
    /// `withf` 校验 snapshot 的 ts_ms 来自 FixedClock。`from_device` 的
    /// 字符串校验留给 inbound 的 ports —— 我们已经在 dispatch_inbound 里
    /// `format!("mobile_sync:{}", source_device_id)`, 编译期保证。
    #[tokio::test]
    async fn text_snapshot_uses_clock_ts_ms() {
        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .withf(|_id, snapshot| snapshot.ts_ms == 1_700_000_000_000)
            .times(1)
            .returning(|_, _| Ok(Some(EntryId::from("entry-clock"))));

        let mut write = MockWrite::new();
        write.expect_write().times(1).returning(|_| Ok(()));

        let inbound =
            ApplyInboundClipboardUseCase::new(Arc::new(repo), Arc::new(capture), Arc::new(write));
        let uc = ApplyIncomingMobileClipUseCase::new(
            Arc::new(inbound),
            Arc::new(IncomingMobileBuffer::new()),
            Arc::new(FixedClock),
        );

        let outcome = uc
            .execute(input_sync_doc(SyncClipboardItemType::Text, "tick", None))
            .await
            .unwrap();
        assert!(matches!(
            outcome,
            ApplyIncomingMobileClipOutcome::Applied { .. }
        ));
    }
}
