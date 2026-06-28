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
use tracing::{debug, info, info_span, warn, Instrument};
use uc_observability::analytics::{
    AnalyticsPort, CaptureOrigin, Event, PayloadSizeBucket, PayloadType,
};
use uc_observability::{stages, FlowId};

use uc_core::blob::ports::BlobContentIngestPort;
use uc_core::clipboard::{ClipboardPayloadSource, PersistedClipboardRepresentation};

use crate::facade::clipboard_outbound::extract_file_paths_from_snapshot;
use uc_core::ids::{EntryId, EventId};
use uc_core::ports::clipboard::{
    FindEntryIdBySnapshotHashPort, ReplaceEntryContentPort, RepresentationCachePort,
    SaveClipboardEntryPort, SpoolQueuePort, SpoolRequest, TouchClipboardEntryPort,
};
use uc_core::ports::{
    ClipboardEventWriterPort, ClipboardRepresentationNormalizerPort, DeviceIdentityPort,
    SelectRepresentationPolicyPort,
};
use uc_core::{
    ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
    ObservedClipboardRepresentation, PayloadAvailability, SnapshotHash, SystemClipboardSnapshot,
};

/// Result of a capture attempt.
///
/// `deduplicated == true` means the snapshot matched an existing entry's
/// content hash and that entry was resurfaced (its active time was bumped to
/// the top of history) instead of persisting a duplicate row. Callers should
/// refresh the UI for the entry but must NOT re-index or re-dispatch it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureOutcome {
    pub entry_id: EntryId,
    pub deduplicated: bool,
    /// The `snapshot_hash` persisted on this entry — its cross-device identity.
    ///
    /// Consumers that advertise this capture to peers (e.g. the
    /// active-clipboard register) MUST reuse this value rather than recomputing
    /// a hash from a separate, pre-digest copy of the snapshot. Recomputing on a
    /// copy that never had `file_content_digests` populated yields the
    /// device-local `text/uri-list` path hash, which diverges from the dispatch
    /// path's content-based hash and makes the receiver dedup into two entries.
    pub snapshot_hash: String,
}

/// How a captured snapshot is committed to storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitMode {
    /// Persist as a brand-new entry under the resolved `entry_id`.
    Create,
    /// Replace the content of the existing entry identified by the resolved
    /// `entry_id` in place — reusing its identity and sticky state. Used by the
    /// inbound upgrade path when a completed delivery supersedes a partial entry
    /// that already carries the same content hash.
    Replace,
}

/// Capture clipboard content and create persistent entries.
///
/// Uses trait objects (`Arc<dyn Port>`) rather than generic parameters —
/// the recommended pattern for application-layer use cases, matching the
/// rest of `uc-application`.
pub struct CaptureClipboardUseCase {
    save_entry: Arc<dyn SaveClipboardEntryPort>,
    touch_entry: Arc<dyn TouchClipboardEntryPort>,
    find_entry_by_snapshot_hash: Arc<dyn FindEntryIdBySnapshotHashPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    representation_cache: Arc<dyn RepresentationCachePort>,
    spool_queue: Arc<dyn SpoolQueuePort>,
    /// Materialize path-backed files into the blob store and recover their
    /// content hash in one streaming pass. Used for two file-rep shapes:
    /// - `ClipboardPayloadSource::LocalFile` reps → produce a `BlobReady`
    ///   `PersistedClipboardRepresentation` (bypassing normalizer/cache/spool).
    /// - file paths parsed out of an Inline `text/uri-list` rep (e.g. Windows
    ///   file copy) → fill `file_content_digests` so the entry's snapshot
    ///   identity is derived from device-independent file content rather than
    ///   the device-local `text/uri-list` path text.
    blob_ingest: Arc<dyn BlobContentIngestPort>,
    /// Transactional entry-replace used by [`CommitMode::Replace`]. Swaps the
    /// content behind an existing entry_id in place (FK-safe cascade, sticky
    /// state preserved) instead of inserting a new entry. Only the inbound
    /// upgrade path drives the `Replace` mode; local capture always `Create`s.
    replace_entry: Arc<dyn ReplaceEntryContentPort>,
    /// Shared per-identity write coordinator. When wired, a *local* capture
    /// serializes its "resurface-or-create by content hash" section on the lock
    /// for that hash so it cannot race an inbound apply of the same content into
    /// two entries (R5-F3). Inbound captures do NOT lock here — the inbound use
    /// case already holds the same per-identity lock around the call, so locking
    /// again would deadlock on the non-reentrant mutex. `None` skips locking
    /// (prior behavior; harmless when no concurrent same-content writer exists).
    coordinator: Option<Arc<crate::entry_identity::EntryIdentityCoordinator>>,
    /// schema doc §12.1 · outbound 同步链路源头流量信号。
    /// 仅在 `ClipboardChangeOrigin::{LocalCapture, LocalRestore}` 路径 emit；
    /// `RemotePush` 严禁 emit（红线：与入站同步双计会污染 DAU 信号）。
    analytics: Arc<dyn AnalyticsPort>,
}

