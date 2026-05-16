//! `CaptureClipboardUseCase` — persist one clipboard snapshot as a
//! `ClipboardEntry` + `ClipboardEvent`, normalize + cache representations,
//! queue large staged reps onto the durable spool.
//!
//! ## Behaviour
//!
//! 1. Use the provided snapshot from the platform layer (事实)
//! 2. Generate `ClipboardEvent` with timestamp (时间点)
//! 3. Normalize snapshot representations (类型转换)
//! 4. Apply representation selection policy (策略决策)
//! 5. Create `ClipboardEntry` for user consumption (用户可见结果)
//!
//! ## History
//!
//! Originally lived at `uc-app/src/usecases/internal/capture_clipboard.rs`.
//! Moved here in Slice 2 Phase 3 (T0a) so `uc-application` use cases (e.g.
//! `ApplyInboundClipboardUseCase`) can depend on it without a reverse
//! `uc-application → uc-app` import (forbidden per `uc-app/AGENTS.md` §3).
//! The old path keeps a deprecated re-export shim until Slice 5 deletes
//! `uc-app`.

use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use futures::future::try_join_all;
use tracing::{debug, info, info_span, Instrument};
use uc_observability::analytics::{
    AnalyticsPort, CaptureOrigin, Event, PayloadSizeBucket, PayloadType,
};
use uc_observability::{stages, FlowId};

use uc_core::ids::{EntryId, EventId};
use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort, SpoolRequest};
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationNormalizerPort,
    DeviceIdentityPort, SelectRepresentationPolicyPort,
};
use uc_core::{
    ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
    ObservedClipboardRepresentation, PayloadAvailability, SystemClipboardSnapshot,
};

/// Capture clipboard content and create persistent entries.
///
/// Uses trait objects (`Arc<dyn Port>`) rather than generic parameters —
/// the recommended pattern for application-layer use cases, matching the
/// rest of `uc-application`.
pub struct CaptureClipboardUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    representation_cache: Arc<dyn RepresentationCachePort>,
    spool_queue: Arc<dyn SpoolQueuePort>,
    /// schema doc §12.1 · outbound 同步链路源头流量信号。
    /// 仅在 `ClipboardChangeOrigin::{LocalCapture, LocalRestore}` 路径 emit；
    /// `RemotePush` 严禁 emit（红线：与入站同步双计会污染 DAU 信号）。
    analytics: Arc<dyn AnalyticsPort>,
}

impl CaptureClipboardUseCase {
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
        representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        representation_cache: Arc<dyn RepresentationCachePort>,
        spool_queue: Arc<dyn SpoolQueuePort>,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> Self {
        Self {
            entry_repo,
            event_writer,
            representation_policy,
            representation_normalizer,
            device_identity,
            representation_cache,
            spool_queue,
            analytics,
        }
    }

    /// Execute the clipboard capture workflow with a pre-captured snapshot.
    ///
    /// Called from the daemon's clipboard change callback — the snapshot is
    /// already read by the platform layer, avoiding a redundant OS read.
    pub async fn execute(&self, snapshot: SystemClipboardSnapshot) -> Result<EntryId> {
        self.execute_with_origin(snapshot, ClipboardChangeOrigin::LocalCapture, None)
            .await?
            .ok_or_else(|| anyhow::anyhow!("local capture should always persist an entry"))
    }

