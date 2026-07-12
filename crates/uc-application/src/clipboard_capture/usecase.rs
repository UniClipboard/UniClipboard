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
use unicode_normalization::UnicodeNormalization;

use uc_core::blob::ports::BlobContentIngestPort;
use uc_core::clipboard::{
    is_file_mime_or_format, ClipboardPayloadSource, EntryFileSet, EntryFileSetExcludeReason,
    EntryFileSetLine, EntryFileSetLineKind, FileSetMemberKind, FileSetMemberLocation,
    PersistedClipboardRepresentation,
};

use crate::facade::clipboard_outbound::{parse_uri_list_line, UriListLineKind};
use uc_core::ids::{EntryId, EventId};
use uc_core::ports::clipboard::{
    EntryFileSetRepositoryPort, FindEntryIdBySnapshotHashPort, ReplaceEntryContentPort,
    RepresentationCachePort, SaveClipboardEntryPort, SpoolQueuePort, SpoolRequest,
    TouchClipboardEntryPort,
};
use uc_core::ports::{
    ClipboardEventWriterPort, ClipboardRepresentationNormalizerPort, DeviceIdentityPort,
    SelectRepresentationPolicyPort, SettingsPort,
};
use uc_core::{
    ClipboardChangeOrigin, ClipboardEntry, ClipboardEntryContentCategory, ClipboardEvent,
    ClipboardSelectionDecision, ObservedClipboardRepresentation, PayloadAvailability, SnapshotHash,
    SystemClipboardSnapshot,
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
    /// Persists the file-class entry's line-level manifest built from this
    /// same capture (see [`build_entry_file_set`]) so later readers (out of
    /// this phase's scope) don't have to re-parse/re-hash the source data.
    entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,
    /// Source of the file-set capture caps (`max_file_set_total_bytes` /
    /// `max_file_set_member_count`). Read only when building a file-class
    /// manifest, so text/image captures never pay the settings load.
    settings: Arc<dyn SettingsPort>,
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
        entry_file_set_repo: Arc<dyn EntryFileSetRepositoryPort>,
        settings: Arc<dyn SettingsPort>,
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
            entry_file_set_repo,
            settings,
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
            // Build this capture's file-set manifest (line-level: kept for
            // persistence below) and use it to populate `file_content_digests`
            // / `file_set_v1_component` so `snapshot_hash()` is based on
            // device-independent file *content* rather than the text/uri-list
            // path text (device-specific). Skipped when either is already
            // populated (RemotePush: the inbound materializer fills these
            // from the wire before this capture runs; it does not yet build
            // a manifest — out of this phase's scope). No current snapshot
            // constructor pre-populates `file_set_v1_component`, so that half
            // of the guard is a no-op today; it exists for parity with
            // `file_content_digests` and to cover a future wire path that
            // carries the component directly.
            let mut file_set: Option<EntryFileSet> = None;
            // Only file-class captures build a manifest, so keep the text/image
            // hot path off the settings port (see the `settings` field doc).
            // Mirrors the file-class detection inside `build_entry_file_set`.
            let is_file_class = snapshot.representations.iter().any(|rep| {
                matches!(rep.source(), ClipboardPayloadSource::LocalFile { .. })
                    || is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id)
            });
            if snapshot.file_content_digests.is_empty()
                && snapshot.file_set_v1_component.is_none()
                && is_file_class
            {
                // Read the file-set caps for this capture. A load failure must
                // not drop the capture, but it must stay bounded — fall back to
                // the conservative ADR-010 default ceiling rather than no cap,
                // so a transient settings error can't let a directory capture
                // traverse and hash an entire tree unbounded.
                let caps = match self.settings.load().await {
                    Ok(s) => FileSetCaps {
                        max_total_bytes: s.file_sync.max_file_set_total_bytes,
                        max_member_count: s.file_sync.max_file_set_member_count,
                    },
                    Err(err) => {
                        warn!(error = %err, "capture: settings load failed; using fallback file-set caps for this capture");
                        FileSetCaps::fallback()
                    }
                };
                if let Some(built) =
                    build_entry_file_set(&snapshot, self.blob_ingest.as_ref(), caps).await
                {
                    if let Some(component) = built.file_set_v1_component() {
                        snapshot.file_set_v1_component = Some(component);
                    } else {
                        let digests = built.content_digest_contribution();
                        if !digests.is_empty() {
                            snapshot.file_content_digests = digests;
                        }
                    }
                    file_set = Some(built);
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
                let total_size = snapshot.total_size_bytes();
                let content_category = ClipboardEntryContentCategory::from_snapshot(&snapshot);
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
                            total_size,
                        )
                        .with_content_category(content_category);
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
                            total_size,
                            content_category,
                        )
                        .await
                        .map_err(anyhow::Error::from),
                }
            }
            .instrument(info_span!(stages::PERSIST_ENTRY))
            .await?;

            // Persist the file-set manifest built above, now that `entry_id`
            // is durable. Best-effort: a failure here does not roll back the
            // capture (nothing reads this manifest yet — see module doc);
            // it's logged so a persistently-failing save is visible.
            //
            // FIXME(followup): once a reader exists (dispatch/resend/display),
            // this best-effort save becomes a real inconsistency source — the
            // entry would exist while `load()` returns `Ok(None)`, which is
            // indistinguishable from "not a file-class entry". At that point
            // either make this save part of the entry's persistence
            // transaction, or give the reader a way to tell the two apart.
            if let Some(file_set) = &file_set {
                if let Err(err) = self.entry_file_set_repo.save(&entry_id, file_set).await {
                    warn!(
                        entry_id = %entry_id,
                        error = %err,
                        "capture: failed to persist entry file-set manifest"
                    );
                }
            }

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

