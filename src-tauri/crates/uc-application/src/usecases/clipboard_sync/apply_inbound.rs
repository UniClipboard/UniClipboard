//! `ApplyInboundClipboardUseCase` — daemon-side inbound clipboard
//! processing pipeline (Slice 2 Phase 3 · T4).
//!
//! ## Flow
//!
//! 1. **Dedup short-circuit**: if `content_hash` already exists in the
//!    local `clipboard_event` table, return `DuplicateSkipped`. Skips
//!    persist + OS-clipboard write — Phase 3 acceptance #4 guarantees a
//!    repeat copy from a peer doesn't double-write the user's clipboard.
//! 2. **Envelope decode**: V3 → `SystemClipboardSnapshot`. Decode failure
//!    is non-fatal (`DecodeFailed` outcome) — corrupted payloads from a
//!    misbehaving peer don't crash the daemon's ingest loop.
//! 3. **Capture pipeline**: reuse `CaptureClipboardUseCase` with origin
//!    `RemotePush` so the entry, event, normalised representations,
//!    cache, spool, and (optional) search index all match the local
//!    capture path's schema (D5 decision).
//! 4. **OS clipboard write**: via `ClipboardWriteCoordinator` with
//!    `RemotePush` intent — registers a 60s hash guard + one-shot
//!    next-origin override so the daemon's own clipboard watcher doesn't
//!    re-dispatch the just-written content (write-back loop defence).
//!    The **full** snapshot (every V3-decoded representation) is handed
//!    to the coordinator; the platform layer internally decides whether
//!    to atomically write multiple formats (Windows today) or to narrow
//!    to the paste-priority rep via `SelectRepresentationPolicyV1`
//!    (macOS / Linux fallback today).
//!
//! Step ordering (3 → 4) matters: capture commits the event before the
//! OS write fires, so when the watcher consumes the origin guard it
//! already sees the persisted row.
//!
//! ## Testability
//!
//! `CaptureClipboardUseCase` and `ClipboardWriteCoordinator` are
//! concrete structs with 7+2 port dependencies. Holding them as
//! `Arc<dyn Trait>` via two thin internal abstractions
//! ([`InboundCapture`] / [`InboundWrite`]) keeps the use case mockable
//! without requiring tests to construct full real implementations.
//! Production wires the concrete types via the blanket impls below.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use bytes::Bytes;
use thiserror::Error;
use tracing::{debug, error, info, instrument, warn};
use url::Url;

use uc_core::ids::{DeviceId, EntryId, FormatId, RepresentationId};
use uc_core::ports::ClipboardEntryRepositoryPort;
use uc_core::{
    ClipboardChangeOrigin, MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot,
};

use crate::clipboard_capture::CaptureClipboardUseCase;
use crate::clipboard_write::{ClipboardWriteCoordinator, ClipboardWriteIntent};
use crate::facade::blob_transfer::{
    BlobTransferFacade, FetchBlobCommand, FetchBlobResult, FetchTransferContext,
};
use crate::usecases::clipboard_sync::payload_codec::{
    decode_v3_bytes_to_snapshot_and_blob_refs, V3BlobRef,
};

/// Caller-supplied input mapped from the facade's public `InboundNotice`.
///
/// Keeping this struct separate from `crate::facade::clipboard::InboundNotice`
/// avoids the use case importing from the facade layer (§11.4 keeps the
/// arrow `facade → use case`, never the reverse).
#[derive(Debug, Clone)]
pub struct ApplyInboundInput {
    pub from_device: DeviceId,
    pub content_hash: String,
    pub plaintext: Bytes,
}

/// Result of one `execute` call. Daemon's worker maps each variant to a
/// distinct telemetry path (WS event for `Applied`, debug log for
/// `DuplicateSkipped`, warn log for `DecodeFailed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyOutcome {
    /// New content — persisted + OS clipboard written. WS event fires.
    Applied { entry_id: EntryId },
    /// `content_hash` was already present in the local DB. No persist,
    /// no OS write, no WS event.
    DuplicateSkipped {
        content_hash: String,
        existing_entry_id: EntryId,
    },
    /// V3 envelope was malformed. Frame dropped silently except for a
    /// warning log; receiver loop keeps running.
    DecodeFailed { reason: String },
}

#[derive(Debug, Error)]
pub enum ApplyInboundError {
    #[error("dedup query failed: {0}")]
    DedupQuery(String),
    #[error("capture pipeline failed: {0}")]
    Capture(String),
    #[error("clipboard write failed: {0}")]
    WriteCoordinator(String),
    #[error("internal: {0}")]
    Internal(String),
}