    /// `preset_entry_id` 让上层在 capture 之前预先决定本次产物的 entry_id。
    /// inbound 同步路径需要这个能力:fetch + capture 完成才能把 OS 剪贴板写完,
    /// 但 UI 进度卡片必须在 fetch 之前就能挂上;预设 entry_id 让占位卡片和最终
    /// entry 共享同一个 id,前端无需做 transfer_id → entry_id 的合并。
    /// 本地 capture 路径传 `None` 即可,内部按既有逻辑生成新 id。
    pub async fn execute_with_origin(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
    ) -> Result<Option<EntryId>> {
        // Root span: all pipeline stages are children of clipboard.flow.
        // The origin field distinguishes local capture from remote push.
        //
        // 跨设备可观测性(PR2):root span 必须携带 `flow.id` + `flow.kind`,这是
        // Sentry 上把"A 端发送 → B 端接收"两条 trace join 在一起的钩子。PR2
        // 阶段 flow_id 仅在本机生成,跨设备传播由 PR3 在协议层落地(届时
        // inbound 路径会用 wire 上带过来的 flow_id 替换本地生成的)。`peer.device_id`
        // 和 `clipboard.entry_id` 在 capture 入口尚未确定,声明为
        // `tracing::field::Empty` 占位,后续 stage 用 `Span::current().record(...)`
        // 回填。
        let flow_id = FlowId::generate();
        let root = info_span!(
            "clipboard.flow",
            flow.id = %flow_id,
            flow.kind = "clipboard_capture",
            origin = ?origin,
            peer.device_id = tracing::field::Empty,
            clipboard.entry_id = tracing::field::Empty,
        );

        async move {
            if origin == ClipboardChangeOrigin::LocalRestore {
                info!(origin = ?origin, "Skipping clipboard capture");
                return Ok(None);
            }
            if !Self::has_supported_representation(&snapshot) {
                info!(
                    origin = ?origin,
                    representation_count = snapshot.representations.len(),
                    "Skipping clipboard capture because snapshot has no supported representations"
                );
                return Ok(None);
            }
            info!("Starting clipboard capture with provided snapshot");

            let event_id = EventId::new();
            let captured_at_ms = snapshot.ts_ms;
            // `RemotePush { from_device: Some(_) }` 路径走的是 apply_inbound:
            // 这次 capture 把对端推过来的 snapshot 落库,事件源就是对端,
            // 否则 delivery view 会把这条远端推送进来的 entry 误识别为
            // 本机产生,详情页显示"来自本机 + 等待同步"。
            // 守卫路径(`from_device: None`)与本地路径一样,按本机 id 记录。
            let source_device = match origin {
                ClipboardChangeOrigin::RemotePush {
                    from_device: Some(d),
                } => d,
                _ => self.device_identity.current_device_id(),
            };
            let snapshot_hash = {
                let _guard = info_span!(
                    "clipboard.snapshot_hash",
                    representation_count = snapshot.representations.len(),
                )
                .entered();
                snapshot.snapshot_hash()
            };

            // 1. 生成 event + snapshot representations
            let new_event = ClipboardEvent::new(
                event_id.clone(),
                captured_at_ms,
                source_device,
                snapshot_hash,
            );

            // 3. Normalize representations
            let normalized_reps = async {
                let normalized_futures: Vec<_> = snapshot
                    .representations
                    .iter()
                    .map(|rep| self.representation_normalizer.normalize(rep))
                    .collect();
                try_join_all(normalized_futures).await
            }
            .instrument(info_span!(stages::NORMALIZE))
            .await?;

            // Aggregated summary per capture (per-representation details at trace level)
            {
                let mut inline = 0usize;
                let mut staged_with_preview = 0usize;
                let mut staged = 0usize;
                let mut total_bytes: i64 = 0;
                let mut breakdown_parts: Vec<String> = Vec::with_capacity(normalized_reps.len());
                for rep in &normalized_reps {
                    total_bytes += rep.size_bytes;
                    breakdown_parts.push(format!("{}:{}", rep.format_id, rep.size_bytes));
                    match rep.payload_state() {
                        PayloadAvailability::Inline => inline += 1,
                        PayloadAvailability::Staged if rep.inline_data.is_some() => {
                            staged_with_preview += 1
                        }
                        PayloadAvailability::Staged => staged += 1,
                        _ => {}
                    }
                }
                let breakdown = breakdown_parts.join(", ");
                info!(
                    representations = normalized_reps.len(),
                    inline,
                    staged_with_preview,
                    staged,
                    total_bytes,
                    breakdown = %breakdown,
                    "Normalized clipboard representations"
                );
            }

            async {
                self.event_writer
                    .insert_event(&new_event, &normalized_reps)
                    .await
            }
            .instrument(info_span!(stages::PERSIST_EVENT))
            .await?;

            // Cache representations for immediate access by the background blob worker.
            // This must happen before persist_entry so the worker gets a cache hit
            // when it is notified (via try_send in spool_blobs below).
            async {
                for rep in &normalized_reps {
                    if rep.payload_state() == PayloadAvailability::Staged {
                        if let Some(observed) =
                            snapshot.representations.iter().find(|o| o.id == rep.id)
                        {
                            self.representation_cache
                                .put(&rep.id, observed.bytes.clone())
                                .await;
                        }
                    }
                }
                Ok::<(), anyhow::Error>(())
            }
            .instrument(info_span!(stages::CACHE_REPRESENTATIONS))
            .await?;

            // 4. policy.select(snapshot) — purely sync, .entered() is safe (no .await inside)
            let (entry_id, new_selection) = {
                let _guard = info_span!(stages::SELECT_POLICY).entered();
                let entry_id = preset_entry_id.unwrap_or_else(EntryId::new);
                let selection = self.representation_policy.select(&snapshot)?;
                let new_selection = ClipboardSelectionDecision::new(entry_id.clone(), selection);
                (entry_id, new_selection)
            };

            // 回填 root span 的 `clipboard.entry_id` 占位 —— 让后续所有
            // child span / event 都能在 Sentry trace 视图上 join 到同一个
            // 业务实体。`Span::current()` 在 `.instrument(root)` 的 async
            // 上下文里 == root span,record 直接生效。
            tracing::Span::current()
                .record("clipboard.entry_id", tracing::field::display(&entry_id));

            // 5. Spool large representations to disk BEFORE creating the entry.
            //
            // Durability invariant: when `entry_repo.save_entry_and_selection`
            // succeeds, the spool file for every Staged rep is already on disk
            // (`DurableSpoolQueue::enqueue` fsyncs before returning). The
            // in-memory cache is just an accelerator; spool is the source of
            // truth for representations that haven't been promoted to a blob yet.
            //
            // Previous behaviour: spool writes ran in a detached `tokio::spawn`
            // after `entry.save`, so a process exit / cache eviction between
            // the entry write and the spool write produced a permanently
            // orphaned representation (`Staged` in DB, no bytes anywhere). That
            // generated UNICLIPBOARD-RUST-5/6 — 25 + 30 events on a single
            // unrecoverable entry. The synchronous order eliminates that race
            // at the cost of capture latency on large payloads.
            //
            // On spool failure (disk full, permission denied, etc.) capture
            // returns `Err` and the entry is **not** persisted. Better to lose
            // the clipboard than to show a phantom entry that can never be
            // restored.
            let spool_reps: Vec<SpoolRequest> = normalized_reps
                .iter()
                .filter(|rep| rep.payload_state() == PayloadAvailability::Staged)
                .filter_map(|rep| {
                    snapshot
                        .representations
                        .iter()
                        .find(|o| o.id == rep.id)
                        .map(|observed| SpoolRequest {
                            rep_id: rep.id.clone(),
                            bytes: observed.bytes.clone(),
                        })
                })
                .collect();

            if !spool_reps.is_empty() {
                async {
                    for req in spool_reps {
                        let rep_id = req.rep_id.clone();
                        self.spool_queue.enqueue(req).await.map_err(|err| {
                            anyhow::anyhow!(
                                "Failed to durably spool representation {} during capture: {}",
                                rep_id,
                                err
                            )
                        })?;
                    }
                    Ok::<(), anyhow::Error>(())
                }
                .instrument(info_span!(stages::SPOOL_BLOBS))
                .await?;
            }

            // 6. entry_repo.insert_entry — bytes are durable by this point.
            async {
                let created_at_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|e| anyhow::anyhow!("Failed to get system time: {}", e))?
                    .as_millis() as i64;
                let total_size = snapshot.total_size_bytes();

                let new_entry = ClipboardEntry::new(
                    entry_id.clone(),
                    event_id.clone(),
                    created_at_ms,
                    Self::generate_title(&snapshot),
                    total_size,
                );
                self.entry_repo
                    .save_entry_and_selection(&new_entry, &new_selection)
                    .await
            }
            .instrument(info_span!(stages::PERSIST_ENTRY))
            .await?;

            info!(event_id = %event_id, entry_id = %entry_id, "Clipboard capture completed");

            // schema doc §12.1 · outbound 同步链路源头信号。
            // 红线：`RemotePush`（入站同步写本地剪贴板）严禁 emit，否则会与
            // 入站同步双计、污染 DAU。`LocalRestore` 已在入口短路 return None
            // 走不到这里；只有 `LocalCapture` 会真实落点为 `system_watcher`。
            // 未来若 manual_restore 路径开始持久化新 entry，再补 mapping。
            if let Some(capture_origin) = telemetry_capture_origin(origin) {
                self.analytics.capture(Event::ClipboardEntryCaptured {
                    origin: capture_origin,
                    payload_type: infer_payload_type(&snapshot),
                    payload_size_bucket: PayloadSizeBucket::from_bytes(
                        u64::try_from(snapshot.total_size_bytes()).unwrap_or(0),
                    ),
                });
            }

            Ok(Some(entry_id))
        }
        .instrument(root)
        .await
    }

    /// Generate a title from the clipboard snapshot for display.
    ///
    /// Tries to extract text content from text/plain representations,
    /// falling back to None if no text is found.
    fn generate_title(snapshot: &SystemClipboardSnapshot) -> Option<String> {
        const MAX_TITLE_LENGTH: usize = 200;

        for rep in &snapshot.representations {
            if let Some(mime) = &rep.mime {
                let mime_str = mime.as_str();
                if mime_str.eq_ignore_ascii_case("text/plain")
                    || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
                    || mime_str.eq_ignore_ascii_case("text/plain;charset=utf-8")
                    || mime_str.starts_with("text/")
                {
                    if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            // Use char_indices() to find a safe character boundary
                            let char_count = trimmed.chars().count();
                            if char_count > MAX_TITLE_LENGTH {
                                let truncate_at = trimmed
                                    .char_indices()
                                    .nth(MAX_TITLE_LENGTH)
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(trimmed.len());
                                let truncated = &trimmed[..truncate_at];
                                return Some(format!("{}...", truncated));
                            }
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }

        debug!("No text representation found in snapshot, title will be None");
        None
    }

    fn has_supported_representation(snapshot: &SystemClipboardSnapshot) -> bool {
        let result = snapshot
            .representations
            .iter()
            .any(Self::is_supported_representation);

        debug!(
            repr_count = snapshot.representations.len(),
            format_ids = ?snapshot
                .representations
                .iter()
                .map(|r| r.format_id.to_string())
                .collect::<Vec<_>>(),
            mimes = ?snapshot
                .representations
                .iter()
                .map(|r| r.mime.as_ref().map(|m| m.as_str().to_string()))
                .collect::<Vec<_>>(),
            result,
            "has_supported_representation evaluated",
        );

        result
    }

    fn is_supported_representation(rep: &ObservedClipboardRepresentation) -> bool {
        if let Some(mime) = &rep.mime {
            let mime_str = mime.as_str();
            if mime_str.starts_with("text/")
                || mime_str.starts_with("image/")
                || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
                || mime_str.eq_ignore_ascii_case("file/uri-list")
                || mime_str.eq_ignore_ascii_case("text/uri-list")
            {
                return true;
            }
        }

        rep.format_id.eq_ignore_ascii_case("text")
            || rep.format_id.eq_ignore_ascii_case("rtf")
            || rep.format_id.eq_ignore_ascii_case("html")
            || rep.format_id.eq_ignore_ascii_case("files")
            || rep.format_id.eq_ignore_ascii_case("image")
            || rep.format_id.eq_ignore_ascii_case("public.utf8-plain-text")
            || rep.format_id.eq_ignore_ascii_case("public.text")
            || rep.format_id.eq_ignore_ascii_case("NSStringPboardType")
    }
}

/// schema doc §12.1 红线 · 把 `ClipboardChangeOrigin` 映射到 telemetry 的
/// `CaptureOrigin`，并在入站同步路径返回 `None` 以阻断双计。
///
/// 返回 `None` = 不 emit `clipboard_entry_captured`，调用方据此跳过 capture。
fn telemetry_capture_origin(origin: ClipboardChangeOrigin) -> Option<CaptureOrigin> {
    match origin {
        ClipboardChangeOrigin::LocalCapture => Some(CaptureOrigin::SystemWatcher),
        // 已在 execute_with_origin 入口短路 return None，走不到 emit；
        // 留 mapping 以便未来 LocalRestore 也会持久化新 entry 时仍然正确。
        ClipboardChangeOrigin::LocalRestore => Some(CaptureOrigin::ManualRestore),
        // 入站同步写本地剪贴板路径——必须过滤，否则 outbound capture
        // 与入站事件双计。
        ClipboardChangeOrigin::RemotePush { .. } => None,
    }
}

/// 按 representation mime / format_id 推断 telemetry payload 大类。
///
/// 优先级 file > image > text（兜底）。schema doc §6.3 只 emit 桶化值，
/// 精确大小通过 `PayloadSizeBucket::from_bytes` 落区间。
fn infer_payload_type(snapshot: &SystemClipboardSnapshot) -> PayloadType {
    if snapshot.representations.iter().any(is_file_rep) {
        PayloadType::File
    } else if snapshot.representations.iter().any(is_image_rep) {
        PayloadType::Image
    } else {
        PayloadType::Text
    }
}

fn is_file_rep(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = &rep.mime {
        let m = mime.as_str();
        if m.eq_ignore_ascii_case("text/uri-list") || m.eq_ignore_ascii_case("file/uri-list") {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("files")
}

fn is_image_rep(rep: &ObservedClipboardRepresentation) -> bool {
    if let Some(mime) = &rep.mime {
        if mime.as_str().starts_with("image/") {
            return true;
        }
    }
    rep.format_id.eq_ignore_ascii_case("image")
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::clipboard::MimeType;
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::ObservedClipboardRepresentation;

    fn rep(format: &str, mime: Option<&str>, bytes: &[u8]) -> ObservedClipboardRepresentation {
        ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            FormatId::from(format),
            mime.map(|m| MimeType(m.to_string())),
            bytes.to_vec(),
        )
    }

    fn snapshot_with(reps: Vec<ObservedClipboardRepresentation>) -> SystemClipboardSnapshot {
        SystemClipboardSnapshot {
            ts_ms: 1_700_000_000_000,
            representations: reps,
        }
    }

    #[test]
    fn has_supported_representation_true_for_text_plain() {
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            b"hi",
        )]);
        assert!(CaptureClipboardUseCase::has_supported_representation(&snap));
    }

    #[test]
    fn has_supported_representation_true_for_image_mime() {
        let snap = snapshot_with(vec![rep("image", Some("image/png"), b"\x89PNG")]);
        assert!(CaptureClipboardUseCase::has_supported_representation(&snap));
    }

    #[test]
    fn has_supported_representation_true_for_files_format_without_mime() {
        let snap = snapshot_with(vec![rep("files", None, b"file:///tmp/x")]);
        assert!(CaptureClipboardUseCase::has_supported_representation(&snap));
    }

    #[test]
    fn has_supported_representation_true_for_uri_list_mime() {
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///tmp/a",
        )]);
        assert!(CaptureClipboardUseCase::has_supported_representation(&snap));
    }

    #[test]
    fn has_supported_representation_false_for_unknown_format_and_mime() {
        let snap = snapshot_with(vec![rep(
            "vendor.private",
            Some("application/x-vendor"),
            b"x",
        )]);
        assert!(!CaptureClipboardUseCase::has_supported_representation(
            &snap
        ));
    }

    #[test]
    fn has_supported_representation_false_for_empty_snapshot() {
        let snap = snapshot_with(vec![]);
        assert!(!CaptureClipboardUseCase::has_supported_representation(
            &snap
        ));
    }

    #[test]
    fn is_supported_representation_matches_legacy_format_aliases() {
        // Windows / older macOS format ids
        let cases: &[(&str, Option<&str>)] = &[
            ("text", None),
            ("rtf", None),
            ("html", None),
            ("image", None),
            ("public.text", None),
            ("NSStringPboardType", None),
        ];
        for (format, mime) in cases {
            let r = rep(format, *mime, b"x");
            assert!(
                CaptureClipboardUseCase::is_supported_representation(&r),
                "expected `{}` to be supported",
                format
            );
        }
    }

    #[test]
    fn generate_title_extracts_first_text_line() {
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            b"hello world",
        )]);
        assert_eq!(
            CaptureClipboardUseCase::generate_title(&snap),
            Some("hello world".to_string())
        );
    }

    #[test]
    fn generate_title_truncates_at_max_length_with_ellipsis() {
        let long = "a".repeat(250);
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            long.as_bytes(),
        )]);
        let title = CaptureClipboardUseCase::generate_title(&snap).expect("title");
        assert!(title.ends_with("..."));
        // 200 chars + "..."
        assert_eq!(title.chars().count(), 203);
    }

    #[test]
    fn generate_title_handles_multibyte_truncation_safely() {
        // 250 个 CJK 字符 (每个 3 bytes UTF-8); 截断必须落在字符边界
        let long: String = std::iter::repeat('中').take(250).collect();
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            long.as_bytes(),
        )]);
        let title = CaptureClipboardUseCase::generate_title(&snap).expect("title");
        assert!(title.ends_with("..."));
        // 不 panic 即说明 char_indices 边界查找正确
        assert_eq!(title.chars().count(), 203);
    }

    #[test]
    fn generate_title_returns_none_when_no_text_representation() {
        let snap = snapshot_with(vec![rep("image", Some("image/png"), b"\x89PNG")]);
        assert_eq!(CaptureClipboardUseCase::generate_title(&snap), None);
    }

    #[test]
    fn generate_title_skips_whitespace_only_text() {
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            b"   \t\n  ",
        )]);
        assert_eq!(CaptureClipboardUseCase::generate_title(&snap), None);
    }

    #[test]
    fn generate_title_handles_invalid_utf8_by_skipping() {
        let snap = snapshot_with(vec![rep(
            "public.utf8-plain-text",
            Some("text/plain"),
            &[0xff, 0xfe, 0xfd],
        )]);
        assert_eq!(CaptureClipboardUseCase::generate_title(&snap), None);
    }
}
