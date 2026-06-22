//! Reconstruct a [`SystemClipboardSnapshot`] from a persisted clipboard entry.
//!
//! Shared between two callers:
//! - [`RestoreClipboardSelectionUseCase`](crate::usecases::clipboard_restore::RestoreClipboardSelectionUseCase),
//!   which writes the snapshot back to the local system clipboard.
//! - The resend path (Stage 1a), which forwards the snapshot to peers through
//!   the outbound dispatch pipeline.
//!
//! Both callers need the same logical reconstruction:
//! - look up entry + selection + reps,
//! - resolve each candidate rep's payload via [`ClipboardPayloadResolverPort`]
//!   (Inline direct / BlobRef via [`BlobReaderPort`] / Staged|Processing via
//!   cache+spool),
//! - pack the resolved bytes into a fresh [`SystemClipboardSnapshot`].
//!
//! Caller-specific concerns stay at the caller:
//! - restore checks [`ClipboardIntegrationMode`](uc_core::clipboard::ClipboardIntegrationMode)
//!   before writing,
//! - resend adds target filtering / fan-out on top.
//!
//! ## Side effect
//!
//! When the paste representation resolves to [`PayloadResolveError::Orphaned`]
//! (cache + spool double miss) the helper demotes the row to
//! [`PayloadAvailability::Lost`] before returning. That way subsequent
//! reconstruction attempts get a stable `Lost` outcome instead of repeatedly
//! producing transient errors. The demotion is best-effort: a DB failure is
//! logged but does not propagate (the original resolve error is what the
//! caller actually returns).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use tracing::{debug, info, warn};

use uc_core::{
    blob::ports::BlobReaderPort,
    clipboard::{
        is_file_mime_or_format, ObservedClipboardRepresentation, PayloadAvailability,
        PersistedClipboardRepresentation, SystemClipboardSnapshot,
    },
    ids::{EntryId, EventId, RepresentationId},
    ports::{
        clipboard::{
            ClipboardPayloadResolverPort, GetClipboardEntryPort, GetRepresentationPort,
            PayloadResolveError, ProcessingUpdateOutcome, ResolvedClipboardPayload,
            UpdateRepresentationProcessingResultPort,
        },
        ClipboardSelectionRepositoryPort,
    },
    BlobId,
};

use crate::usecases::clipboard_restore::file_snapshot::{build_file_snapshot, build_path_list};

/// Typed errors returned by [`reconstruct_snapshot_from_entry`].
///
/// Variants carry enough domain context for callers to translate into their
/// own application errors (e.g. `ClipboardRestoreError::PayloadUnavailable`,
/// `ResendEntryError::EntryNotResendable`) without leaking helper-internal
/// details. The [`PasteRepUnavailable`](Self::PasteRepUnavailable) variant
/// transparently wraps [`PayloadResolveError`] so existing `downcast_ref`
/// sites continue to work when the error is re-wrapped into `anyhow::Error`
/// by callers.
#[derive(Debug, Error)]
pub(crate) enum BuildSnapshotError {
    #[error("Entry not found: {entry_id}")]
    EntryNotFound { entry_id: EntryId },

    #[error("Selection not found for entry {entry_id}")]
    SelectionNotFound { entry_id: EntryId },

    #[error("Representation {rep_id} not found for event {event_id}")]
    PasteRepNotFound {
        event_id: EventId,
        rep_id: RepresentationId,
    },