/// Internal abstraction over the persistence pipeline. Production uses
/// the blanket impl on `CaptureClipboardUseCase`; tests use a `mockall`
/// mock.
#[async_trait]
pub trait InboundCapture: Send + Sync {
    /// Persist `snapshot` as a `RemotePush`-origin entry. Returns
    /// `Ok(Some(EntryId))` on success, `Ok(None)` only in the legitimate
    /// "no supported representation" / `LocalRestore` short-circuit cases
    /// (which `RemotePush` never hits in practice — daemon treats `None`
    /// as `ApplyInboundError::Internal`).
    async fn capture(&self, snapshot: SystemClipboardSnapshot) -> Result<Option<EntryId>>;
}

#[async_trait]
impl InboundCapture for CaptureClipboardUseCase {
    async fn capture(&self, snapshot: SystemClipboardSnapshot) -> Result<Option<EntryId>> {
        self.execute_with_origin(snapshot, ClipboardChangeOrigin::RemotePush, None)
            .await
    }
}

/// Internal abstraction over the OS clipboard write boundary. Production
/// uses the blanket impl on `ClipboardWriteCoordinator`; tests mock it.
#[async_trait]
pub trait InboundWrite: Send + Sync {
    /// Write `snapshot` to the OS clipboard with the `RemotePush`
    /// intent (registers the appropriate hash guards + next-origin
    /// override per the coordinator's contract).
    async fn write(&self, snapshot: SystemClipboardSnapshot) -> Result<()>;
}

#[async_trait]
impl InboundWrite for ClipboardWriteCoordinator {
    async fn write(&self, snapshot: SystemClipboardSnapshot) -> Result<()> {
        ClipboardWriteCoordinator::write(self, snapshot, ClipboardWriteIntent::RemotePush).await
    }
}

/// 入站 blob 本地化抽象。
///
/// 生产环境会把每个 blob 拉到本机缓存目录,再把 file-list 表示改写为本机路径;
/// 测试用 mock 固定调用顺序,避免触碰真实文件系统。
#[async_trait]
pub trait InboundBlobMaterializer: Send + Sync {
    async fn materialize(
        &self,
        from_device: DeviceId,
        snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
    ) -> Result<SystemClipboardSnapshot>;
}

#[async_trait]
pub trait InboundBlobFetcher: Send + Sync {
    async fn fetch_blob(&self, command: FetchBlobCommand) -> Result<FetchBlobResult>;
}

#[async_trait]
impl InboundBlobFetcher for BlobTransferFacade {
    async fn fetch_blob(&self, command: FetchBlobCommand) -> Result<FetchBlobResult> {
        BlobTransferFacade::fetch_blob(self, command)
            .await
            .map_err(|e| anyhow!(e.to_string()))
    }
}

pub struct FileCacheBlobMaterializer {
    fetcher: Arc<dyn InboundBlobFetcher>,
    cache_dir: PathBuf,
}

impl FileCacheBlobMaterializer {
    pub fn new(fetcher: Arc<dyn InboundBlobFetcher>, cache_dir: PathBuf) -> Self {
        Self { fetcher, cache_dir }
    }
}

