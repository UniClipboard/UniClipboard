//! `ApplyInboundClipboardUseCase` —— 入站剪贴板流程的编排主体。

use std::sync::Arc;

use tracing::{debug, error, info, instrument, warn};

use uc_core::ids::EntryId;
use uc_core::ports::ClipboardEntryRepositoryPort;
use uc_core::SystemClipboardSnapshot;

use crate::facade::blob_transfer::SharedHostEventEmitter;
use crate::facade::host_event::{
    ClipboardHostEvent, ClipboardOriginKind, HostEvent, TransferHostEvent,
};
use crate::usecases::clipboard_sync::payload_codec::decode_v3_bytes_to_snapshot_and_blob_refs;

use super::materializer::InboundBlobMaterializer;
use super::ports::{InboundCapture, InboundWrite};
use super::{ApplyInboundError, ApplyInboundInput, ApplyOutcome};

pub struct ApplyInboundClipboardUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    capture: Arc<dyn InboundCapture>,
    write: Arc<dyn InboundWrite>,
    blob_materializer: Option<Arc<dyn InboundBlobMaterializer>>,
    /// Optional host-event emitter for surfacing the inbound entry to UI
    /// before the fetch+capture pipeline finishes. Wired only in daemon
    /// mode; tests / CLI leave it `None`.
    host_event_emitter: Option<SharedHostEventEmitter>,
}