    /// The paste representation cannot be resolved into bytes. Wraps the
    /// typed [`PayloadResolveError`] (Lost / Orphaned / Integrity) so callers
    /// that downcast through `anyhow::Error::downcast_ref::<PayloadResolveError>`
    /// keep working unchanged.
    #[error(transparent)]
    PasteRepUnavailable(#[from] PayloadResolveError),

    /// Blob fetch failed for the paste representation. Distinct from
    /// `PasteRepUnavailable`: the resolver succeeded with a `BlobRef` but the
    /// downstream blob store could not deliver the bytes.
    #[error("Failed to fetch paste representation blob {blob_id}: {reason}")]
    PasteRepBlobFetchFailed { blob_id: BlobId, reason: String },

    #[error("Failed to parse file URI for entry {entry_id} (rep {rep_id}): {reason}")]
    InvalidFileUri {
        entry_id: EntryId,
        rep_id: RepresentationId,
        reason: String,
    },

    #[error("No valid file paths found in entry {entry_id}")]
    NoFilePaths { entry_id: EntryId },

    /// Defensive guard: every candidate rep was skipped. The paste-rep
    /// failure paths above should normally cover this, but a fully empty
    /// candidate list still surfaces here rather than silently returning an
    /// empty snapshot.
    #[error("No restorable representations after packing for entry {entry_id}")]
    NoRestorableRepresentations { entry_id: EntryId },

    /// Underlying repository / I/O failure (e.g. `get_entry`, `get_selection`,
    /// or `get_representation` returned an error rather than `None`).
    #[error("Repository error: {0}")]
    Repository(#[source] anyhow::Error),
}

/// Rebuild a [`SystemClipboardSnapshot`] from a persisted clipboard entry.
///
/// Resolution order:
/// 1. Look up entry + selection.
/// 2. Collect candidate rep ids in `paste / primary / preview / secondary`
///    order and dedup, then load each [`PersistedClipboardRepresentation`].
/// 3. If `paste_rep` is a file rep ([`is_file_mime_or_format`]), take the
///    file branch: resolve the URI list, parse to local paths, validate
///    on-disk existence, and emit a fresh `text/uri-list` snapshot.
/// 4. Otherwise, pack every non-file candidate via
///    [`ClipboardPayloadResolverPort`] (Inline direct / BlobRef via
///    [`BlobReaderPort`] / Staged|Processing via cache+spool). Secondary
///    reps that fail to resolve are skipped with a warning; failure on
///    `paste_rep` itself is fatal.
///
/// See the module docs for the orphan-demotion side effect.
pub(crate) async fn reconstruct_snapshot_from_entry(
    entry_repo: &dyn GetClipboardEntryPort,
    selection_repo: &dyn ClipboardSelectionRepositoryPort,
    representation_repo: &dyn GetRepresentationPort,
    rep_processing_repo: &dyn UpdateRepresentationProcessingResultPort,
    payload_resolver: &dyn ClipboardPayloadResolverPort,
    blob_store: &dyn BlobReaderPort,
    entry_id: &EntryId,
) -> Result<SystemClipboardSnapshot, BuildSnapshotError> {
    debug!(entry_id = %entry_id, "snapshot_from_entry.reconstruct start");

    let entry = entry_repo
        .get_entry(entry_id)
        .await
        .map_err(|e| BuildSnapshotError::Repository(e.into()))?
        .ok_or_else(|| BuildSnapshotError::EntryNotFound {
            entry_id: entry_id.clone(),
        })?;

    let selection = selection_repo
        .get_selection(entry_id)
        .await
        .map_err(BuildSnapshotError::Repository)?
        .ok_or_else(|| BuildSnapshotError::SelectionNotFound {
            entry_id: entry_id.clone(),
        })?;

    // 候选 rep 收集顺序：paste_rep 居首（保留"目标应用最优先粘贴"的语义），
    // 然后是 primary / preview / secondary。整体去重后传给后续打包逻辑。
    let mut candidate_ids = Vec::new();
    candidate_ids.push(selection.selection.paste_rep_id.clone());
    candidate_ids.push(selection.selection.primary_rep_id.clone());
    candidate_ids.push(selection.selection.preview_rep_id.clone());
    candidate_ids.extend(selection.selection.secondary_rep_ids.clone());

    let mut seen = HashSet::new();
    candidate_ids.retain(|rep_id| seen.insert(rep_id.clone()));

    let mut candidates = Vec::new();
    for rep_id in &candidate_ids {
        let rep = representation_repo
            .get_representation(&entry.event_id, rep_id)
            .await
            .map_err(|e| BuildSnapshotError::Repository(e.into()))?;
        if let Some(rep) = rep {
            candidates.push(rep);
        } else if *rep_id == selection.selection.paste_rep_id {
            return Err(BuildSnapshotError::PasteRepNotFound {
                event_id: entry.event_id.clone(),
                rep_id: rep_id.clone(),
            });
        }
    }

    // 文件分支：paste_rep 是文件类型（CF_HDROP / NSPasteboardTypeFileURL）时，
    // 走专用的 file snapshot 路径。文件 rep 的语义与文本/图像表示不可混写在
    // 同一个 NSPasteboardItem / clipboard item 中，平台层目前也仅支持文件单独
    // 写入；同时 build_file_snapshot 会校验本地文件存在性。
    let paste_rep = candidates
        .iter()
        .find(|rep| rep.id == selection.selection.paste_rep_id)
        .ok_or_else(|| BuildSnapshotError::PasteRepNotFound {
            event_id: entry.event_id.clone(),
            rep_id: selection.selection.paste_rep_id.clone(),
        })?;

    if is_file_rep(paste_rep) {
        debug!(
            entry_id = %entry_id,
            paste_rep_id = %paste_rep.id,
            "snapshot_from_entry.reconstruct: detected file entry, using file branch"
        );
        return build_file_branch(
            payload_resolver,
            blob_store,
            rep_processing_repo,
            entry_id,
            paste_rep,
        )
        .await;
    }

    // 非文件分支：把所有非文件候选 rep 都打包成多 rep snapshot，paste_rep 居首。
    // 每条 rep 通过 `ClipboardPayloadResolverPort` 取字节，由 resolver 负责按
    // payload_state 路由（Inline 直读已解密 inline_data / BlobReady 走 blob_store /
    // Staged|Processing 走 cache+spool）。
    //
    // 注意不能直接读 `rep.inline_data`：当 rep 的明文体积超过 inline_threshold
    // （默认 16KB）时，normalizer 会把它标成 Staged 并只在 inline_data 里留
    // 500 字符的 UI preview，真实字节走 spool/blob 异步物化。直接读 inline_data
    // 会拿到截断版，写到 NSPasteboard 上的 RTF / HTML 解析失败 → 粘出空。
    let mut representations = Vec::with_capacity(candidates.len());
    let mut paste_first = true;
    let mut packed_rep_ids: Vec<RepresentationId> = Vec::new();
    for rep in &candidates {
        if is_file_rep(rep) {
            debug!(
                entry_id = %entry_id,
                rep_id = %rep.id,
                format_id = %rep.format_id,
                "snapshot_from_entry.reconstruct: skipping file rep when paste_rep is non-file"
            );
            continue;
        }

        let is_paste_rep = rep.id == paste_rep.id;
        let bytes = match payload_resolver.resolve(rep).await {
            Ok(ResolvedClipboardPayload::Inline { bytes, .. }) => bytes,
            Ok(ResolvedClipboardPayload::BlobRef { blob_id, .. }) => {
                match blob_store.get(&blob_id).await {
                    Ok(plaintext) => plaintext,
                    Err(err) if is_paste_rep => {
                        return Err(BuildSnapshotError::PasteRepBlobFetchFailed {
                            blob_id,
                            reason: err.to_string(),
                        });
                    }
                    Err(err) => {
                        warn!(
                            entry_id = %entry_id,
                            rep_id = %rep.id,
                            blob_id = %blob_id,
                            error = %err,
                            "snapshot_from_entry.reconstruct: skipping rep, blob fetch failed"
                        );
                        continue;
                    }
                }
            }
            Err(resolver_err) if is_paste_rep => {
                // Active demotion: orphaned paste-rep (cache+spool double
                // miss) gets demoted to `Lost` so the next attempt sees a
                // stable `Lost` error instead of churning through the same
                // transient failure path.
                if let PayloadResolveError::Orphaned { rep_id, state } = &resolver_err {
                    demote_orphaned_to_lost(rep_processing_repo, rep_id, state).await;
                }
                return Err(BuildSnapshotError::PasteRepUnavailable(resolver_err));
            }
            Err(err) => {
                warn!(
                    entry_id = %entry_id,
                    rep_id = %rep.id,
                    format_id = %rep.format_id,
                    payload_state = ?rep.payload_state,
                    error = %err,
                    "snapshot_from_entry.reconstruct: skipping rep, resolver failed (likely Staged without cache/spool bytes)"
                );
                continue;
            }
        };

        let observed = ObservedClipboardRepresentation::new(
            rep.id.clone(),
            rep.format_id.clone(),
            rep.mime_type.clone(),
            bytes,
        );

        packed_rep_ids.push(rep.id.clone());
        if paste_first {
            representations.insert(0, observed);
            paste_first = false;
        } else {
            representations.push(observed);
        }
    }

    if representations.is_empty() {
        return Err(BuildSnapshotError::NoRestorableRepresentations {
            entry_id: entry_id.clone(),
        });
    }

    debug!(
        entry_id = %entry_id,
        event_id = %entry.event_id,
        paste_rep_id = %paste_rep.id,
        packed_rep_count = representations.len(),
        packed_rep_ids = ?packed_rep_ids,
        total_size_bytes = representations.iter().map(|r| r.size_bytes() as usize).sum::<usize>(),
        "snapshot_from_entry.reconstruct packed representations"
    );

    Ok(SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations,
        file_content_digests: Vec::new(),
    })
}

fn is_file_rep(rep: &PersistedClipboardRepresentation) -> bool {
    is_file_mime_or_format(rep.mime_type.as_ref(), &rep.format_id)
}

async fn build_file_branch(
    payload_resolver: &dyn ClipboardPayloadResolverPort,
    blob_store: &dyn BlobReaderPort,
    rep_processing_repo: &dyn UpdateRepresentationProcessingResultPort,
    entry_id: &EntryId,
    rep: &PersistedClipboardRepresentation,
) -> Result<SystemClipboardSnapshot, BuildSnapshotError> {
    // 与非文件分支同样走 payload_resolver：file URI list 在文件较多时同样会
    // 触发 inline_threshold，rep.inline_data 只剩 500-char 预览截断版，直接
    // clone 会拿到不完整的 URI 列表 → 文件路径解析丢失。resolver 会按
    // payload_state 正确路由（Inline / BlobReady / Staged|Processing）。
    let bytes = match payload_resolver.resolve(rep).await {
        Ok(ResolvedClipboardPayload::Inline { bytes, .. }) => bytes,
        Ok(ResolvedClipboardPayload::BlobRef { blob_id, .. }) => blob_store
            .get(&blob_id)
            .await
            .map_err(|err| BuildSnapshotError::PasteRepBlobFetchFailed {
                blob_id,
                reason: err.to_string(),
            })?,
        Err(resolver_err) => {
            if let Some(blob_id) = &rep.blob_id {
                blob_store.get(blob_id).await.map_err(|blob_err| {
                    BuildSnapshotError::PasteRepBlobFetchFailed {
                        blob_id: blob_id.clone(),
                        reason: format!(
                            "resolver failed ({resolver_err}); blob fallback also failed: {blob_err}"
                        ),
                    }
                })?
            } else {
                // Mirror the non-file branch's stable-`Lost` invariant: an
                // orphaned paste-rep (cache + spool double miss) with no
                // blob fallback gets demoted before propagating, so the
                // next attempt sees a stable `Lost` outcome.
                if let PayloadResolveError::Orphaned { rep_id, state } = &resolver_err {
                    demote_orphaned_to_lost(rep_processing_repo, rep_id, state).await;
                }
                return Err(BuildSnapshotError::PasteRepUnavailable(resolver_err));
            }
        }
    };

    let uri_string =
        String::from_utf8(bytes).map_err(|err| BuildSnapshotError::InvalidFileUri {
            entry_id: entry_id.clone(),
            rep_id: rep.id.clone(),
            reason: format!("URI list bytes are not valid UTF-8: {err}"),
        })?;

    let mut file_paths = Vec::new();
    for line in uri_string.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with("file://") {
            // 错误消息只暴露 entry_id / rep_id —— `line` 是 `file://...` URI，
            // 含完整文件路径，属于用户私有 payload，禁止写入错误链 / 日志。
            match url::Url::parse(line) {
                Ok(url) => {
                    let path =
                        url.to_file_path()
                            .map_err(|_| BuildSnapshotError::InvalidFileUri {
                                entry_id: entry_id.clone(),
                                rep_id: rep.id.clone(),
                                reason: "URL could not be converted to a local file path"
                                    .to_string(),
                            })?;
                    file_paths.push(path);
                }
                Err(e) => {
                    return Err(BuildSnapshotError::InvalidFileUri {
                        entry_id: entry_id.clone(),
                        rep_id: rep.id.clone(),
                        reason: e.to_string(),
                    });
                }
            }
        } else {
            file_paths.push(PathBuf::from(line));
        }
    }