#[async_trait]
impl InboundBlobMaterializer for FileCacheBlobMaterializer {
    async fn materialize(
        &self,
        from_device: DeviceId,
        mut snapshot: SystemClipboardSnapshot,
        blob_refs: Vec<V3BlobRef>,
    ) -> Result<SystemClipboardSnapshot> {
        if blob_refs.is_empty() {
            return Ok(snapshot);
        }

        // Split blob refs by destination:
        //   - `representation_index = Some(i)`: bytes belong to envelope rep i
        //     (image / large binary path). Fetched bytes are written back into
        //     `snapshot.representations[i]` so the rep round-trips with full
        //     content; receiver does NOT spill these to disk.
        //   - `representation_index = None`: free-standing file (legacy
        //     file-URI path). Fetched bytes go to cache_dir, file-list rep is
        //     rewritten with local `file://` URIs.
        let (rep_refs, file_refs): (Vec<V3BlobRef>, Vec<V3BlobRef>) = blob_refs
            .into_iter()
            .partition(|r| r.representation_index.is_some());

        // 1. Hydrate representation-bound blobs back into the snapshot.
        for blob_ref in rep_refs {
            let entry_id = blob_ref.entry_id.clone();
            let advertised_size = blob_ref.size_bytes;
            let idx = blob_ref
                .representation_index
                .expect("partition guarantees Some");
            debug!(
                entry_id = %entry_id,
                size_bytes = advertised_size,
                representation_index = idx,
                mime = blob_ref.mime.as_deref().unwrap_or(""),
                "materialize: fetching representation-bound blob"
            );

            let fetched = self
                .fetcher
                .fetch_blob(FetchBlobCommand {
                    ticket: blob_ref.ticket,
                    entry_id: blob_ref.entry_id.clone(),
                })
                .await
                .map_err(|e| {
                    warn!(
                        entry_id = %entry_id,
                        size_bytes = advertised_size,
                        representation_index = idx,
                        error = %e,
                        "materialize: representation-bound blob fetch failed"
                    );
                    e
                })?;

            let usize_idx = idx as usize;
            let rep_count = snapshot.representations.len();
            let rep = snapshot.representations.get_mut(usize_idx).ok_or_else(|| {
                anyhow!(
                    "materialize: representation_index {idx} out of bounds (snapshot has {rep_count} reps)"
                )
            })?;
            let fetched_len = fetched.plaintext.len();
            rep.bytes = fetched.plaintext.to_vec();
            info!(
                entry_id = %entry_id,
                representation_index = idx,
                bytes_written = fetched_len,
                "materialize: blob inlined back into representation"
            );
        }

        if file_refs.is_empty() {
            return Ok(snapshot);
        }

        // 2. Free-standing files: existing cache_dir + file-list rewrite path.
        let mut local_paths = Vec::with_capacity(file_refs.len());
        let mut used_names = HashSet::new();
        let blob_ref_total = file_refs.len();

        for (idx, blob_ref) in file_refs.into_iter().enumerate() {
            let entry_id = blob_ref.entry_id.clone();
            let advertised_size = blob_ref.size_bytes;
            let declared_name = blob_ref.filename.clone();
            debug!(
                idx,
                total = blob_ref_total,
                entry_id = %entry_id,
                size_bytes = advertised_size,
                filename = declared_name.as_deref().unwrap_or(""),
                "materialize: fetching blob"
            );

            // transfer_id 直接复用 entry_id —— 一个 entry 对应一次 blob 拉取,
            // 前端 useTransferProgress 用它定位 UI(每个 entry 一个进度条)。
            let transfer_context = FetchTransferContext {
                transfer_id: blob_ref.entry_id.as_ref().to_string(),
                peer_id: from_device.as_str().to_string(),
                total_bytes: Some(advertised_size),
            };
            let fetched = self
                .fetcher
                .fetch_blob(FetchBlobCommand {
                    ticket: blob_ref.ticket,
                    entry_id: blob_ref.entry_id.clone(),
                    transfer_context: Some(transfer_context),
                })
                .await
                .map_err(|e| {
                    warn!(
                        idx,
                        total = blob_ref_total,
                        entry_id = %entry_id,
                        size_bytes = advertised_size,
                        error = %e,
                        "materialize: blob fetch failed"
                    );
                    e
                })?;

            let entry_dir = self
                .cache_dir
                .join("iroh-blobs")
                .join(sanitize_path_segment(blob_ref.entry_id.as_ref()));
            tokio::fs::create_dir_all(&entry_dir).await?;

            let filename = unique_filename(blob_ref.filename.as_deref(), idx, &mut used_names);
            let path = entry_dir.join(filename);
            let fetched_len = fetched.plaintext.len();
            tokio::fs::write(&path, fetched.plaintext).await?;
            info!(
                idx,
                total = blob_ref_total,
                entry_id = %entry_id,
                bytes_written = fetched_len,
                path = %path.display(),
                "materialize: blob cached to local path"
            );
            local_paths.push(path);
        }

        let uri_list = local_file_uri_list(&local_paths)?;
        let mut rewritten_rep_count = 0usize;
        for rep in &mut snapshot.representations {
            if is_file_list_representation(rep) {
                rep.bytes = uri_list.as_bytes().to_vec();
                rewritten_rep_count += 1;
            }
        }

        if rewritten_rep_count == 0 {
            snapshot
                .representations
                .push(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("files"),
                    Some(MimeType("text/uri-list".to_string())),
                    uri_list.into_bytes(),
                ));
            info!(
                local_path_count = local_paths.len(),
                "materialize: appended synthetic files rep (no file-list rep in payload)"
            );
        } else {
            info!(
                rewritten_rep_count,
                local_path_count = local_paths.len(),
                "materialize: rewrote file-list reps with local paths"
            );
        }