/// Whole-set capture caps (ADR-010). Zero means "no cap" for that dimension.
#[derive(Debug, Clone, Copy)]
struct FileSetCaps {
    /// Total bytes across all file members. `0` disables the byte cap.
    max_total_bytes: u64,
    /// Number of file members. `0` disables the count cap.
    max_member_count: u64,
}

impl FileSetCaps {
    /// Conservative fallback caps for when the settings load fails: a transient
    /// error must not silently stop file sync, but it also must not disable the
    /// caps entirely — with directory capture, unbounded caps would let one
    /// settings hiccup traverse and hash an entire directory tree. Mirror the
    /// ADR-010 defaults (`uc-core` `Settings::default`: 1 GiB / 2000 members)
    /// so the fallback stays bounded without dropping the capture.
    fn fallback() -> Self {
        Self {
            max_total_bytes: 1024 * 1024 * 1024, // 1 GiB
            max_member_count: 2000,
        }
    }

    /// No cap on either dimension. Test-only: exercises the manifest builder
    /// without cap interference. Production never disables the caps — a
    /// settings-load failure uses [`Self::fallback`], not this.
    #[cfg(test)]
    fn unbounded() -> Self {
        Self {
            max_total_bytes: 0,
            max_member_count: 0,
        }
    }
}

/// Build the line-level [`EntryFileSet`] manifest for a freshly captured
/// file-class snapshot. Returns `None` when the snapshot carries no
/// resolvable file lines (not a file-class snapshot at all).
///
/// Two file-rep shapes contribute lines:
/// - `ClipboardPayloadSource::LocalFile` reps (e.g. macOS Finder copy): one
///   line per rep, keyed by its path (there is no backing uri-list text to
///   preserve, so the path's display form stands in for `original_text`).
/// - An inline `text/uri-list` file rep (e.g. Windows file copy): one line
///   per line of that rep's text, in original order — including blank/
///   comment/non-file lines, so the manifest can later distinguish "one more
///   line" or "different line order" as a different identity.
///
/// The two shapes are mutually exclusive in practice (a snapshot carries
/// either `LocalFile` reps or an inline uri-list rep), so the inline branch
/// only runs when no `LocalFile` rep is present.
///
/// # Whole-set caps (ADR-010)
///
/// Before hashing, a metadata-only traversal expands directory leaves while
/// summing member sizes and counts. If either `caps` dimension is exceeded,
/// traversal stops immediately and every discovered file line is marked
/// `Excluded { SizeCapExceeded }` and hashing is skipped entirely: an
/// over-budget set's identity falls back to path text anyway, so streaming
/// gigabytes just to discard the digests would be pure waste. The set is still
/// admitted to local history; only sync is suppressed (the outbound path skips
/// any manifest with excluded lines). The check is all-or-nothing to match
/// [`EntryFileSet::content_digest_contribution`]. Symlinks and non-regular
/// special files stop the same traversal without being dereferenced.
///
/// Each file line's content hash comes from the fallible
/// [`BlobContentIngestPort::hash_path`] (never a rep's `content_hash()`,
/// which `panic!`s on a stream-hash error) — identity only, without
/// materializing a blob. Blob materialization is a separate, later step (the
/// dispatch path publishes lazily); doing it here would both stall the
/// capture loop on large files and orphan a blob this entry never
/// references. A hash failure marks just that one line `Excluded` so the
/// manifest still records "this line was a file we couldn't read" for
/// display/debugging, rather than silently dropping it. Crucially this does
/// NOT weaken the entry's identity: any `Excluded` line makes
/// [`EntryFileSet::content_digest_contribution`] return empty (all-or-
/// nothing), so identity falls back to path-text — matching the dispatch
/// side (`publish_file_blob_refs`), which is itself all-or-nothing. This
/// avoids keying identity on a device-/timing-dependent subset of readable
/// files (the dual-channel file dedup divergence bug).
async fn build_entry_file_set(
    snapshot: &SystemClipboardSnapshot,
    blob_ingest: &dyn BlobContentIngestPort,
    caps: FileSetCaps,
) -> Option<EntryFileSet> {
    let local_file_members: Vec<TopLevelFileMember> = snapshot
        .representations
        .iter()
        .filter_map(|rep| match rep.source() {
            ClipboardPayloadSource::LocalFile { path, size_bytes } => {
                Some((path.clone(), *size_bytes))
            }
            ClipboardPayloadSource::Inline(_) => None,
        })
        .enumerate()
        .map(|(idx, (path, size_bytes))| TopLevelFileMember {
            line_index: idx as i64,
            root_index: idx as i64,
            original_text: path.display().to_string(),
            root_name: normalized_basename(&path),
            path,
            known_size: Some(size_bytes),
        })
        .collect();

    if !local_file_members.is_empty() {
        let top_level_count = local_file_members.len() as i64;
        let lines = build_file_member_lines(
            local_file_members,
            Vec::new(),
            top_level_count,
            blob_ingest,
            caps,
        )
        .await;
        debug!(
            line_count = lines.len(),
            "capture: built file-set manifest from LocalFile reps"
        );
        return Some(EntryFileSet { lines });
    }

    let uri_list_text = snapshot.representations.iter().find_map(|rep| {
        if !is_file_mime_or_format(rep.mime.as_ref(), &rep.format_id) {
            return None;
        }
        rep.inline_bytes()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
            .map(str::to_string)
    })?;

    let mut file_members = Vec::new();
    let mut non_file_lines = Vec::new();
    let mut root_index = 0i64;
    let top_level_count = uri_list_text.lines().count() as i64;
    for (idx, raw_line) in uri_list_text.lines().enumerate() {
        match parse_uri_list_line(raw_line) {
            UriListLineKind::File(path) => file_members.push(TopLevelFileMember {
                line_index: idx as i64,
                root_index: {
                    let current = root_index;
                    root_index = root_index.saturating_add(1);
                    current
                },
                original_text: raw_line.to_string(),
                root_name: normalized_basename(&path),
                path,
                known_size: None,
            }),
            UriListLineKind::NonFile => non_file_lines.push(EntryFileSetLine {
                line_index: idx as i64,
                original_text: raw_line.to_string(),
                member_location: None,
                kind: EntryFileSetLineKind::NonFile,
            }),
        }
    }
    let lines = build_file_member_lines(
        file_members,
        non_file_lines,
        top_level_count,
        blob_ingest,
        caps,
    )
    .await;
    debug!(
        line_count = lines.len(),
        "capture: built file-set manifest from inline uri-list text"
    );
    Some(EntryFileSet { lines })
}