impl ApplyInboundClipboardUseCase {
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        capture: Arc<dyn InboundCapture>,
        write: Arc<dyn InboundWrite>,
    ) -> Self {
        Self {
            entry_repo,
            capture,
            write,
            blob_materializer: None,
            host_event_emitter: None,
        }
    }

    pub fn with_blob_materializer(
        mut self,
        blob_materializer: Arc<dyn InboundBlobMaterializer>,
    ) -> Self {
        self.blob_materializer = Some(blob_materializer);
        self
    }

    /// Wire a host-event emitter cell. When set, ApplyInbound emits
    /// `ClipboardHostEvent::IncomingPending` immediately after V3 decode
    /// (before blob fetch starts) and a failure status on capture errors,
    /// so the UI can render a placeholder card with a live progress bar.
    pub fn with_host_event_emitter(mut self, emitter: SharedHostEventEmitter) -> Self {
        self.host_event_emitter = Some(emitter);
        self
    }

    fn emit_host_event(&self, event: HostEvent) {
        let Some(cell) = self.host_event_emitter.as_ref() else {
            return;
        };
        let emitter = cell.read().unwrap_or_else(|p| p.into_inner()).clone();
        if let Err(err) = emitter.emit(event) {
            warn!(error = %err, "apply_inbound: failed to emit host event");
        }
    }

    #[instrument(
        name = "apply_inbound.execute",
        skip_all,
        fields(
            from_device = %input.from_device,
            content_hash = %input.content_hash,
            plaintext_len = input.plaintext.len(),
        )
    )]
    pub async fn execute(
        &self,
        input: ApplyInboundInput,
    ) -> Result<ApplyOutcome, ApplyInboundError> {
        // 1. Dedup short-circuit. The repo's default `Ok(None)` impl
        // (used by in-memory test fakes) degrades dedup to off — safe,
        // worst case we re-write the OS clipboard with identical bytes.
        let existing = self
            .entry_repo
            .find_entry_id_by_snapshot_hash(&input.content_hash)
            .await
            .map_err(|e| ApplyInboundError::DedupQuery(e.to_string()))?;
        if let Some(existing_entry_id) = existing {
            debug!(
                existing_entry_id = %existing_entry_id,
                "inbound dropped: duplicate of existing local entry"
            );
            return Ok(ApplyOutcome::DuplicateSkipped {
                content_hash: input.content_hash,
                existing_entry_id,
            });
        }

        // 2. Decode V3 envelope. Decode failure is non-fatal — drop the
        // frame, keep the loop alive (peer may be on a newer wire).
        let (snapshot, blob_refs) =
            match decode_v3_bytes_to_snapshot_and_blob_refs(input.plaintext.as_ref()) {
                Ok(decoded) => decoded,
                Err(e) => {
                    let reason = e.to_string();
                    warn!(reason, "inbound dropped: envelope decode failed");
                    return Ok(ApplyOutcome::DecodeFailed { reason });
                }
            };

        info!(
            blob_ref_count = blob_refs.len(),
            rep_count = snapshot.representations.len(),
            rep_formats = %format_rep_summary(&snapshot),
            "inbound: decoded V3 envelope"
        );

        // Pre-allocate the receiver-side entry_id so the UI placeholder, the
        // blob-fetch progress events, and the eventual `clipboard.new_content`
        // all share the same id. Without this, the placeholder card couldn't
        // be linked to the final entry by id and we'd need a transfer_id →
        // entry_id remap on the frontend.
        let receiver_entry_id = EntryId::new();
        let advertised_total_bytes: u64 = blob_refs.iter().map(|r| r.size_bytes).sum();
        // free-standing files 走 V3BlobRef.filename;rep-bound blobs (image /
        // 大二进制) 通常 filename 为 None,自动被 filter_map 跳过。
        let advertised_filenames: Vec<String> = blob_refs
            .iter()
            .filter_map(|r| r.filename.clone())
            .collect();
        self.emit_host_event(HostEvent::Clipboard(ClipboardHostEvent::IncomingPending {
            entry_id: receiver_entry_id.as_ref().to_string(),
            from_device: input.from_device.as_str().to_string(),
            total_bytes: (advertised_total_bytes > 0).then_some(advertised_total_bytes),
            filenames: advertised_filenames,
        }));

        let snapshot = match (blob_refs.is_empty(), &self.blob_materializer) {
            (true, _) => snapshot,
            (false, Some(materializer)) => {
                let count = blob_refs.len();
                let snapshot = materializer
                    .materialize(
                        input.from_device.clone(),
                        receiver_entry_id.clone(),
                        snapshot,
                        blob_refs,
                    )
                    .await
                    .map_err(|e| {
                        warn!(error = %e, blob_ref_count = count, "inbound: blob materialize failed");
                        // Tell the UI to fail the placeholder card too —
                        // otherwise it stays stuck in "transferring".
                        self.emit_host_event(HostEvent::Transfer(
                            TransferHostEvent::StatusChanged {
                                transfer_id: receiver_entry_id.as_ref().to_string(),
                                entry_id: receiver_entry_id.as_ref().to_string(),
                                status: "failed".to_string(),
                                reason: Some(e.to_string()),
                            },
                        ));
                        ApplyInboundError::Internal(format!("blob materialize: {e}"))
                    })?;
                info!(
                    blob_ref_count = count,
                    rep_count = snapshot.representations.len(),
                    rep_formats = %format_rep_summary(&snapshot),
                    "inbound: blob refs materialized into local cache"
                );
                snapshot
            }
            (false, None) => {
                let reason =
                    "payload contains blob refs but no blob materializer is wired".to_string();
                warn!(reason, "inbound dropped: blob materializer missing");
                self.emit_host_event(HostEvent::Transfer(TransferHostEvent::StatusChanged {
                    transfer_id: receiver_entry_id.as_ref().to_string(),
                    entry_id: receiver_entry_id.as_ref().to_string(),
                    status: "failed".to_string(),
                    reason: Some(reason.clone()),
                }));
                return Ok(ApplyOutcome::DecodeFailed { reason });
            }
        };

        // 3. Persist via the same capture pipeline local copies use
        // (D5: same schema). Cloning the snapshot lets us keep one for
        // the OS write below; capture takes ownership of the original.
        let snapshot_for_write = snapshot.clone();
        let entry_id = self
            .capture
            .capture(receiver_entry_id.clone(), snapshot)
            .await
            .map_err(|e| ApplyInboundError::Capture(e.to_string()))?
            .ok_or_else(|| {
                ApplyInboundError::Internal(
                    "capture returned None for RemotePush origin (unexpected)".to_string(),
                )
            })?;

        // 4. Schedule OS clipboard write in the background.
        //
        // 异步化:OS clipboard write 在大 payload 场景下能阻塞 1-3 秒(macOS
        // NSPasteboard 跨进程 IPC、Windows CF_HTML 编码),如果让 apply_inbound
        // 主流程 await,上游 mobile_sync `finalize_transfer_lifecycle` 也会被
        // 顺带推迟那么久 —— 前端会出现"entry 已经显示图片 → 2 秒后才看到
        // status_changed transferring → 紧接 completed"的反向状态过渡。
        //
        // entry 已经在第 3 步持久化(capture 已写库),OS clipboard write 是
        // best-effort —— 失败只影响"用户能否立即从系统剪贴板粘贴",不影响
        // entry 真相、不影响 transfer 状态。失败时 background task warn,
        // 不向上抛错。
        //
        // 送入 full snapshot(不 narrow):platform 层内部按能力差异消化多 rep。
        // - Windows:`write_snapshot_multi_windows` 原子写入 CF_UNICODETEXT + CF_HTML 等
        // - macOS / Linux:`write_snapshot_multi` 的降级分支用 `SelectRepresentationPolicyV1`
        //   选 paste-priority rep 后走单 rep 快路径(行为与上游 `narrow_to_primary` 等价)
        debug!(entry_id = %entry_id, "inbound: entry persisted, scheduling background OS clipboard write");

        let write_port = Arc::clone(&self.write);
        let entry_id_for_write = entry_id.clone();
        tokio::spawn(async move {
            if let Err(e) = write_port.write(snapshot_for_write).await {
                error!(
                    error = %e,
                    entry_id = %entry_id_for_write,
                    "inbound: OS clipboard background write failed after capture"
                );
            }
        });

        info!(entry_id = %entry_id, "inbound clipboard applied");

        // 关键:发出 `clipboard.new_content`,让前端 placeholder 卡片下线。
        //
        // 单点修复链路如下:
        //   1. 流程入口(line 136)我们 emit 了 `IncomingPending`,前端
        //      `useClipboardEventStream.ts:82` 据此 `addPendingEntry()` 显示
        //      "正在接收"占位卡片。
        //   2. apply_inbound 写完 OS clipboard 后,clipboard_watcher 会收到
        //      回声,但因 origin == RemotePush 在 watcher 入口短路返回(避免
        //      重复 capture),那条短路把 watcher 原本会 emit 的 new_content
        //      也吃掉了。
        //   3. 历史上从来没有任何点 emit 过 `ClipboardHostEvent::NewContent`
        //      给入站路径,导致前端 `removePendingEntry()` 永远收不到信号。
        //      用户看到"正在接收"卡死,只能 reload 才能看到真实 entry。
        //      2026-05-08 移动端图片回归把这条慢流量(数 MB JPEG)放大成可见
        //      bug —— 文本同步因为太快、列表常常被别的原因刷新而蒙混过关。
        //
        // 在此处 emit `NewContent { origin: Remote }`,前端
        // `useClipboardEventStream.ts:114-122` 收到后:
        //   * `removePendingEntry(entry_id)` 清掉占位卡片
        //   * 走 remote 分支 `onRemoteInvalidate()` 节流刷新列表 —— 真实 entry
        //     接替占位卡片,UI 状态收敛。
        //
        // 注:OS clipboard write 异步化之后,这条事件不再与 OS 写入完成绑定,
        // 而是和 entry 持久化对齐 —— 前端拿 entry 内容靠
        // `/clipboard/entries/<id>/resource`,不依赖 OS clipboard 状态。
        //
        // preview 字段:与 watcher 路径(`clipboard_watcher.rs:163`)保持一致用
        // 占位串。前端只把它打日志,不渲染;真实 preview 由列表刷新时从 daemon
        // 列表 API 拿到的 `ClipboardItemResponse` 提供。
        self.emit_host_event(HostEvent::Clipboard(ClipboardHostEvent::NewContent {
            entry_id: entry_id.as_ref().to_string(),
            preview: "New clipboard content".to_string(),
            origin: ClipboardOriginKind::Remote,
        }));

        Ok(ApplyOutcome::Applied { entry_id })
    }
}

/// Compact summary of the snapshot's representations for tracing.
/// Format: `format_id[@mime]:bytes, ...` — always safe to log because
/// `format_id` / `mime` / byte counts are metadata, never user payload.
pub(super) fn format_rep_summary(snapshot: &SystemClipboardSnapshot) -> String {
    snapshot
        .representations
        .iter()
        .map(|rep| {
            let mime_suffix = rep
                .mime
                .as_ref()
                .map(|m| format!("@{}", m.as_str()))
                .unwrap_or_default();
            format!(
                "{}{}:{}",
                rep.format_id.as_str(),
                mime_suffix,
                rep.bytes.len()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