        Ok(snapshot)
    }
}

pub struct ApplyInboundClipboardUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    capture: Arc<dyn InboundCapture>,
    write: Arc<dyn InboundWrite>,
    blob_materializer: Option<Arc<dyn InboundBlobMaterializer>>,
}

fn is_file_list_representation(rep: &ObservedClipboardRepresentation) -> bool {
    rep.mime
        .as_ref()
        .map(|mime| {
            mime.as_str().eq_ignore_ascii_case("text/uri-list")
                || mime.as_str().eq_ignore_ascii_case("file/uri-list")
        })
        .unwrap_or(false)
        || rep.format_id.eq_ignore_ascii_case("files")
        || rep.format_id.eq_ignore_ascii_case("public.file-url")
}

fn unique_filename(
    candidate: Option<&str>,
    idx: usize,
    used_names: &mut HashSet<String>,
) -> String {
    let base = candidate
        .and_then(|name| {
            std::path::Path::new(name)
                .file_name()
                .and_then(|n| n.to_str())
        })
        .map(sanitize_path_segment)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| format!("blob-{idx}"));

    if used_names.insert(base.clone()) {
        return base;
    }

    let mut counter = 1usize;
    loop {
        let candidate = format!("{counter}-{base}");
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
        counter += 1;
    }
}

fn sanitize_path_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '\0' => '_',
            ch if ch.is_control() => '_',
            ch => ch,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_string()
}