struct TopLevelFileMember {
    line_index: i64,
    root_index: i64,
    original_text: String,
    root_name: String,
    path: std::path::PathBuf,
    known_size: Option<u64>,
}

struct PendingFileSetLine {
    line_index: i64,
    original_text: String,
    member_location: FileSetMemberLocation,
    file_path: Option<std::path::PathBuf>,
}

#[derive(Debug, Clone, Copy)]
enum ExpansionFailure {
    SizeCapExceeded,
    UnsupportedMember,
    IngestFailed,
}

impl ExpansionFailure {
    fn exclude_reason(self) -> EntryFileSetExcludeReason {
        match self {
            Self::SizeCapExceeded => EntryFileSetExcludeReason::SizeCapExceeded,
            Self::UnsupportedMember => EntryFileSetExcludeReason::UnsupportedMember,
            Self::IngestFailed => EntryFileSetExcludeReason::IngestFailed,
        }
    }
}

#[derive(Default)]
struct TraversalBudget {
    member_count: u64,
    total_bytes: u64,
}

impl TraversalBudget {
    fn admit(&mut self, size_bytes: u64, caps: FileSetCaps) -> bool {
        self.member_count = self.member_count.saturating_add(1);
        self.total_bytes = self.total_bytes.saturating_add(size_bytes);
        (caps.max_member_count > 0 && self.member_count > caps.max_member_count)
            || (caps.max_total_bytes > 0 && self.total_bytes > caps.max_total_bytes)
    }
}

async fn build_file_member_lines(
    roots: Vec<TopLevelFileMember>,
    mut lines: Vec<EntryFileSetLine>,
    top_level_count: i64,
    blob_ingest: &dyn BlobContentIngestPort,
    caps: FileSetCaps,
) -> Vec<EntryFileSetLine> {
    if caps.max_member_count > 0 && roots.len() as u64 > caps.max_member_count {
        lines.extend(
            roots
                .into_iter()
                .map(|root| excluded_root_line(root, EntryFileSetExcludeReason::SizeCapExceeded)),
        );
        lines.sort_by_key(|line| line.line_index);
        return lines;
    }

    let mut budget = TraversalBudget::default();
    let mut pending = Vec::new();
    let mut next_directory_line = top_level_count;
    let mut failure = None;

    let mut roots = roots.into_iter();
    while let Some(root) = roots.next() {
        let metadata = tokio::fs::symlink_metadata(&root.path).await;
        match metadata {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                pending.push(pending_root_marker(root, next_directory_line));
                failure = Some(ExpansionFailure::UnsupportedMember);
                break;
            }
            Ok(metadata) if metadata.is_dir() => {
                match expand_directory(&root, &mut next_directory_line, &mut budget, caps).await {
                    Ok(mut members) => pending.append(&mut members),
                    Err((reason, marker)) => {
                        pending.push(marker);
                        failure = Some(reason);
                        break;
                    }
                }
            }
            Ok(metadata) if metadata.is_file() => {
                let size = root.known_size.unwrap_or_else(|| metadata.len());
                if budget.admit(size, caps) {
                    pending.push(pending_flat_file(root, metadata));
                    failure = Some(ExpansionFailure::SizeCapExceeded);
                    break;
                }
                pending.push(pending_flat_file(root, metadata));
            }
            Ok(_) => {
                pending.push(pending_root_marker(root, next_directory_line));
                failure = Some(ExpansionFailure::UnsupportedMember);
                break;
            }
            Err(_) => {
                let size = root.known_size.unwrap_or(0);
                if budget.admit(size, caps) {
                    pending.push(pending_missing_file(root));
                    failure = Some(ExpansionFailure::SizeCapExceeded);
                    break;
                }
                pending.push(pending_missing_file(root));
            }
        }
    }

    if let Some(failure) = failure {
        let reason = failure.exclude_reason();
        pending.extend(roots.map(|root| PendingFileSetLine {
            line_index: root.line_index,
            original_text: root.original_text,
            member_location: FileSetMemberLocation {
                root_index: root.root_index,
                root_name: root.root_name,
                relative_path: normalized_basename(&root.path),
                kind: FileSetMemberKind::File,
            },
            file_path: None,
        }));
        warn!(
            reason = ?reason,
            "capture: file-set traversal stopped; the whole set is ineligible"
        );
        lines.extend(pending.into_iter().map(|line| EntryFileSetLine {
            line_index: line.line_index,
            original_text: line.original_text,
            member_location: Some(line.member_location),
            kind: EntryFileSetLineKind::Excluded { reason },
        }));
    } else {
        for line in pending {
            let kind = match line.file_path {
                Some(path) => classify_file_path(&path, blob_ingest, false).await,
                None => EntryFileSetLineKind::NonFile,
            };
            lines.push(EntryFileSetLine {
                line_index: line.line_index,
                original_text: line.original_text,
                member_location: Some(line.member_location),
                kind,
            });
        }
    }
    lines.sort_by_key(|line| line.line_index);
    lines
}

