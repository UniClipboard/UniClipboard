//! `ApplyInboundClipboardUseCase` —— 入站剪贴板流程的编排主体。

use std::sync::Arc;

use moka::sync::Cache;
use tracing::{debug, error, info, instrument, warn, Instrument};
use uc_observability::FlowId;

use uc_core::clipboard::ActiveClipboardState;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    AdvanceActiveClipboardPort, CheckEntryAvailabilityPort, FindEntryIdBySnapshotHashPort,
};
use uc_core::{SnapshotHash, SystemClipboardSnapshot};

use crate::entry_identity::EntryIdentityCoordinator;

use crate::facade::blob_transfer::SharedHostEventEmitter;
use crate::facade::clipboard_live_index::{
    ClipboardLiveIndexInput, ClipboardLiveIndexOutcome, ClipboardLiveIndexPort,
};
use crate::facade::host_event::{
    ClipboardHostEvent, ClipboardOriginKind, HostEvent, TransferHostEvent,
};
use crate::usecases::clipboard_sync::payload_codec::decode_v3_bytes_to_snapshot_and_blob_refs;

use super::materializer::InboundBlobMaterializer;
use super::ports::{InboundCapture, InboundWrite};
use super::timing::{RAPID_DUPLICATE_WINDOW, VISIBLE_DUPLICATE_WINDOW};
use super::{ApplyInboundError, ApplyInboundInput, ApplyOutcome};

const RECENT_INBOUND_MAX_RECORDS: u64 = 128;

pub struct ApplyInboundClipboardUseCase {
    entry_repo: Arc<dyn FindEntryIdBySnapshotHashPort>,
    capture: Arc<dyn InboundCapture>,
    write: Arc<dyn InboundWrite>,
    blob_materializer: Option<Arc<dyn InboundBlobMaterializer>>,
    /// Inbound idempotency, `snapshot_hash` → `entry_id`: collapses a peer
    /// re-pushing byte-identical frames to one logical clip. TTL =
    /// `RAPID_DUPLICATE_WINDOW` (see [`super::timing`]).
    recent_snapshot_hashes: Cache<String, EntryId>,
    /// Inbound idempotency, `visible_key` → `entry_id`: collapses "same visible
    /// content, different `snapshot_hash`" (a peer re-sending with extended
    /// representations). TTL = `VISIBLE_DUPLICATE_WINDOW` (see [`super::timing`]).
    recent_visible_content: Cache<String, EntryId>,
    /// Serializes "find entry by content hash → create / replace / skip" across
    /// every writer of the same content (the two inbound channels here, and —
    /// once shared via [`Self::with_entry_identity_coordinator`] — local
    /// capture). Holding its per-identity lock across the find + materialize +
    /// commit section is what makes the dedup atomic. Defaults to a private
    /// instance (sufficient for inbound-vs-inbound); the composition root
    /// overrides it with the shared one so capture-vs-inbound is covered too.
    coordinator: Arc<EntryIdentityCoordinator>,
    /// Optional availability query. When wired, a hash match is only treated as
    /// "already held" if the matched entry is fully available; a matched but
    /// partial entry (e.g. a cancelled transfer's `uniclip-missing://`
    /// placeholder) is upgraded in place by a completing delivery instead of
    /// suppressing it. `None` degrades to "a hash match is always held" (the
    /// prior skip-on-match behavior).
    availability: Option<Arc<dyn CheckEntryAvailabilityPort>>,
    /// Optional host-event emitter for surfacing the inbound entry to UI
    /// before the fetch+capture pipeline finishes. Wired only in daemon
    /// mode; tests / CLI leave it `None`.
    host_event_emitter: Option<SharedHostEventEmitter>,
    /// Optional cross-device active-clipboard register. When wired, a freshly
    /// applied inbound entry advances the register at capture-commit (the OS
    /// write that trails it is best-effort and intentionally not gated on).
    /// `None` in tests / contexts that don't track active state.
    active_register: Option<Arc<dyn AdvanceActiveClipboardPort>>,
    /// Optional search live-indexer. When wired, a freshly applied inbound
    /// entry is indexed for full-text search (best-effort), so remote-origin
    /// clipboard is searchable just like local captures. `None` in tests /
    /// contexts without a search subsystem.
    search_live_index: Option<Arc<dyn ClipboardLiveIndexPort>>,
}