impl CaptureClipboardUseCase {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        save_entry: Arc<dyn SaveClipboardEntryPort>,
        touch_entry: Arc<dyn TouchClipboardEntryPort>,
        find_entry_by_snapshot_hash: Arc<dyn FindEntryIdBySnapshotHashPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
        representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        representation_cache: Arc<dyn RepresentationCachePort>,
        spool_queue: Arc<dyn SpoolQueuePort>,
        blob_ingest: Arc<dyn BlobContentIngestPort>,
        replace_entry: Arc<dyn ReplaceEntryContentPort>,
        analytics: Arc<dyn AnalyticsPort>,
    ) -> Self {
        Self {
            save_entry,
            touch_entry,
            find_entry_by_snapshot_hash,
            event_writer,
            representation_policy,
            representation_normalizer,
            device_identity,
            representation_cache,
            spool_queue,
            blob_ingest,
            replace_entry,
            coordinator: None,
            analytics,
        }
    }

    /// Share the per-identity write coordinator so a local capture serializes
    /// its hash-keyed resurface-or-create against inbound apply of the same
    /// content (R5-F3). Without it, local capture does not lock (prior
    /// behavior).
    pub fn with_entry_identity_coordinator(
        mut self,
        coordinator: Arc<crate::entry_identity::EntryIdentityCoordinator>,
    ) -> Self {
        self.coordinator = Some(coordinator);
        self
    }

    /// Execute the clipboard capture workflow with a pre-captured snapshot.
    ///
    /// Called from the daemon's clipboard change callback — the snapshot is
    /// already read by the platform layer, avoiding a redundant OS read.
    pub async fn execute(&self, snapshot: SystemClipboardSnapshot) -> Result<EntryId> {
        self.execute_with_origin(
            snapshot,
            ClipboardChangeOrigin::LocalCapture,
            None,
            None,
            CommitMode::Create,
        )
        .await?
        .map(|outcome| outcome.entry_id)
        .ok_or_else(|| anyhow::anyhow!("local capture should always persist an entry"))
    }

    /// `preset_entry_id` 让上层在 capture 之前预先决定本次产物的 entry_id。
    /// inbound 同步路径需要这个能力:fetch + capture 完成才能把 OS 剪贴板写完,
    /// 但 UI 进度卡片必须在 fetch 之前就能挂上;预设 entry_id 让占位卡片和最终
    /// entry 共享同一个 id,前端无需做 transfer_id → entry_id 的合并。
    /// 本地 capture 路径传 `None` 即可,内部按既有逻辑生成新 id。
    /// `authoritative_hash` overrides the persisted cross-device identity.
    /// Local captures pass `None` and let the snapshot hash itself; inbound
    /// (`RemotePush`) passes `Some(wire_hash)` so the entry is stored under the
    /// exact identity the sender advertised. The latter MUST NOT be recomputed
    /// from the materialized snapshot — for a cancelled transfer the file rep is
    /// a `uniclip-missing://` placeholder (no `file_content_digests`) and for a
    /// completed one it carries receiver-rewritten local paths; both hash
    /// differently from the wire identity and would fork the entry, breaking
    /// dedup against every other channel that carries the same wire hash.
    pub async fn execute_with_origin(
        &self,
        mut snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
        preset_entry_id: Option<EntryId>,
        authoritative_hash: Option<SnapshotHash>,
        commit_mode: CommitMode,
    ) -> Result<Option<CaptureOutcome>> {
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
            // Populate file_content_digests so snapshot_hash() is based on
            // device-independent file *content* rather than the text/uri-list
            // path text (device-specific). Skipped when already populated
            // (RemotePush: the inbound materializer fills these from the wire
            // before this capture runs).
            if snapshot.file_content_digests.is_empty() {
                let digests =
                    derive_file_content_digests(&snapshot, self.blob_ingest.as_ref()).await;
                if !digests.is_empty() {
                    snapshot.file_content_digests = digests;
                }
            }
            let snapshot_hash = match authoritative_hash {
                // Inbound: persist the sender's wire identity verbatim (F-4).
                Some(wire_hash) => wire_hash,
                // Local capture: the snapshot is authoritative for its own hash.
                None => {
                    let _guard = info_span!(
                        "clipboard.snapshot_hash",
                        representation_count = snapshot.representations.len(),
                    )
                    .entered();
                    snapshot.snapshot_hash()
                }
            };
            // Keep the canonical hash string before `snapshot_hash` is moved
            // into the event below, so the outcome can carry the exact identity
            // this entry is persisted under (see `CaptureOutcome::snapshot_hash`).
            let snapshot_hash_str = snapshot_hash.to_string();

            // Serialize the resurface-or-create section against any other writer
            // of this same content (R5-F3). Only a *local* capture locks here:
            // an inbound (`RemotePush`) capture is already inside the inbound use
            // case's per-identity lock for this hash, so locking the same
            // (non-reentrant) mutex again would deadlock. The guard is held
            // across persist and dropped when this async block returns.
            let _identity_guard = match (&self.coordinator, origin) {
                (Some(coordinator), ClipboardChangeOrigin::LocalCapture) => {
                    Some(coordinator.lock(&snapshot_hash_str).await)
                }
                _ => None,
            };

            // Local-capture dedup: if this exact content already exists,
            // resurface the existing entry (bump it to the top of history)
            // instead of persisting a duplicate row and re-dispatching it.
            // Gated to `LocalCapture` — `RemotePush` runs its own dedup
            // upstream, and `LocalRestore` already short-circuits above.
            //
            // Non-fatal: a lookup failure must not drop the capture, so on
            // error we degrade to the prior no-dedup behavior (create a new
            // entry) rather than propagating.
            if origin == ClipboardChangeOrigin::LocalCapture {
                if let Some(existing) = resurface_existing_entry(
                    self.find_entry_by_snapshot_hash.as_ref(),
                    self.touch_entry.as_ref(),
                    &snapshot_hash_str,
                    captured_at_ms,
                )
                .await
                {
                    info!(
                        entry_id = %existing,
                        "Local capture matched existing content; resurfaced instead of duplicating"
                    );
                    return Ok(Some(CaptureOutcome {
                        entry_id: existing,
                        deduplicated: true,
                        snapshot_hash: snapshot_hash_str,
                    }));
                }
            }

            // 1. 生成 event + snapshot representations
            let new_event = ClipboardEvent::new(
                event_id.clone(),
                captured_at_ms,
                source_device,
                snapshot_hash,
            );

            // 3. Normalize representations.
            //
            // 分流:Inline source 走 normalizer 既有逻辑(inline / staged / staged_with_preview
            // 决策);LocalFile source 调 BlobContentIngestPort.ingest_path 同步物化到 blob 仓库,
            // 直接产出 BlobReady 状态的 PersistedRep —— 绕过 representation_cache / spool_queue,
            // 因为它不需要"暂存字节等待异步物化"。
            //
            // LocalFile 在 capture 同步路径里物化(hardlink 时是 O(1),跨卷流式 copy 时是
            // O(file_size) IO),让 dashboard 第一秒就能从 /clipboard/blobs/{blob_id} 取到真图。
            let normalized_reps = async {
                let mut out: Vec<PersistedClipboardRepresentation> =
                    Vec::with_capacity(snapshot.representations.len());
                for observed in &snapshot.representations {
                    match observed.source() {
                        ClipboardPayloadSource::LocalFile { path, size_bytes } => {
                            let blob_id = self
                                .blob_ingest
                                .ingest_path(path)
                                .await
                                .map(|ingested| ingested.blob_id)
                                .map_err(|err| {
                                    // No path in the message: a clipboard file
                                    // path is user content.
                                    anyhow::anyhow!(
                                        "LocalFile rep ingest into blob store failed: {err}"
                                    )
                                })?;
                            info!(
                                rep_id = %observed.id,
                                blob_id = %blob_id,
                                file_size = size_bytes,
                                "Ingested LocalFile rep into blob store as BlobReady"
                            );
                            out.push(PersistedClipboardRepresentation::new(
                                observed.id.clone(),
                                observed.format_id.clone(),
                                observed.mime.clone(),
                                *size_bytes as i64,
                                None,          // inline_data
                                Some(blob_id), // blob_id ⇒ payload_state=BlobReady
                            ));
                        }
                        ClipboardPayloadSource::Inline(_) => {
                            let persisted =
                                self.representation_normalizer.normalize(observed).await?;
                            out.push(persisted);
                        }
                    }
                }
                Ok::<Vec<PersistedClipboardRepresentation>, anyhow::Error>(out)
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

            // Create commits the event as a standalone insert here; Replace
            // defers the event insert into the transactional entry-replace below
            // so the old event/reps and the new ones swap atomically.
            if commit_mode == CommitMode::Create {
                async {
                    self.event_writer
                        .insert_event(&new_event, &normalized_reps)
                        .await
                }
                .instrument(info_span!(stages::PERSIST_EVENT))
                .await?;
            }

            // Cache representations for immediate access by the background blob worker.
            // This must happen before persist_entry so the worker gets a cache hit
            // when it is notified (via try_send in spool_blobs below).
            async {
                for rep in &normalized_reps {
                    if rep.payload_state() == PayloadAvailability::Staged {
                        if let Some(observed) =
                            snapshot.representations.iter().find(|o| o.id == rep.id)
                        {
                            // Staged path 当前仍要求 Inline source —— LocalFile rep 在
                            // 上游 BlobWriter ingest 阶段会被产出 BlobReady 状态,不会
                            // 走到 Staged 分支。
                            if let Some(bytes) = observed.inline_bytes() {
                                self.representation_cache.put(&rep.id, bytes.to_vec()).await;
                            }
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
                    let observed = snapshot.representations.iter().find(|o| o.id == rep.id)?;
                    // Staged spool 仅承载 Inline 字节;LocalFile rep 不进 Staged。
                    let bytes = observed.inline_bytes()?;
                    Some(SpoolRequest {
                        rep_id: rep.id.clone(),
                        bytes: bytes.to_vec(),
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

            // 6. Persist the entry — bytes are durable by this point. Create
            //    inserts a fresh entry; Replace swaps the content behind the
            //    existing entry_id in one transaction (event/reps/selection +
            //    cascade), reusing its identity and sticky state.
            async {
                let title = Self::generate_title(&snapshot);
                let total_size = snapshot.total_size_bytes();
                match commit_mode {
                    CommitMode::Create => {
                        let created_at_ms = SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .map_err(|e| anyhow::anyhow!("Failed to get system time: {}", e))?
                            .as_millis() as i64;
                        let new_entry = ClipboardEntry::new(
                            entry_id.clone(),
                            event_id.clone(),
                            created_at_ms,
                            title,
                            total_size,
                        );
                        self.save_entry
                            .save_entry_and_selection(&new_entry, &new_selection)
                            .await
                            .map_err(anyhow::Error::from)
                    }
                    CommitMode::Replace => self
                        .replace_entry
                        .replace_entry_content(
                            &entry_id,
                            &new_event,
                            &normalized_reps,
                            &new_selection,
                            title,
                            total_size,
                        )
                        .await
                        .map_err(anyhow::Error::from),
                }
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

            Ok(Some(CaptureOutcome {
                entry_id,
                deduplicated: false,
                snapshot_hash: snapshot_hash_str,
            }))
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
                if mime_str.starts_with("text/") {
                    let Some(rep_bytes) = rep.inline_bytes() else {
                        continue;
                    };
                    if let Ok(text) = std::str::from_utf8(rep_bytes) {
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
                || mime_str.eq_ignore_ascii_case("file/uri-list")
                || mime_str.eq_ignore_ascii_case("text/uri-list")
            {
                return true;
            }
        }

        // format_id may still carry platform-native identifiers (UTIs,
        // NSPasteboard legacy names) — that is the field's documented
        // role. Only the `mime` field is normalized to RFC at the
        // engine boundary.
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

/// Resolve a local-capture dedup hit into the entry that should be
/// resurfaced, or `None` when the capture must be persisted as a new entry.
///
/// Returns `Some(entry_id)` only when an entry carrying this `snapshot_hash`
/// exists AND its active time was successfully bumped (`touch_entry` updated a
/// row). Three cases yield `None` so the caller degrades to creating a fresh
/// entry instead of returning a stale id:
///   - no entry matches the hash (`Ok(None)`),
///   - the lookup itself failed (`Err`), and
///   - `touch_entry` updated no rows (`Ok(false)`) — the entry was deleted
///     between the lookup and the touch (e.g. a concurrent cleanup), so the
///     id would dangle if returned as `deduplicated: true`.
///
/// All failure paths are non-fatal: a dedup miss must never drop the capture.
async fn resurface_existing_entry(
    find_entry_by_snapshot_hash: &dyn FindEntryIdBySnapshotHashPort,
    touch_entry: &dyn TouchClipboardEntryPort,
    snapshot_hash: &str,
    captured_at_ms: i64,
) -> Option<EntryId> {
    let existing = match find_entry_by_snapshot_hash
        .find_entry_id_by_snapshot_hash(snapshot_hash)
        .await
    {
        Ok(Some(existing)) => existing,
        Ok(None) => return None,
        Err(e) => {
            warn!(error = %e, "Local-capture dedup lookup failed; proceeding to create entry");
            return None;
        }
    };

    match touch_entry.touch_entry(&existing, captured_at_ms).await {
        Ok(true) => Some(existing),
        Ok(false) => {
            debug!(
                entry_id = %existing,
                "Dedup target vanished before resurface (0 rows touched); creating new entry"
            );
            None
        }
        Err(e) => {
            warn!(
                entry_id = %existing,
                error = %e,
                "Failed to resurface existing entry; creating new entry"
            );
            None
        }
    }
}

/// Derive `file_content_digests` for a freshly captured snapshot so that
/// [`SystemClipboardSnapshot::snapshot_hash`] keys on device-independent file
/// *content* rather than the device-local `text/uri-list` path text. Returns
/// the digest list (empty when the snapshot carries no resolvable files).
///
/// Two file-rep shapes contribute, both resolved to file paths and hashed via
/// the fallible [`BlobContentIngestPort::hash_path`] (never a rep's
/// `content_hash()`, which `panic!`s on a stream-hash error):
/// - `ClipboardPayloadSource::LocalFile` reps (e.g. macOS Finder copy): the path
///   is carried directly on the rep.
/// - Inline `text/uri-list` file reps (e.g. Windows file copy): the files live
///   as path text, not `LocalFile` reps. Without hashing them, capture would
///   store the uri-list path-text hash while dispatch sends the content hash,
///   and the receiver would create two entries for one file.
///
/// `extract_file_paths_from_snapshot` is shared with the dispatch path so both
/// resolve the same set of files (including macOS APFS file-reference
/// resolution). On *any* per-file hash failure the whole digest list is
/// discarded (not just the failed file) so identity never keys on a partial
/// subset; it then falls back to the uri-list path text.
///
/// The two shapes are mutually exclusive in practice (a snapshot carries either
/// `LocalFile` reps or an inline uri-list rep), so the inline branch only runs
/// when no `LocalFile` digests were found.
async fn derive_file_content_digests(
    snapshot: &SystemClipboardSnapshot,
    blob_ingest: &dyn BlobContentIngestPort,
) -> Vec<[u8; 32]> {
    // Identity contributors as file paths. `LocalFile` reps (e.g. macOS Finder
    // copy) carry the path directly; an inline `text/uri-list` rep (e.g. Windows
    // file copy) carries them as path text. The two shapes are mutually
    // exclusive in practice, so the inline extraction only runs when no
    // `LocalFile` rep is present.
    let local_file_paths: Vec<_> = snapshot
        .representations
        .iter()
        .filter_map(|r| match r.source() {
            ClipboardPayloadSource::LocalFile { path, .. } => Some(path.clone()),
            ClipboardPayloadSource::Inline(_) => None,
        })
        .collect();
    let local_file_digests = local_file_paths.len();
    let mut extracted_paths = 0usize;
    let paths = if local_file_paths.is_empty() {
        let extracted = extract_file_paths_from_snapshot(snapshot);
        extracted_paths = extracted.len();
        extracted
    } else {
        local_file_paths
    };

    // Identity only: hash each file's content (device-independent) without
    // materializing/encrypting a blob into local storage. The actual file bytes
    // for transfer are published lazily by the (spawned, non-blocking) dispatch
    // path, so a synchronous ingest here would both stall the capture loop on
    // large files and orphan a blob this entry never references. `hash_path` is
    // fallible, so an unreadable or deleted copied file degrades to a fallback
    // identity instead of panicking — a `LocalFile` rep's `content_hash()` would
    // `panic!` on a stream-hash error.
    let mut digests: Vec<[u8; 32]> = Vec::with_capacity(paths.len());
    let mut had_hash_failure = false;
    for path in &paths {
        match blob_ingest.hash_path(path).await {
            Ok(content_hash) => digests.push(content_hash.bytes),
            Err(err) => {
                had_hash_failure = true;
                // No path in the field: a clipboard file path is user content.
                warn!(error = %err, "capture: could not derive complete file-set identity");
            }
        }
    }
    // A partial file-set identity (some files hashed, some failed) would key the
    // snapshot on a strict subset, colliding with a different copy that contains
    // only those files. Discard everything on any failure so identity
    // deterministically falls back to the `text/uri-list` path text.
    if had_hash_failure {
        digests.clear();
    }

    // Diagnostic for the dual-entry file-sync bug: when this returns empty for a
    // file copy, the entry's `snapshot_hash` falls back to the device-local
    // `text/uri-list` path text, which diverges from the dispatch path's
    // content-based hash and makes the receiver create a second entry. The
    // counts pin which stage produced no digest on the next repro: no `LocalFile`
    // rep AND no extractable inline path (`extracted_uri_list_paths = 0`) means
    // the file rep was not recognised / not a parseable `file:` URL; a non-zero
    // path count with `derived_digests = 0` means at least one `hash_path` failed
    // and the partial set was discarded (each failure logs its own warn above).
    debug!(
        local_file_digests,
        extracted_uri_list_paths = extracted_paths,
        derived_digests = digests.len(),
        "capture: derived file content digests for snapshot identity"
    );

    digests
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
        // ADR-005 §2.5 用户主动 resend:复用既有 entry 重发 fan-out,不产生
        // 新 entry,也不应该计入 capture 漏斗 —— 它代表"已有 entry 的二次
        // 同步尝试",与 RemotePush 同样需要在 telemetry 上被剔除,避免污染
        // "首次同步"与"复制 → 同步延迟"等指标。实际上 ResendEntryUseCase
        // 不经 clipboard_capture 路径,正常情况下这里不会被命中;留 arm 让
        // match 在 exhaustive 上闭合,并明确语义。
        ClipboardChangeOrigin::Resend => None,
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
            file_content_digests: Vec::new(),
        }
    }

    /// Fake ingest whose content hash is keyed on the file *name*, so two
    /// devices addressing the same file by different absolute paths produce
    /// the same content hash — modelling identical bytes behind device-local
    /// paths without touching the filesystem.
    struct FakeIngestByName;

    impl FakeIngestByName {
        fn name_hash(source_path: &std::path::Path) -> uc_core::ContentHash {
            let name = source_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            let mut bytes = [0u8; 32];
            let nb = name.as_bytes();
            let n = nb.len().min(32);
            bytes[..n].copy_from_slice(&nb[..n]);
            uc_core::ContentHash::from(&bytes)
        }
    }

    #[async_trait::async_trait]
    impl BlobContentIngestPort for FakeIngestByName {
        async fn ingest_path(
            &self,
            source_path: &std::path::Path,
        ) -> anyhow::Result<uc_core::blob::ports::IngestedBlob> {
            Ok(uc_core::blob::ports::IngestedBlob {
                blob_id: uc_core::ids::BlobId::new(),
                content_hash: Self::name_hash(source_path),
                size_bytes: 0,
            })
        }

        async fn hash_path(
            &self,
            source_path: &std::path::Path,
        ) -> anyhow::Result<uc_core::ContentHash> {
            Ok(Self::name_hash(source_path))
        }
    }

    /// Core of the double-channel dedup fix: capture must derive a file
    /// entry's identity from device-independent file *content*, not the
    /// device-local `text/uri-list` path text. Two devices copying the same
    /// file (different absolute paths, same content) must produce the same
    /// `snapshot_hash` — otherwise the receiver creates two entries.
    #[tokio::test]
    async fn inline_uri_list_identity_is_device_independent() {
        let blob_ingest = FakeIngestByName;

        // Same file ("report.msi"), addressed by two device-local paths.
        let snap_a = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///Users/alice/report.msi",
        )]);
        let snap_b = snapshot_with(vec![rep(
            "files",
            Some("text/uri-list"),
            b"file:///home/bob/report.msi",
        )]);

        // Sanity: without content digests the bare uri-list snapshots hash
        // differently (the bug) — identity leaks the device-local path text.
        assert_ne!(
            snap_a.snapshot_hash(),
            snap_b.snapshot_hash(),
            "bare uri-list identity must differ by device-local path text (pre-fix state)"
        );

        let mut a = snap_a.clone();
        let mut b = snap_b.clone();
        a.file_content_digests = derive_file_content_digests(&a, &blob_ingest).await;
        b.file_content_digests = derive_file_content_digests(&b, &blob_ingest).await;

        assert!(
            !a.file_content_digests.is_empty(),
            "capture must fill content digests for inline uri-list files"
        );
        assert_eq!(
            a.file_content_digests, b.file_content_digests,
            "same file content → same digests regardless of device-local path"
        );
        assert_eq!(
            a.snapshot_hash(),
            b.snapshot_hash(),
            "content-based identity must be device-independent (fixes the double-entry split)"
        );
        assert_ne!(
            a.snapshot_hash(),
            snap_a.snapshot_hash(),
            "filling content digests must move identity off the path-text hash"
        );
    }

    /// A per-file ingest failure must be skipped (not abort the capture); when
    /// every referenced file fails, the digest list is empty and identity
    /// falls back to the uri-list text.
    #[tokio::test]
    async fn inline_uri_list_ingest_failure_is_skipped() {
        struct AlwaysFails;
        #[async_trait::async_trait]
        impl BlobContentIngestPort for AlwaysFails {
            async fn ingest_path(
                &self,
                _: &std::path::Path,
            ) -> anyhow::Result<uc_core::blob::ports::IngestedBlob> {
                Err(anyhow::anyhow!("unreadable"))
            }

            async fn hash_path(&self, _: &std::path::Path) -> anyhow::Result<uc_core::ContentHash> {
                Err(anyhow::anyhow!("unreadable"))
            }
        }

        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///Users/alice/report.msi",
        )]);
        let digests = derive_file_content_digests(&snap, &AlwaysFails).await;
        assert!(
            digests.is_empty(),
            "all-files-failed must yield no digests (identity falls back to uri-list text)"
        );
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

    // --- resurface_existing_entry: local-capture dedup decision ---------

    /// What the fake repo's `touch_entry` should simulate.
    enum Touch {
        /// A row was updated — the entry still exists.
        Updated,
        /// 0 rows updated — the entry was deleted between find and touch.
        NoRows,
        /// The update itself failed.
        Err,
    }

    /// Minimal fake implementing only the two narrow ports
    /// `resurface_existing_entry` depends on.
    struct DedupFakeRepo {
        /// `Ok(_)` value returned by `find_entry_id_by_snapshot_hash`.
        found: Option<EntryId>,
        /// When true, the lookup returns `Err` instead of `Ok(found)`.
        find_err: bool,
        touch: Touch,
    }

    use uc_core::clipboard::ClipboardRepositoryError;

    #[async_trait::async_trait]
    impl FindEntryIdBySnapshotHashPort for DedupFakeRepo {
        async fn find_entry_id_by_snapshot_hash(
            &self,
            _snapshot_hash: &str,
        ) -> Result<Option<EntryId>, ClipboardRepositoryError> {
            if self.find_err {
                return Err(ClipboardRepositoryError::Storage(
                    "simulated dedup lookup failure".to_string(),
                ));
            }
            Ok(self.found.clone())
        }
    }

    #[async_trait::async_trait]
    impl TouchClipboardEntryPort for DedupFakeRepo {
        async fn touch_entry(
            &self,
            _entry_id: &EntryId,
            _active_time_ms: i64,
        ) -> Result<bool, ClipboardRepositoryError> {
            match self.touch {
                Touch::Updated => Ok(true),
                Touch::NoRows => Ok(false),
                Touch::Err => Err(ClipboardRepositoryError::Storage(
                    "simulated touch failure".to_string(),
                )),
            }
        }
    }

    #[tokio::test]
    async fn resurface_returns_entry_when_found_and_touched() {
        let repo = DedupFakeRepo {
            found: Some(EntryId::from("e1")),
            find_err: false,
            touch: Touch::Updated,
        };
        let out = resurface_existing_entry(&repo, &repo, "blake3v1:abc", 123).await;
        assert_eq!(out, Some(EntryId::from("e1")));
    }

    #[tokio::test]
    async fn resurface_degrades_when_touch_updates_no_rows() {
        // Entry was deleted between find and touch (concurrent cleanup):
        // returning a stale id would broadcast a non-existent entry, so the
        // capture must degrade to creating a fresh entry instead.
        let repo = DedupFakeRepo {
            found: Some(EntryId::from("e1")),
            find_err: false,
            touch: Touch::NoRows,
        };
        assert_eq!(
            resurface_existing_entry(&repo, &repo, "blake3v1:abc", 123).await,
            None
        );
    }

    #[tokio::test]
    async fn resurface_degrades_when_touch_errors() {
        let repo = DedupFakeRepo {
            found: Some(EntryId::from("e1")),
            find_err: false,
            touch: Touch::Err,
        };
        assert_eq!(
            resurface_existing_entry(&repo, &repo, "blake3v1:abc", 123).await,
            None
        );
    }

    #[tokio::test]
    async fn resurface_returns_none_when_no_match() {
        let repo = DedupFakeRepo {
            found: None,
            find_err: false,
            touch: Touch::Updated,
        };
        assert_eq!(
            resurface_existing_entry(&repo, &repo, "blake3v1:abc", 123).await,
            None
        );
    }

    #[tokio::test]
    async fn resurface_returns_none_when_lookup_errors() {
        let repo = DedupFakeRepo {
            found: None,
            find_err: true,
            touch: Touch::Updated,
        };
        assert_eq!(
            resurface_existing_entry(&repo, &repo, "blake3v1:abc", 123).await,
            None
        );
    }
}