fn pending_flat_file(root: TopLevelFileMember, metadata: std::fs::Metadata) -> PendingFileSetLine {
    let relative_path = normalized_basename(&root.path);
    PendingFileSetLine {
        line_index: root.line_index,
        original_text: root.original_text,
        member_location: FileSetMemberLocation {
            root_index: root.root_index,
            root_name: root.root_name,
            relative_path,
            kind: member_kind(&metadata),
        },
        file_path: Some(root.path),
    }
}

fn pending_missing_file(root: TopLevelFileMember) -> PendingFileSetLine {
    let relative_path = normalized_basename(&root.path);
    PendingFileSetLine {
        line_index: root.line_index,
        original_text: root.original_text,
        member_location: FileSetMemberLocation {
            root_index: root.root_index,
            root_name: root.root_name,
            relative_path,
            kind: FileSetMemberKind::File,
        },
        file_path: Some(root.path),
    }
}

fn pending_root_marker(root: TopLevelFileMember, line_index: i64) -> PendingFileSetLine {
    PendingFileSetLine {
        line_index,
        original_text: root.original_text,
        member_location: FileSetMemberLocation {
            root_index: root.root_index,
            root_name: root.root_name,
            relative_path: ".".to_string(),
            kind: FileSetMemberKind::File,
        },
        file_path: None,
    }
}

fn excluded_root_line(
    root: TopLevelFileMember,
    reason: EntryFileSetExcludeReason,
) -> EntryFileSetLine {
    EntryFileSetLine {
        line_index: root.line_index,
        original_text: root.original_text,
        member_location: Some(FileSetMemberLocation {
            root_index: root.root_index,
            root_name: root.root_name,
            relative_path: normalized_basename(&root.path),
            kind: FileSetMemberKind::File,
        }),
        kind: EntryFileSetLineKind::Excluded { reason },
    }
}

async fn expand_directory(
    root: &TopLevelFileMember,
    next_line_index: &mut i64,
    budget: &mut TraversalBudget,
    caps: FileSetCaps,
) -> std::result::Result<Vec<PendingFileSetLine>, (ExpansionFailure, PendingFileSetLine)> {
    use std::collections::VecDeque;

    let mut pending = Vec::new();
    let mut queue = VecDeque::from([(root.path.clone(), String::new())]);
    while let Some((directory, relative_directory)) = queue.pop_front() {
        let mut read_dir = tokio::fs::read_dir(&directory).await.map_err(|_| {
            (
                ExpansionFailure::IngestFailed,
                directory_marker(root, next_line_index, &relative_directory),
            )
        })?;
        let mut entries = Vec::new();
        while let Some(entry) = read_dir.next_entry().await.map_err(|_| {
            (
                ExpansionFailure::IngestFailed,
                directory_marker(root, next_line_index, &relative_directory),
            )
        })? {
            entries.push(entry);
        }
        entries.sort_by_key(|entry| entry.file_name());

        if entries.is_empty() {
            let relative_path = if relative_directory.is_empty() {
                ".".to_string()
            } else {
                relative_directory.clone()
            };
            let marker = PendingFileSetLine {
                line_index: take_line_index(next_line_index),
                original_text: root.original_text.clone(),
                member_location: FileSetMemberLocation {
                    root_index: root.root_index,
                    root_name: root.root_name.clone(),
                    relative_path,
                    kind: FileSetMemberKind::EmptyDirectory,
                },
                file_path: None,
            };
            if budget.admit(0, caps) {
                return Err((ExpansionFailure::SizeCapExceeded, marker));
            }
            pending.push(marker);
            continue;
        }

        for entry in entries {
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                return Err((
                    ExpansionFailure::UnsupportedMember,
                    directory_marker(root, next_line_index, &relative_directory),
                ));
            };
            let name = name.nfc().collect::<String>();
            let relative_path = if relative_directory.is_empty() {
                name
            } else {
                format!("{relative_directory}/{name}")
            };
            let path = entry.path();
            let metadata = tokio::fs::symlink_metadata(&path).await.map_err(|_| {
                (
                    ExpansionFailure::IngestFailed,
                    directory_marker(root, next_line_index, &relative_path),
                )
            })?;
            if metadata.file_type().is_symlink() {
                return Err((
                    ExpansionFailure::UnsupportedMember,
                    directory_marker(root, next_line_index, &relative_path),
                ));
            }
            if metadata.is_dir() {
                queue.push_back((path, relative_path));
                continue;
            }
            if !metadata.is_file() {
                return Err((
                    ExpansionFailure::UnsupportedMember,
                    directory_marker(root, next_line_index, &relative_path),
                ));
            }

            let member = PendingFileSetLine {
                line_index: take_line_index(next_line_index),
                original_text: root.original_text.clone(),
                member_location: FileSetMemberLocation {
                    root_index: root.root_index,
                    root_name: root.root_name.clone(),
                    relative_path,
                    kind: member_kind(&metadata),
                },
                file_path: Some(path),
            };
            if budget.admit(metadata.len(), caps) {
                return Err((ExpansionFailure::SizeCapExceeded, member));
            }
            pending.push(member);
        }
    }
    Ok(pending)
}