impl ApplyInboundClipboardUseCase {
    pub fn new(
        entry_repo: Arc<dyn FindEntryIdBySnapshotHashPort>,
        capture: Arc<dyn InboundCapture>,
        write: Arc<dyn InboundWrite>,
    ) -> Self {
        Self {
            entry_repo,
            capture,
            write,
            blob_materializer: None,
            coordinator: Arc::new(EntryIdentityCoordinator::new()),
            availability: None,
            host_event_emitter: None,
            active_register: None,
            search_live_index: None,
            recent_snapshot_hashes: Cache::builder()
                .max_capacity(RECENT_INBOUND_MAX_RECORDS)
                .time_to_live(RAPID_DUPLICATE_WINDOW)
                .build(),
            recent_visible_content: Cache::builder()
                .max_capacity(RECENT_INBOUND_MAX_RECORDS)
                .time_to_live(VISIBLE_DUPLICATE_WINDOW)
                .build(),
        }
    }

    /// Wire the availability query so a hash match against a partial entry
    /// triggers an in-place upgrade rather than a skip. Without it, any hash
    /// match is treated as already-held (prior behavior).
    pub fn with_check_entry_availability(
        mut self,
        availability: Arc<dyn CheckEntryAvailabilityPort>,
    ) -> Self {
        self.availability = Some(availability);
        self
    }