fn local_file_uri_list(paths: &[PathBuf]) -> Result<String> {
    let mut out = String::new();
    for path in paths {
        let url = Url::from_file_path(path).map_err(|_| {
            anyhow!(
                "failed to convert cache path to file URL: {}",
                path.display()
            )
        })?;
        out.push_str(url.as_str());
        out.push('\n');
    }
    Ok(out)
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
        }
    }

    pub fn with_blob_materializer(
        mut self,
        blob_materializer: Arc<dyn InboundBlobMaterializer>,
    ) -> Self {
        self.blob_materializer = Some(blob_materializer);
        self
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

        let snapshot = match (blob_refs.is_empty(), &self.blob_materializer) {
            (true, _) => snapshot,
            (false, Some(materializer)) => {
                let count = blob_refs.len();
                let snapshot = materializer
                    .materialize(input.from_device.clone(), snapshot, blob_refs)
                    .await
                    .map_err(|e| {
                        warn!(error = %e, blob_ref_count = count, "inbound: blob materialize failed");
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
                return Ok(ApplyOutcome::DecodeFailed { reason });
            }
        };

        // 3. Persist via the same capture pipeline local copies use
        // (D5: same schema). Cloning the snapshot lets us keep one for
        // the OS write below; capture takes ownership of the original.
        let snapshot_for_write = snapshot.clone();
        let entry_id = self
            .capture
            .capture(snapshot)
            .await
            .map_err(|e| ApplyInboundError::Capture(e.to_string()))?
            .ok_or_else(|| {
                ApplyInboundError::Internal(
                    "capture returned None for RemotePush origin (unexpected)".to_string(),
                )
            })?;

        // 4. Write OS clipboard with RemotePush guard. Order matters —
        // capture must complete first so the watcher's origin lookup
        // sees the persisted row even if it fires immediately.
        //
        // 送入 full snapshot（不 narrow）：platform 层内部按能力差异消化多 rep。
        // - Windows：`write_snapshot_multi_windows` 原子写入 CF_UNICODETEXT + CF_HTML 等
        // - macOS / Linux：`write_snapshot_multi` 的降级分支用 `SelectRepresentationPolicyV1`
        //   选 paste-priority rep 后走单 rep 快路径（行为与上游 `narrow_to_primary` 等价）
        //
        // 背景：quick `260423-9do` 交付了平台层的多 rep 写入能力，但此前应用层仍在
        // narrow，导致主流量走单 rep 快路径、新能力 0 触发。本改动把 full snapshot 直送
        // platform 层，由 platform 根据自身 OS 能力内部分流。详见
        // `.planning/quick/260423-a3b-windows-rep-apply-inbound-narrow/`。
        debug!(entry_id = %entry_id, "inbound: entry persisted, writing OS clipboard");

        self.write.write(snapshot_for_write).await.map_err(|e| {
            error!(error = %e, entry_id = %entry_id, "inbound: OS clipboard write failed after capture");
            ApplyInboundError::WriteCoordinator(e.to_string())
        })?;

        info!(entry_id = %entry_id, "inbound clipboard applied");
        Ok(ApplyOutcome::Applied { entry_id })
    }
}

/// Compact summary of the snapshot's representations for tracing.
/// Format: `format_id[@mime]:bytes, ...` — always safe to log because
/// `format_id` / `mime` / byte counts are metadata, never user payload.
fn format_rep_summary(snapshot: &SystemClipboardSnapshot) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecases::clipboard_sync::payload_codec::{
        encode_snapshot_to_v3_bytes, encode_snapshot_with_blob_refs_to_v3_bytes, V3BlobRef,
    };
    use mockall::predicate::*;

    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::ports::blob::{BlobDigest, BlobTicket, PlaintextHash};
    use uc_core::ports::PeerAddressError;
    use uc_core::{MimeType, ObservedClipboardRepresentation};

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
            async fn capture(&self, snapshot: SystemClipboardSnapshot) -> Result<Option<EntryId>>;
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
            .returning(|_| Ok(Some(EntryId::from("entry-new"))));

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
        capture.expect_capture().times(1).returning(|_| Ok(None));

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

    /// Verdict 5 — write coordinator failure surfaces as
    /// `WriteCoordinator` error. Capture has already committed (the
    /// entry stays in DB; manual cleanup is the daemon operator's job).
    /// Pin this trade-off so a future refactor doesn't silently start
    /// rolling back persistence on write failure.
    #[tokio::test]
    async fn write_failure_surfaces_after_capture_commits() {
        let (input, _) = fixture_input("write-will-fail");

        let mut repo = MockEntryRepo::new();
        repo.expect_find_entry_id_by_snapshot_hash()
            .times(1)
            .returning(|_| Ok(None));

        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .times(1)
            .returning(|_| Ok(Some(EntryId::from("entry-committed"))));

        let mut write = MockWrite::new();
        write
            .expect_write()
            .times(1)
            .returning(|_| Err(anyhow::anyhow!("OS clipboard locked")));

        let uc = build(repo, capture, write);
        let err = uc
            .execute(input)
            .await
            .expect_err("write failure must surface");
        match err {
            ApplyInboundError::WriteCoordinator(msg) => {
                assert!(
                    msg.contains("OS clipboard locked"),
                    "underlying error should propagate, got: {msg}"
                );
            }
            other => panic!("expected WriteCoordinator, got {other:?}"),
        }
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
            .withf(move |_from_device, snapshot, refs| {
                snapshot.representations[0].bytes == b"file:///sender/original.txt\n"
                    && refs == &vec![blob_ref.clone()]
            })
            .returning(|_from_device, mut snapshot, _| {
                snapshot.representations[0].bytes = b"file:///local/cache/original.txt\n".to_vec();
                Ok(snapshot)
            });

        let assert_local_file = |snapshot: &SystemClipboardSnapshot| {
            snapshot.representations[0].bytes == b"file:///local/cache/original.txt\n"
        };
        let mut capture = MockCapture::new();
        capture
            .expect_capture()
            .withf(move |snapshot| assert_local_file(snapshot))
            .times(1)
            .returning(|_| Ok(Some(EntryId::from("entry-new"))));

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
            .expect_fetch_blob()
            .times(1)
            .withf(move |command| command.entry_id == entry_id && command.ticket == ticket)
            .returning(|command| {
                Ok(crate::facade::blob_transfer::FetchBlobResult {
                    plaintext: Bytes::from_static(b"hello world"),
                    entry_id: command.entry_id,
                    plaintext_hash: PlaintextHash::from_bytes([0; 32]),
                    digest: BlobDigest::from_bytes([1; 32]),
                })
            });

        let materializer =
            FileCacheBlobMaterializer::new(Arc::new(fetcher), cache_dir.path().to_path_buf());
        let rewritten = materializer
            .materialize(DeviceId::new("peer-x"), snapshot, vec![blob_ref])
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
            .materialize(snapshot, vec![blob_ref])
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
            .materialize(snapshot, vec![blob_ref])
            .await
            .expect_err("out-of-bounds index should fail");
        assert!(
            err.to_string().contains("out of bounds"),
            "error must mention out-of-bounds context: {err}"
        );
    }
}