fn directory_marker(
    root: &TopLevelFileMember,
    next_line_index: &mut i64,
    relative_path: &str,
) -> PendingFileSetLine {
    PendingFileSetLine {
        line_index: take_line_index(next_line_index),
        original_text: root.original_text.clone(),
        member_location: FileSetMemberLocation {
            root_index: root.root_index,
            root_name: root.root_name.clone(),
            relative_path: if relative_path.is_empty() {
                ".".to_string()
            } else {
                relative_path.to_string()
            },
            kind: FileSetMemberKind::File,
        },
        file_path: None,
    }
}

fn take_line_index(next_line_index: &mut i64) -> i64 {
    let current = *next_line_index;
    *next_line_index = (*next_line_index).saturating_add(1);
    current
}

fn normalized_basename(path: &std::path::Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().nfc().collect())
        .unwrap_or_else(|| ".".to_string())
}

fn member_kind(metadata: &std::fs::Metadata) -> FileSetMemberKind {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 != 0 {
            return FileSetMemberKind::Executable;
        }
    }
    FileSetMemberKind::File
}

/// Classify one resolved file path into a manifest line's kind.
///
/// `over_cap` short-circuits to `Excluded { SizeCapExceeded }` without
/// hashing — the set is over budget, so its content digests would be
/// discarded anyway (see [`build_entry_file_set`]). Otherwise the content
/// hash comes from the fallible [`BlobContentIngestPort::hash_path`]; a hash
/// failure (unreadable/deleted file) degrades to `Excluded { IngestFailed }`
/// rather than propagating — a `LocalFile` rep's `content_hash()` would
/// `panic!` on the same failure, which is exactly what routing through the
/// fallible `hash_path` avoids.
async fn classify_file_path(
    path: &std::path::Path,
    blob_ingest: &dyn BlobContentIngestPort,
    over_cap: bool,
) -> EntryFileSetLineKind {
    if over_cap {
        return EntryFileSetLineKind::Excluded {
            reason: EntryFileSetExcludeReason::SizeCapExceeded,
        };
    }
    match blob_ingest.hash_path(path).await {
        Ok(content_hash) => EntryFileSetLineKind::File {
            content_hash,
            // Not yet materialized into a blob at capture time (see this
            // function's doc); a later, out-of-scope step fills these in.
            blob_id: None,
            size_bytes: None,
        },
        Err(err) => {
            // No path in the field: a clipboard file path is user content.
            warn!(error = %err, "capture: could not derive file-set line content hash");
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::IngestFailed,
            }
        }
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
            file_set_v1_component: None,
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directory_capture_expands_members_and_preserves_member_semantics() {
        use std::os::unix::fs::PermissionsExt;

        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("roote\u{301}");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("visible.txt"), b"visible").unwrap();
        std::fs::write(root.join(".hidden"), b"hidden").unwrap();
        std::fs::create_dir(root.join("nested")).unwrap();
        let executable = root.join("nested/run.sh");
        std::fs::write(&executable, b"#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir(root.join("empty")).unwrap();
        std::fs::write(root.join("e\u{301}.txt"), b"unicode").unwrap();

        let uri_list = format!("file://{}", root.display());
        let snapshot = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);
        let set = build_entry_file_set(&snapshot, &FakeIngestByName, FileSetCaps::unbounded())
            .await
            .expect("directory should produce a manifest");

        let members: Vec<_> = set
            .lines
            .iter()
            .map(|line| {
                let location = line
                    .member_location
                    .as_ref()
                    .expect("directory member location");
                (
                    location.root_name.as_str(),
                    location.relative_path.as_str(),
                    location.kind,
                )
            })
            .collect();
        assert!(members.contains(&("root\u{e9}", ".hidden", FileSetMemberKind::File)));
        assert!(members.contains(&("root\u{e9}", "visible.txt", FileSetMemberKind::File)));
        assert!(members.contains(&("root\u{e9}", "nested/run.sh", FileSetMemberKind::Executable)));
        assert!(members.contains(&("root\u{e9}", "empty", FileSetMemberKind::EmptyDirectory)));
        assert!(members.contains(&("root\u{e9}", "\u{e9}.txt", FileSetMemberKind::File)));
        assert!(set.has_directory_structure());
        assert!(set.content_digest_contribution().is_empty());
        assert!(set.file_set_v1_component().is_some());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn directory_capture_rejects_symlinks_and_special_files() {
        use std::os::unix::fs::symlink;
        use std::os::unix::net::UnixListener;

        for forbidden in ["symlink", "socket"] {
            let parent = tempfile::tempdir().unwrap();
            let root = parent.path().join("root");
            std::fs::create_dir(&root).unwrap();
            std::fs::write(root.join("target.txt"), b"target").unwrap();
            let _socket = if forbidden == "symlink" {
                symlink(root.join("target.txt"), root.join("forbidden")).unwrap();
                None
            } else {
                Some(UnixListener::bind(root.join("forbidden")).unwrap())
            };
            let uri_list = format!("file://{}", root.display());
            let snapshot = snapshot_with(vec![rep(
                "public.file-url",
                Some("text/uri-list"),
                uri_list.as_bytes(),
            )]);

            let set = build_entry_file_set(&snapshot, &PanicOnHash, FileSetCaps::unbounded())
                .await
                .expect("ineligible directory should still produce a manifest");
            assert!(set.lines.iter().any(|line| matches!(
                line.kind,
                EntryFileSetLineKind::Excluded {
                    reason: EntryFileSetExcludeReason::UnsupportedMember
                }
            )));
            assert_eq!(set.file_lines().count(), 0);
        }
    }

    #[tokio::test]
    async fn directory_member_count_cap_stops_before_hashing() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("root");
        std::fs::create_dir(&root).unwrap();
        for name in ["a", "b", "c"] {
            std::fs::write(root.join(name), name.as_bytes()).unwrap();
        }
        let uri_list = format!("file://{}", root.display());
        let snapshot = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);

        let set = build_entry_file_set(&snapshot, &PanicOnHash, caps(0, 2))
            .await
            .expect("over-cap directory should produce a manifest");
        assert!(set.lines.iter().all(|line| matches!(
            line.kind,
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::SizeCapExceeded
            }
        )));
    }

    #[tokio::test]
    async fn traversal_failure_keeps_remaining_top_level_roots() {
        let parent = tempfile::tempdir().unwrap();
        let paths: Vec<_> = ["a", "b", "c"]
            .into_iter()
            .map(|name| {
                let path = parent.path().join(name);
                std::fs::write(&path, b"xx").unwrap();
                path
            })
            .collect();
        let uri_list = paths
            .iter()
            .map(|path| format!("file://{}", path.display()))
            .collect::<Vec<_>>()
            .join("\n");
        let snapshot = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);

        let set = build_entry_file_set(&snapshot, &PanicOnHash, caps(1, 0))
            .await
            .expect("over-cap selection should produce a manifest");
        assert_eq!(set.lines.len(), 3);
        assert!(set.lines.iter().all(|line| matches!(
            line.kind,
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::SizeCapExceeded
            }
        )));
    }

    #[tokio::test]
    async fn mixed_file_and_directory_keep_stable_root_indexes() {
        let parent = tempfile::tempdir().unwrap();
        let loose = parent.path().join("loose.txt");
        std::fs::write(&loose, b"loose").unwrap();
        let root = parent.path().join("root");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("child.txt"), b"child").unwrap();
        let uri_list = format!("file://{}\nfile://{}", loose.display(), root.display());
        let snapshot = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);

        let set = build_entry_file_set(&snapshot, &FakeIngestByName, FileSetCaps::unbounded())
            .await
            .expect("mixed selection should produce a manifest");
        let flat = set
            .lines
            .iter()
            .find(|line| line.line_index == 0)
            .unwrap()
            .member_location
            .as_ref()
            .unwrap();
        assert_eq!(
            (
                flat.root_index,
                flat.root_name.as_str(),
                flat.relative_path.as_str()
            ),
            (0, "loose.txt", "loose.txt")
        );
        let child = set
            .lines
            .iter()
            .find(|line| {
                line.member_location
                    .as_ref()
                    .is_some_and(|location| location.relative_path == "child.txt")
            })
            .unwrap();
        assert_eq!(
            (
                child.member_location.as_ref().unwrap().root_index,
                child.member_location.as_ref().unwrap().root_name.as_str()
            ),
            (1, "root")
        );
        assert!(child.line_index >= 2);
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
        a.file_content_digests = build_entry_file_set(&a, &blob_ingest, FileSetCaps::unbounded())
            .await
            .expect("uri-list snapshot should yield a file-set")
            .content_digest_contribution();
        b.file_content_digests = build_entry_file_set(&b, &blob_ingest, FileSetCaps::unbounded())
            .await
            .expect("uri-list snapshot should yield a file-set")
            .content_digest_contribution();

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

    #[tokio::test]
    async fn directory_identity_is_stable_across_devices_and_distinct_from_flat_files() {
        let fixture = tempfile::tempdir().unwrap();
        let device_a = fixture.path().join("device-a/root");
        let device_b = fixture.path().join("device-b/root");
        for root in [&device_a, &device_b] {
            std::fs::create_dir_all(root).unwrap();
            std::fs::write(root.join("1.txt"), b"one").unwrap();
            std::fs::write(root.join("2.txt"), b"two").unwrap();
        }

        let make_directory_snapshot = |root: &std::path::Path| {
            snapshot_with(vec![rep(
                "files",
                Some("text/uri-list"),
                format!("file://{}", root.display()).as_bytes(),
            )])
        };
        let mut directory_a = make_directory_snapshot(&device_a);
        let mut directory_b = make_directory_snapshot(&device_b);
        for snapshot in [&mut directory_a, &mut directory_b] {
            let set = build_entry_file_set(snapshot, &FakeIngestByName, FileSetCaps::unbounded())
                .await
                .expect("directory manifest");
            snapshot.file_set_v1_component = set.file_set_v1_component();
        }
        assert_eq!(directory_a.snapshot_hash(), directory_b.snapshot_hash());

        let flat_a = fixture.path().join("flat-a/1.txt");
        let flat_b = fixture.path().join("flat-b/2.txt");
        std::fs::create_dir_all(flat_a.parent().unwrap()).unwrap();
        std::fs::create_dir_all(flat_b.parent().unwrap()).unwrap();
        std::fs::write(&flat_a, b"one").unwrap();
        std::fs::write(&flat_b, b"two").unwrap();
        let mut flat = snapshot_with(vec![rep(
            "files",
            Some("text/uri-list"),
            format!("file://{}\nfile://{}", flat_a.display(), flat_b.display()).as_bytes(),
        )]);
        flat.file_content_digests =
            build_entry_file_set(&flat, &FakeIngestByName, FileSetCaps::unbounded())
                .await
                .expect("flat manifest")
                .content_digest_contribution();

        assert_ne!(directory_a.snapshot_hash(), flat.snapshot_hash());
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
        let file_set = build_entry_file_set(&snap, &AlwaysFails, FileSetCaps::unbounded())
            .await
            .expect("uri-list snapshot should still yield a file-set (all lines excluded)");
        assert!(
            file_set.content_digest_contribution().is_empty(),
            "all-files-failed must yield no digests (identity falls back to uri-list text)"
        );
    }

    /// Multi-file uri-list where only *some* files hash successfully. Identity
    /// must stay all-or-nothing: a partial subset of digests would key the
    /// entry on whichever files happened to be readable at capture time, which
    /// can differ across devices/retries and re-introduces the double-entry
    /// dedup split. So a partial failure must yield an EMPTY contribution (fall
    /// back to path-text), NOT the digests of the readable subset.
    #[tokio::test]
    async fn inline_uri_list_partial_ingest_failure_falls_back_to_empty_identity() {
        // Fails only for "locked.msi"; hashes any other path by name.
        struct FailsOne;
        #[async_trait::async_trait]
        impl BlobContentIngestPort for FailsOne {
            async fn ingest_path(
                &self,
                _: &std::path::Path,
            ) -> anyhow::Result<uc_core::blob::ports::IngestedBlob> {
                Err(anyhow::anyhow!("unused"))
            }

            async fn hash_path(
                &self,
                source_path: &std::path::Path,
            ) -> anyhow::Result<uc_core::ContentHash> {
                if source_path.file_name().and_then(|n| n.to_str()) == Some("locked.msi") {
                    Err(anyhow::anyhow!("locked"))
                } else {
                    Ok(FakeIngestByName::name_hash(source_path))
                }
            }
        }

        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///Users/alice/ok-a.txt\nfile:///Users/alice/locked.msi\nfile:///Users/alice/ok-b.txt",
        )]);
        let file_set = build_entry_file_set(&snap, &FailsOne, FileSetCaps::unbounded())
            .await
            .expect("uri-list snapshot should yield a file-set");

        // The manifest still records all three lines (two File, one Excluded).
        assert_eq!(file_set.lines.len(), 3);
        assert_eq!(file_set.file_lines().count(), 2);

        assert!(
            file_set.content_digest_contribution().is_empty(),
            "partial ingest failure must yield NO digests (all-or-nothing), not the readable subset"
        );
    }

    // ── whole-set caps (ADR-010) ────────────────────────────────────────

    /// Hasher that fails the test if ever asked to hash — proves the
    /// size-cap branch skips content hashing entirely.
    struct PanicOnHash;
    #[async_trait::async_trait]
    impl BlobContentIngestPort for PanicOnHash {
        async fn ingest_path(
            &self,
            _: &std::path::Path,
        ) -> anyhow::Result<uc_core::blob::ports::IngestedBlob> {
            panic!("ingest_path must not be called once the file-set cap is tripped")
        }
        async fn hash_path(&self, _: &std::path::Path) -> anyhow::Result<uc_core::ContentHash> {
            panic!("hash_path must not be called once the file-set cap is tripped")
        }
    }

    fn caps(total_bytes: u64, member_count: u64) -> FileSetCaps {
        FileSetCaps {
            max_total_bytes: total_bytes,
            max_member_count: member_count,
        }
    }

    /// Member-count cap trips before any filesystem access (the count check
    /// short-circuits), so the paths need not exist and hashing is skipped:
    /// every file line becomes `Excluded { SizeCapExceeded }`, identity falls
    /// back to path text.
    #[tokio::test]
    async fn member_count_cap_excludes_all_file_lines_without_hashing() {
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///Users/alice/a.txt\nfile:///Users/alice/b.txt\nfile:///Users/alice/c.txt",
        )]);
        // 3 members, cap = 2.
        let file_set = build_entry_file_set(&snap, &PanicOnHash, caps(0, 2))
            .await
            .expect("file-class snapshot yields a manifest");

        assert_eq!(file_set.lines.len(), 3);
        assert_eq!(
            file_set.file_lines().count(),
            0,
            "over-cap set has no File lines"
        );
        assert!(file_set.lines.iter().all(|l| matches!(
            l.kind,
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::SizeCapExceeded
            }
        )));
        assert!(
            file_set.content_digest_contribution().is_empty(),
            "over-cap identity must fall back to path text"
        );
    }

    /// Just at the member-count cap → normal hashing (not over-cap). A cap of
    /// `N` admits exactly `N` members; only `> N` trips it.
    #[tokio::test]
    async fn member_count_exactly_at_cap_hashes_normally() {
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///Users/alice/a.txt\nfile:///Users/alice/b.txt",
        )]);
        let file_set = build_entry_file_set(&snap, &FakeIngestByName, caps(0, 2))
            .await
            .expect("file-class snapshot yields a manifest");
        assert_eq!(file_set.file_lines().count(), 2);
        assert!(!file_set.content_digest_contribution().is_empty());
    }

    /// Total-bytes cap measured from real file metadata. Two files summing
    /// above the cap → every file line `Excluded { SizeCapExceeded }`, no
    /// hashing.
    #[cfg(unix)]
    #[tokio::test]
    async fn total_bytes_cap_excludes_all_file_lines_without_hashing() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let mut uri_list = String::new();
        for (name, size) in [("a.bin", 400usize), ("b.bin", 400usize)] {
            let path = dir.path().join(name);
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&vec![0u8; size]).unwrap();
            uri_list.push_str(&format!("file://{}\n", path.display()));
        }
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);
        // 800 bytes total, cap = 500.
        let file_set = build_entry_file_set(&snap, &PanicOnHash, caps(500, 0))
            .await
            .expect("file-class snapshot yields a manifest");
        assert!(file_set.lines.iter().all(|l| matches!(
            l.kind,
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::SizeCapExceeded
            }
        )));
        assert!(file_set.content_digest_contribution().is_empty());
    }

    /// Total under the byte cap → normal hashing, real File lines.
    #[cfg(unix)]
    #[tokio::test]
    async fn total_bytes_within_cap_hashes_normally() {
        use std::io::Write;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("small.bin");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&[0u8; 100])
            .unwrap();
        let uri_list = format!("file://{}\n", path.display());
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            uri_list.as_bytes(),
        )]);
        let file_set = build_entry_file_set(&snap, &FakeIngestByName, caps(500, 0))
            .await
            .expect("file-class snapshot yields a manifest");
        assert_eq!(file_set.file_lines().count(), 1);
        assert!(!file_set.content_digest_contribution().is_empty());
    }

    /// An unreadable member leaves the byte-cap verdict `false` (can't confirm
    /// over-budget), so hashing still runs and that member becomes
    /// `IngestFailed` — not `SizeCapExceeded`.
    #[cfg(unix)]
    #[tokio::test]
    async fn total_bytes_cap_ignores_unmeasurable_member() {
        let snap = snapshot_with(vec![rep(
            "public.file-url",
            Some("text/uri-list"),
            b"file:///nonexistent/uc-cap-test/gone.bin",
        )]);
        // hash_path fails for the missing file → IngestFailed, and the cap
        // pre-check must NOT have short-circuited to SizeCapExceeded.
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
        let file_set = build_entry_file_set(&snap, &AlwaysFails, caps(1, 0))
            .await
            .expect("file-class snapshot yields a manifest");
        assert!(
            file_set.lines.iter().all(|l| matches!(
                l.kind,
                EntryFileSetLineKind::Excluded {
                    reason: EntryFileSetExcludeReason::IngestFailed
                }
            )),
            "unmeasurable member must be IngestFailed, not SizeCapExceeded"
        );
    }

    /// LocalFile-rep shape (macOS Finder) also honours the member-count cap.
    #[tokio::test]
    async fn local_file_reps_honour_member_count_cap() {
        let reps = vec![
            ObservedClipboardRepresentation::new_local_file(
                RepresentationId::new(),
                FormatId::from("public.file-url"),
                None,
                std::path::PathBuf::from("/Users/alice/x.txt"),
                10,
            ),
            ObservedClipboardRepresentation::new_local_file(
                RepresentationId::new(),
                FormatId::from("public.file-url"),
                None,
                std::path::PathBuf::from("/Users/alice/y.txt"),
                10,
            ),
        ];
        let file_set = build_entry_file_set(&snapshot_with(reps), &PanicOnHash, caps(0, 1))
            .await
            .expect("file-class snapshot yields a manifest");
        assert!(file_set.lines.iter().all(|l| matches!(
            l.kind,
            EntryFileSetLineKind::Excluded {
                reason: EntryFileSetExcludeReason::SizeCapExceeded
            }
        )));
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