    if file_paths.is_empty() {
        return Err(BuildSnapshotError::NoFilePaths {
            entry_id: entry_id.clone(),
        });
    }

    // 本地源文件不存在 → 通过 `PayloadResolveError::Lost` 让上层（restore facade）
    // 映射为 `PayloadUnavailable`（HTTP 410 Gone）+ warn 级日志，避免被当成
    // 5xx 上报到 Sentry。reason 严禁携带文件路径或文件名——属于用户私有
    // payload（uc-application §16.3）。
    let missing_count = file_paths.iter().filter(|p| !p.exists()).count();
    if missing_count > 0 {
        return Err(BuildSnapshotError::PasteRepUnavailable(
            PayloadResolveError::Lost {
                rep_id: rep.id.clone(),
                reason: format!(
                    "{} of {} referenced file(s) no longer exist on disk",
                    missing_count,
                    file_paths.len()
                ),
            },
        ));
    }

    let snapshot = build_file_snapshot(&build_path_list(&file_paths));

    info!(
        entry_id = %entry_id,
        file_count = file_paths.len(),
        "snapshot_from_entry.reconstruct(file): files validated and snapshot built"
    );

    Ok(snapshot)
}

/// Demote an orphaned representation (cache+spool double miss) to `Lost`.
///
/// Called when the resolver reports [`PayloadResolveError::Orphaned`] for a
/// paste-rep. The representation can no longer be materialized — bytes are
/// gone from both cache and spool, and the worker has no source to retry
/// from. Marking it `Lost` ensures the next reconstruction attempt routes
/// to the `Lost` arm in the resolver and the caller-side mapping returns a
/// stable `PayloadUnavailable` / `EntryNotResendable` error instead of
/// producing 500s + Sentry events.
///
/// Best-effort: any DB failure is logged but does not propagate, because the
/// original resolve error is what the caller actually returns.
///
/// Free function (not a method) so unit tests can exercise the four
/// [`ProcessingUpdateOutcome`] arms without constructing the full helper
/// dependency set.
pub(crate) async fn demote_orphaned_to_lost(
    rep_processing_repo: &dyn UpdateRepresentationProcessingResultPort,
    rep_id: &RepresentationId,
    state: &PayloadAvailability,
) {
    let last_error =
        "orphaned during snapshot reconstruction: bytes lost before blob materialization";
    match rep_processing_repo
        .update_processing_result(
            rep_id,
            &[
                PayloadAvailability::Staged,
                PayloadAvailability::Processing,
                PayloadAvailability::Failed {
                    last_error: String::new(),
                },
            ],
            None,
            PayloadAvailability::Lost,
            Some(last_error),
        )
        .await
    {
        Ok(ProcessingUpdateOutcome::Updated(_)) => {
            info!(
                representation_id = %rep_id,
                payload_state = ?state,
                "Demoted orphaned representation to Lost (cache+spool miss)"
            );
        }
        Ok(ProcessingUpdateOutcome::StateMismatch) => {
            warn!(
                representation_id = %rep_id,
                payload_state = ?state,
                "Skipped Lost demotion due to state mismatch (likely already updated)"
            );
        }
        Ok(ProcessingUpdateOutcome::NotFound) => {
            warn!(
                representation_id = %rep_id,
                "Skipped Lost demotion: representation missing from DB"
            );
        }
        Err(err) => {
            warn!(
                representation_id = %rep_id,
                error = %err,
                "Failed to demote orphaned representation to Lost"
            );
        }
    }
}