    /// Override the per-identity coordinator with a shared instance so local
    /// capture and inbound apply serialize on the same content lock (R5-F3).
    pub fn with_entry_identity_coordinator(
        mut self,
        coordinator: Arc<EntryIdentityCoordinator>,
    ) -> Self {
        self.coordinator = coordinator;
        self
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

    /// Wire the cross-device active-clipboard register. When set, a newly
    /// applied inbound entry advances the register so this device reflects
    /// that the peer's content is now its active clipboard state.
    pub fn with_active_register(mut self, register: Arc<dyn AdvanceActiveClipboardPort>) -> Self {
        self.active_register = Some(register);
        self
    }

    /// Wire the search live-indexer. When set, a successfully applied inbound
    /// entry is indexed for full-text search on a best-effort basis, so
    /// remote-origin clipboard shows up in search like local captures.
    pub fn with_search_live_index(mut self, index: Arc<dyn ClipboardLiveIndexPort>) -> Self {
        self.search_live_index = Some(index);
        self
    }

    /// Index a freshly applied inbound entry for search. Best-effort: the entry
    /// is already persisted, so an index failure is logged and swallowed rather
    /// than failing the inbound apply. Mirrors the OS-clipboard watcher's
    /// live-index pass, but for remote-origin (P2P + mobile) entries.
    async fn index_for_search(&self, entry_id: &EntryId, snapshot: Arc<SystemClipboardSnapshot>) {
        let Some(index) = self.search_live_index.as_ref() else {
            return;
        };
        match index
            .index_capture(ClipboardLiveIndexInput {
                entry_id: entry_id.as_ref().to_string(),
                snapshot,
            })
            .await
        {
            Ok(ClipboardLiveIndexOutcome::Indexed) => {
                debug!(entry_id = %entry_id, "inbound: indexed for search")
            }
            Ok(ClipboardLiveIndexOutcome::Skipped { reason }) => {
                debug!(entry_id = %entry_id, reason, "inbound: search live index skipped")
            }
            Err(e) => {
                warn!(error = %e, entry_id = %entry_id, "inbound: search live index failed (best-effort, ignored)")
            }
        }
    }

    /// Advance the active-clipboard register for a freshly applied inbound
    /// entry. The activation is attributed to the sending device, stamped
    /// with the snapshot's observed time — the best available proxy on the
    /// receiver for when the sender activated this content. Best-effort: a
    /// register storage failure is logged and swallowed.
    async fn advance_active_register(
        &self,
        snapshot_hash: String,
        entry_id: EntryId,
        activated_by: uc_core::ids::DeviceId,
        activated_at_ms: i64,
    ) {
        let Some(register) = self.active_register.as_ref() else {
            return;
        };
        let state =
            ActiveClipboardState::new(snapshot_hash, entry_id, activated_at_ms, activated_by);
        if let Err(e) = register.advance(&state).await {
            warn!(
                error = %e,
                snapshot_hash = %state.snapshot_hash,
                "active register: inbound advance failed (best-effort, ignored)"
            );
        }
    }

    fn emit_host_event(&self, event: HostEvent) {
        let Some(bus) = self.host_event_emitter.as_ref() else {
            return;
        };
        bus.emit_or_warn(event);
    }

    fn find_recent_duplicate(
        &self,
        snapshot_hash: &str,
        visible_key: Option<&str>,
    ) -> Option<EntryId> {
        if let Some(id) = self.recent_snapshot_hashes.get(snapshot_hash) {
            return Some(id);
        }
        self.recent_visible_content.get(visible_key?)
    }

    fn remember_recent_inbound(
        &self,
        snapshot_hash: String,
        visible_key: Option<String>,
        entry_id: EntryId,
    ) {
        self.recent_snapshot_hashes
            .insert(snapshot_hash, entry_id.clone());
        if let Some(visible_key) = visible_key {
            self.recent_visible_content.insert(visible_key, entry_id);
        }
    }

    /// Whether `entry_id` is fully held locally. With no availability port
    /// wired, a hash match is treated as held (the prior skip-on-match
    /// behavior). A transient availability-query error also degrades to "held"
    /// so a flaky query never turns a genuine duplicate into a spurious
    /// re-download / re-create.
    async fn is_entry_available(&self, entry_id: &EntryId) -> bool {
        match &self.availability {
            Some(availability) => availability
                .is_entry_available(entry_id)
                .await
                .unwrap_or(true),
            None => true,
        }
    }

    // 跨设备可观测性(PR2):
    //   - `peer.device_id` 是 PR2 起的标准字段名,把发送方 device 摆到一级
    //     span field;`from_device` 暂时保留兼容现有日志查询,Sentry tag
    //     索引完全切换后会下线。
    //   - `flow.id` 优先沿用 wire header 上带过来的对端 flow_id,实现
    //     A 端 root flow.id == B 端 root flow.id;旧版 peer 没带时才本地生成。
    //   - `flow.kind` 静态 `clipboard_sync`,方便按业务流过滤。
    #[instrument(
        name = "apply_inbound.execute",
        skip_all,
        fields(
            from_device = %input.from_device,
            peer.device_id = %input.from_device,
            snapshot_hash = %input.snapshot_hash,
            plaintext_len = input.plaintext.len(),
            flow.id = tracing::field::Empty,
            flow.kind = "clipboard_sync",
        )
    )]
    pub async fn execute(
        &self,
        input: ApplyInboundInput,
    ) -> Result<ApplyOutcome, ApplyInboundError> {
        let flow_id = input.flow_id.clone().unwrap_or_else(FlowId::generate);
        tracing::Span::current().record("flow.id", tracing::field::display(&flow_id));
        // 1. Decode V3 envelope. Decode failure is non-fatal — drop the
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

        // 2. Hold the per-identity lock across the whole "find by hash →
        // materialize → create / replace / skip" section so it is atomic
        // against every other writer of the same content (no double-create, no
        // create-vs-replace interleave). Layer ③ in-flight suppression is
        // deferred, so the download runs inside the lock: same-identity
        // deliveries serialize (a late duplicate then finds the committed entry
        // and skips its own download — a free bandwidth saving), while different
        // identities proceed in parallel via the coordinator's lock striping.
        let _identity_guard = self.coordinator.lock(&input.snapshot_hash).await;

        // 3. Pre-download dedup. A hash match that is *fully held* is skipped
        // before we show a progress card or download anything. A match that is
        // *partial* (e.g. a cancelled transfer's `uniclip-missing://`
        // placeholder) is NOT held — fall through to materialize and upgrade it
        // in place. The repo's default `Ok(None)` impl (in-memory test fakes)
        // degrades dedup to off; `is_entry_available` defaults to "held" when no
        // availability port is wired (prior skip-on-match behavior).
        let existing = self
            .entry_repo
            .find_entry_id_by_snapshot_hash(&input.snapshot_hash)
            .await
            .map_err(|e| ApplyInboundError::DedupQuery(e.to_string()))?;
        if let Some(existing_id) = existing.as_ref() {
            if self.is_entry_available(existing_id).await {
                debug!(
                    existing_entry_id = %existing_id,
                    "inbound dropped: duplicate of existing, fully-held local entry"
                );
                return Ok(ApplyOutcome::DuplicateSkipped {
                    snapshot_hash: input.snapshot_hash,
                    existing_entry_id: existing_id.clone(),
                });
            }
            debug!(
                existing_entry_id = %existing_id,
                "inbound: hash matches a partial local entry; will materialize and upgrade in place"
            );
        }

        // Pre-allocate the receiver-side entry_id so the UI placeholder, the
        // blob-fetch progress events, and the eventual `clipboard.new_content`
        // all share the same id. Without this, the placeholder card couldn't
        // be linked to the final entry by id and we'd need a transfer_id →
        // entry_id remap on the frontend.
        //
        // For the in-place upgrade path (hash matched a *partial* entry), reuse
        // that entry's id: the completed content is persisted under `existing`
        // below, so the IncomingPending card and the final entry must share it —
        // a fresh id would strand the pending card on a different entry.
        let receiver_entry_id = existing.clone().unwrap_or_else(EntryId::new);
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

        let (snapshot, is_partial) = match (blob_refs.is_empty(), &self.blob_materializer) {
            (true, _) => (snapshot, false),
            (false, Some(materializer)) => {
                let count = blob_refs.len();
                let result = materializer
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
                let partial = result.is_partial();
                info!(
                    blob_ref_count = count,
                    rep_count = result.snapshot.representations.len(),
                    rep_formats = %format_rep_summary(&result.snapshot),
                    missing_count = result.missing.len(),
                    partial,
                    "inbound: blob refs materialized into local cache"
                );
                (result.snapshot, partial)
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

        // 6. Rapid in-memory dedup of a recently-completed re-push. Only
        // complete entries are remembered, so this never suppresses the
        // completing delivery of a partial. Consulted only when the DB had no
        // match — a partial DB match takes the upgrade path below.
        let visible_key = snapshot.meaningful_origin_key();
        if existing.is_none() {
            if let Some(existing_entry_id) =
                self.find_recent_duplicate(&input.snapshot_hash, visible_key.as_deref())
            {
                debug!(
                    existing_entry_id = %existing_entry_id,
                    "inbound dropped: rapid duplicate of recently applied entry"
                );
                return Ok(ApplyOutcome::DuplicateSkipped {
                    snapshot_hash: input.snapshot_hash,
                    existing_entry_id,
                });
            }
        }

        // 7. Persist via the same capture pipeline local copies use (D5: same
        // schema): create a new entry, or upgrade the matched partial in place.
        // Keep one snapshot clone behind an `Arc` for the downstream consumers
        // (search live-index, the background OS write) before capture takes the
        // original. Persist under the sender's wire identity, never a hash
        // recomputed from the materialized snapshot (F-4): a cancelled
        // transfer's `uniclip-missing://` placeholder would recompute to a
        // divergent hash and fork the entry. `parse` is non-panicking — an
        // unparseable wire hash degrades to `None` (recompute), never a DoS.
        let snapshot_for_write = Arc::new(snapshot.clone());
        let authoritative_hash = SnapshotHash::parse(&input.snapshot_hash);
        let entry_id = match existing {
            // Any surviving match is partial — fully-held matches returned at
            // step 3.
            Some(existing_id) => {
                if is_partial {
                    // Don't replace a partial with another partial: keep the
                    // existing placeholder so the eventual completed delivery
                    // upgrades it (avoids thrashing between two partials).
                    debug!(
                        existing_entry_id = %existing_id,
                        "inbound: delivery also partial; keeping existing placeholder"
                    );
                    return Ok(ApplyOutcome::DuplicateSkipped {
                        snapshot_hash: input.snapshot_hash,
                        existing_entry_id: existing_id,
                    });
                }
                self.capture
                    .replace_with_identity(
                        existing_id,
                        input.from_device,
                        snapshot,
                        authoritative_hash,
                    )
                    .await
                    .map_err(|e| ApplyInboundError::Capture(e.to_string()))?
                    .ok_or_else(|| {
                        ApplyInboundError::Internal(
                            "replace returned None for RemotePush origin (unexpected)".to_string(),
                        )
                    })?
            }
            None => self
                .capture
                .capture_with_identity(
                    receiver_entry_id.clone(),
                    input.from_device,
                    snapshot,
                    authoritative_hash,
                )
                .await
                .map_err(|e| ApplyInboundError::Capture(e.to_string()))?
                .ok_or_else(|| {
                    ApplyInboundError::Internal(
                        "capture returned None for RemotePush origin (unexpected)".to_string(),
                    )
                })?,
        };

        // The find → commit section is complete; release the per-identity lock
        // before the best-effort side work (register advance, search index, OS
        // write) so a concurrent delivery of a *different* identity is never
        // blocked behind it.
        drop(_identity_guard);

        // 8. Schedule OS clipboard write in the background.
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
        //
        // Partial entry(materialize 被用户 cancel)**不能**写 OS clipboard:
        // 半残 snapshot 会把 `uniclip-missing://` 占位 URI 推到系统剪贴板,
        // 用户 cmd-V 出来的是"垃圾"。entry 已落库可以从应用内复用,但 OS
        // pasteboard 必须保留用户之前的内容不被污染。
        //
        // dedup 窗口(`remember_recent_inbound`)同样不能登记 partial entry:
        // 否则用户在取消后立即重新触发同一文件传输,`find_recent_duplicate`
        // 会把第二次也判为 dup 直接 skip,用户陷入"取消后无法恢复"困境。
        // partial 不进 dedup,完整成功才记。
        if !is_partial {
            self.remember_recent_inbound(
                input.snapshot_hash.clone(),
                visible_key,
                entry_id.clone(),
            );
            // Advance the active-clipboard register at capture-commit (D1
            // call-site: inbound apply). The OS write below is detached and
            // best-effort, so the register is intentionally decoupled from it
            // for the bulk content-sync path.
            self.advance_active_register(
                input.snapshot_hash.clone(),
                entry_id.clone(),
                input.from_device,
                snapshot_for_write.ts_ms,
            )
            .await;

            // Best-effort: index the applied entry so remote-origin clipboard
            // (P2P + mobile) is searchable like local captures. The entry is
            // already persisted, so indexing never gates the inbound apply.
            self.index_for_search(&entry_id, Arc::clone(&snapshot_for_write))
                .await;

            debug!(entry_id = %entry_id, "inbound: entry persisted, scheduling background OS clipboard write");
            let write_port = Arc::clone(&self.write);
            let entry_id_for_write = entry_id.clone();
            let from_device_for_write = input.from_device.clone();
            let snapshot_hash_for_write = input.snapshot_hash.clone();
            let origin_guard_key_for_write = snapshot_for_write.origin_guard_key();
            // `.in_current_span()` keeps the spawned task under `apply_inbound.execute`
            // so trace_id / from_device / snapshot_hash propagate into the failure event.
            // Without this the background failure was a context-less orphan in Sentry —
            // the missing peer_id field is exactly what made the recent UNICLIPBOARD-RUST-F
            // triage take an extra hour (couldn't tell whether 50 failures were one peer
            // hammering or many peers each pushing once).
            tokio::spawn(
                async move {
                    // The live-index pass above already awaited and dropped its
                    // `Arc` clone, so this reclaims sole ownership without
                    // copying. The fallback clone is unreachable in practice
                    // (refcount is 1 here) and only guards a future second holder.
                    let snapshot_for_write = Arc::try_unwrap(snapshot_for_write)
                        .unwrap_or_else(|shared| (*shared).clone());
                    if let Err(e) = write_port.write(snapshot_for_write).await {
                        error!(
                            event = "inbound_os_write_failed",
                            error_kind = "inbound_os_write_failed",
                            error = %e,
                            entry_id = %entry_id_for_write,
                            from_device = %from_device_for_write,
                            snapshot_hash = %snapshot_hash_for_write,
                            origin_guard_key = %origin_guard_key_for_write,
                            "inbound: OS clipboard background write failed after capture"
                        );
                    }
                }
                .in_current_span(),
            );
        } else {
            info!(
                entry_id = %entry_id,
                "inbound: partial entry persisted, skipping OS clipboard write to avoid \
                 leaking uniclip-missing:// placeholders into the system pasteboard"
            );
            // 抑制 unused warning(partial 分支不消费 snapshot_for_write)。
            drop(snapshot_for_write);
        }

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
                rep.size_bytes()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}