/// Bundles the ports [`reconstruct_snapshot_from_entry`] needs so callers can
/// rebuild a snapshot from an entry id with a single dependency instead of
/// threading six ports each. The free function above stays the single source
/// of truth; this is a thin owning wrapper.
#[derive(Clone)]
pub(crate) struct SnapshotReconstructor {
    entry_repo: Arc<dyn GetClipboardEntryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    blob_store: Arc<dyn BlobReaderPort>,
}

impl SnapshotReconstructor {
    pub(crate) fn new(
        entry_repo: Arc<dyn GetClipboardEntryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
        blob_store: Arc<dyn BlobReaderPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            representation_repo,
            rep_processing_repo,
            payload_resolver,
            blob_store,
        }
    }

    /// Rebuild the [`SystemClipboardSnapshot`] for `entry_id`. Delegates to
    /// [`reconstruct_snapshot_from_entry`].
    pub(crate) async fn reconstruct(
        &self,
        entry_id: &EntryId,
    ) -> Result<SystemClipboardSnapshot, BuildSnapshotError> {
        reconstruct_snapshot_from_entry(
            self.entry_repo.as_ref(),
            self.selection_repo.as_ref(),
            self.representation_repo.as_ref(),
            self.rep_processing_repo.as_ref(),
            self.payload_resolver.as_ref(),
            self.blob_store.as_ref(),
            entry_id,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Mutex;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardRepositoryError, ClipboardSelection, ClipboardSelectionDecision,
        MimeType, PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{EventId, FormatId};
    use uc_core::BlobId;

    // ── demote_orphaned_to_lost ─────────────────────────────────────────

    /// Minimal hand-rolled fake for the narrow processing-result port.
    struct FakeRepRepo {
        next: Mutex<Option<Result<ProcessingUpdateOutcome, ClipboardRepositoryError>>>,
        calls: Mutex<Vec<(RepresentationId, PayloadAvailability, Option<String>)>>,
    }

    impl FakeRepRepo {
        fn new(outcome: Result<ProcessingUpdateOutcome, ClipboardRepositoryError>) -> Self {
            Self {
                next: Mutex::new(Some(outcome)),
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for FakeRepRepo {
        async fn update_processing_result(
            &self,
            rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            new_state: PayloadAvailability,
            last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            self.calls.lock().unwrap().push((
                rep_id.clone(),
                new_state.clone(),
                last_error.map(|s| s.to_string()),
            ));
            self.next
                .lock()
                .unwrap()
                .take()
                .expect("FakeRepRepo: update_processing_result called more than once")
        }
    }

    fn dummy_rep(id: &str) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            3,
            None,
            Some(BlobId::from("blob-x")),
        )
    }

    #[tokio::test]
    async fn demote_orphaned_calls_repo_with_lost_target_and_marker_text() {
        let repo = FakeRepRepo::new(Ok(ProcessingUpdateOutcome::Updated(dummy_rep("rep-1"))));
        let rep_id = RepresentationId::from("rep-1");

        demote_orphaned_to_lost(&repo, &rep_id, &PayloadAvailability::Staged).await;

        let calls = repo.calls.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, rep_id);
        assert_eq!(calls[0].1, PayloadAvailability::Lost);
        assert_eq!(
            calls[0].2.as_deref(),
            Some("orphaned during snapshot reconstruction: bytes lost before blob materialization")
        );
    }

    #[tokio::test]
    async fn demote_orphaned_swallows_state_mismatch() {
        let repo = FakeRepRepo::new(Ok(ProcessingUpdateOutcome::StateMismatch));
        let rep_id = RepresentationId::from("rep-2");

        demote_orphaned_to_lost(&repo, &rep_id, &PayloadAvailability::Processing).await;
        assert_eq!(repo.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn demote_orphaned_swallows_not_found() {
        let repo = FakeRepRepo::new(Ok(ProcessingUpdateOutcome::NotFound));
        let rep_id = RepresentationId::from("rep-3");

        demote_orphaned_to_lost(&repo, &rep_id, &PayloadAvailability::Staged).await;
        assert_eq!(repo.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn demote_orphaned_swallows_repo_error() {
        let repo = FakeRepRepo::new(Err(ClipboardRepositoryError::Storage(
            "transient db error".to_string(),
        )));
        let rep_id = RepresentationId::from("rep-4");

        demote_orphaned_to_lost(&repo, &rep_id, &PayloadAvailability::Staged).await;
        assert_eq!(repo.calls.lock().unwrap().len(), 1);
    }

    // ── reconstruct_snapshot_from_entry ─────────────────────────────────
    //
    // Hand-rolled fakes for the 5 ports; mockall does not buy us enough
    // here because every test only stubs a couple of methods and the
    // trait surface is wide.

    struct FakeEntryRepo {
        entry: Option<ClipboardEntry>,
    }

    #[async_trait]
    impl GetClipboardEntryPort for FakeEntryRepo {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
            Ok(self.entry.clone())
        }
    }

    struct FakeSelectionRepo {
        selection: Option<ClipboardSelectionDecision>,
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for FakeSelectionRepo {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            Ok(self.selection.clone())
        }
        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unimplemented!()
        }
    }

    struct StaticRepRepo {
        reps: Vec<PersistedClipboardRepresentation>,
    }

    #[async_trait]
    impl GetRepresentationPort for StaticRepRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>, ClipboardRepositoryError> {
            Ok(self
                .reps
                .iter()
                .find(|r| r.id == *representation_id)
                .cloned())
        }
    }

    /// Always reports a state mismatch — the reconstruct tests don't exercise
    /// the orphan-demotion path, so this stub is a no-op recorder.
    struct StubProcessingRepo;

    #[async_trait]
    impl UpdateRepresentationProcessingResultPort for StubProcessingRepo {
        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome, ClipboardRepositoryError> {
            Ok(ProcessingUpdateOutcome::StateMismatch)
        }
    }

    enum ResolveBehavior {
        Inline(Vec<u8>),
        Lost,
    }

    struct StubResolver {
        behavior: ResolveBehavior,
    }

    #[async_trait]
    impl ClipboardPayloadResolverPort for StubResolver {
        async fn resolve(
            &self,
            rep: &PersistedClipboardRepresentation,
        ) -> Result<ResolvedClipboardPayload, PayloadResolveError> {
            match &self.behavior {
                ResolveBehavior::Inline(bytes) => Ok(ResolvedClipboardPayload::Inline {
                    mime: rep
                        .mime_type
                        .as_ref()
                        .map(|m| m.as_str().to_string())
                        .unwrap_or_default(),
                    bytes: bytes.clone(),
                }),
                ResolveBehavior::Lost => Err(PayloadResolveError::Lost {
                    rep_id: rep.id.clone(),
                    reason: "synthetic lost".to_string(),
                }),
            }
        }
    }

    struct UnusedBlobStore;

    #[async_trait]
    impl BlobReaderPort for UnusedBlobStore {
        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            unreachable!("UnusedBlobStore: get() must not be called in these tests")
        }
    }

    fn text_rep(id: &str, bytes: &[u8]) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            bytes.len() as i64,
            Some(bytes.to_vec()),
            None,
        )
    }

    fn entry_with_event(entry_id: &EntryId, event_id: &EventId) -> ClipboardEntry {
        ClipboardEntry::new(entry_id.clone(), event_id.clone(), 0, None, 0)
    }

    fn selection_for(entry_id: &EntryId, paste_rep_id: &str) -> ClipboardSelectionDecision {
        let paste = RepresentationId::from(paste_rep_id);
        ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: paste.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: paste.clone(),
                paste_rep_id: paste,
                policy_version: SelectionPolicyVersion::V1,
            },
        )
    }

    #[tokio::test]
    async fn reconstruct_returns_entry_not_found_when_repo_returns_none() {
        let entry_id = EntryId::from("entry-missing");

        let err = reconstruct_snapshot_from_entry(
            &FakeEntryRepo { entry: None },
            &FakeSelectionRepo { selection: None },
            &StaticRepRepo { reps: Vec::new() },
            &StubProcessingRepo,
            &StubResolver {
                behavior: ResolveBehavior::Lost,
            },
            &UnusedBlobStore,
            &entry_id,
        )
        .await
        .expect_err("expected EntryNotFound");

        match err {
            BuildSnapshotError::EntryNotFound { entry_id: id } => {
                assert_eq!(id, entry_id);
            }
            other => panic!("expected EntryNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconstruct_returns_selection_not_found_when_selection_missing() {
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("evt-1");

        let err = reconstruct_snapshot_from_entry(
            &FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            },
            &FakeSelectionRepo { selection: None },
            &StaticRepRepo { reps: Vec::new() },
            &StubProcessingRepo,
            &StubResolver {
                behavior: ResolveBehavior::Lost,
            },
            &UnusedBlobStore,
            &entry_id,
        )
        .await
        .expect_err("expected SelectionNotFound");

        match err {
            BuildSnapshotError::SelectionNotFound { entry_id: id } => {
                assert_eq!(id, entry_id);
            }
            other => panic!("expected SelectionNotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconstruct_packs_text_paste_rep_into_snapshot() {
        let entry_id = EntryId::from("entry-text");
        let event_id = EventId::from("evt-text");
        let rep = text_rep("rep-text", b"hello world");

        let snapshot = reconstruct_snapshot_from_entry(
            &FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            },
            &FakeSelectionRepo {
                selection: Some(selection_for(&entry_id, "rep-text")),
            },
            &StaticRepRepo { reps: vec![rep] },
            &StubProcessingRepo,
            &StubResolver {
                behavior: ResolveBehavior::Inline(b"hello world".to_vec()),
            },
            &UnusedBlobStore,
            &entry_id,
        )
        .await
        .expect("expected success");

        assert_eq!(snapshot.representations.len(), 1);
        let only = &snapshot.representations[0];
        assert_eq!(only.format_id.as_ref() as &str, "public.utf8-plain-text");
        assert_eq!(only.inline_bytes(), Some(b"hello world".as_slice()));
    }

    #[tokio::test]
    async fn reconstruct_returns_paste_rep_unavailable_when_resolver_returns_lost() {
        let entry_id = EntryId::from("entry-lost");
        let event_id = EventId::from("evt-lost");
        let rep = text_rep("rep-lost", b"placeholder");

        let err = reconstruct_snapshot_from_entry(
            &FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            },
            &FakeSelectionRepo {
                selection: Some(selection_for(&entry_id, "rep-lost")),
            },
            &StaticRepRepo { reps: vec![rep] },
            &StubProcessingRepo,
            &StubResolver {
                behavior: ResolveBehavior::Lost,
            },
            &UnusedBlobStore,
            &entry_id,
        )
        .await
        .expect_err("expected PasteRepUnavailable");

        let payload_err = match err {
            BuildSnapshotError::PasteRepUnavailable(p) => p,
            other => panic!("expected PasteRepUnavailable, got {other:?}"),
        };
        match payload_err {
            PayloadResolveError::Lost { rep_id, .. } => {
                assert_eq!(rep_id.as_ref() as &str, "rep-lost");
            }
            other => panic!("expected PayloadResolveError::Lost, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reconstruct_returns_paste_rep_not_found_when_rep_missing_from_repo() {
        let entry_id = EntryId::from("entry-orphan-sel");
        let event_id = EventId::from("evt-orphan-sel");
        // Selection references rep-x but the repo has no reps at all.
        let err = reconstruct_snapshot_from_entry(
            &FakeEntryRepo {
                entry: Some(entry_with_event(&entry_id, &event_id)),
            },
            &FakeSelectionRepo {
                selection: Some(selection_for(&entry_id, "rep-x")),
            },
            &StaticRepRepo { reps: Vec::new() },
            &StubProcessingRepo,
            &StubResolver {
                behavior: ResolveBehavior::Lost,
            },
            &UnusedBlobStore,
            &entry_id,
        )
        .await
        .expect_err("expected PasteRepNotFound");

        match err {
            BuildSnapshotError::PasteRepNotFound {
                rep_id,
                event_id: evt,
            } => {
                assert_eq!(rep_id.as_ref() as &str, "rep-x");
                assert_eq!(evt, event_id);
            }
            other => panic!("expected PasteRepNotFound, got {other:?}"),
        }
    }
}
