//! Restore a system clipboard state from a historical entry.
//!
//! 本用例的核心 "rebuild a `SystemClipboardSnapshot` from an entry" 已经抽到
//! [`reconstruct_snapshot_from_entry`](crate::usecases::clipboard_sync::snapshot_from_entry::reconstruct_snapshot_from_entry)，
//! 与 resend 路径共享；这里只保留 restore-specific 的薄逻辑：
//!
//! - 受 `ClipboardIntegrationMode::allow_os_write` 闸口控制（passive 模式直接拒绝）；
//! - 把 helper 返回的 [`BuildSnapshotError`] 翻译回 `anyhow::Result`，并保留
//!   [`PayloadResolveError`] 的 typed downcast 通道，让 `ClipboardRestoreFacade`
//!   能继续映射成 `PayloadUnavailable`；
//! - 把成功的 snapshot 交给 [`ClipboardWriteCoordinator`] 写回本机剪贴板。

use anyhow::Result;
use std::sync::Arc;
use tracing::info;

use uc_core::{
    blob::ports::BlobReaderPort,
    clipboard::{ClipboardContentCategorySet, ClipboardIntegrationMode},
    ids::EntryId,
    ports::{
        clipboard::{
            ClipboardPayloadResolverPort, GetClipboardEntryPort, GetEntrySnapshotHashPort,
            GetRepresentationPort, UpdateRepresentationProcessingResultPort,
        },
        ClipboardSelectionRepositoryPort,
    },
};

use crate::clipboard_write::{
    ClipboardWriteCoordinator, ClipboardWriteIntent, LocalActiveRegisterAdvancer,
    RestoreBroadcastTrigger,
};
use crate::usecases::clipboard_sync::snapshot_from_entry::{
    reconstruct_snapshot_from_entry, BuildSnapshotError,
};

pub(crate) struct RestoreClipboardSelectionUseCase {
    clipboard_repo: Arc<dyn GetClipboardEntryPort>,
    coordinator: Arc<ClipboardWriteCoordinator>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    representation_repo: Arc<dyn GetRepresentationPort>,
    rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
    payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    blob_store: Arc<dyn BlobReaderPort>,
    mode: ClipboardIntegrationMode,
    /// Optional active-clipboard register hook. When wired, a successful
    /// restore advances the cross-device register so the restored content
    /// becomes the latest active clipboard state. `None` in tests / contexts
    /// that don't track active state.
    active_register: Option<LocalActiveRegisterAdvancer>,
    /// Forward lookup of the entry's persisted cross-device snapshot hash.
    /// Wired together with `active_register`: the register must advance with
    /// the value peers resolve the content by (the persisted
    /// `clipboard_event.snapshot_hash`), never a hash recomputed from the
    /// reconstructed snapshot — the two diverge for file entries.
    entry_snapshot_hash_lookup: Option<Arc<dyn GetEntrySnapshotHashPort>>,
    /// Optional restore-broadcast hook. When wired, a successful restore that
    /// advanced the register also offers the activation to the broadcast
    /// subsystem (which applies the `sync_on_restore` + per-device send gate
    /// before announcing it to peers). `None` decouples the OS write from any
    /// network announcement (tests / non-broadcasting contexts).
    restore_broadcast: Option<RestoreBroadcastTrigger>,
}

impl RestoreClipboardSelectionUseCase {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        clipboard_repo: Arc<dyn GetClipboardEntryPort>,
        coordinator: Arc<ClipboardWriteCoordinator>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        representation_repo: Arc<dyn GetRepresentationPort>,
        rep_processing_repo: Arc<dyn UpdateRepresentationProcessingResultPort>,
        payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
        blob_store: Arc<dyn BlobReaderPort>,
        mode: ClipboardIntegrationMode,
    ) -> Self {
        Self {
            clipboard_repo,
            coordinator,
            selection_repo,
            representation_repo,
            rep_processing_repo,
            payload_resolver,
            blob_store,
            mode,
            active_register: None,
            entry_snapshot_hash_lookup: None,
            restore_broadcast: None,
        }
    }

    /// Wire the active-clipboard register advancer plus the lookup that reads
    /// the entry's persisted snapshot hash. When set, a successful restore
    /// advances the cross-device register with the persisted identity
    /// (best-effort). Both are wired together — the advance is meaningless
    /// without the persisted hash to stamp it with.
    pub(crate) fn with_active_register(
        mut self,
        advancer: LocalActiveRegisterAdvancer,
        entry_snapshot_hash_lookup: Arc<dyn GetEntrySnapshotHashPort>,
    ) -> Self {
        self.active_register = Some(advancer);
        self.entry_snapshot_hash_lookup = Some(entry_snapshot_hash_lookup);
        self
    }

    /// Wire the restore-broadcast trigger. When set, a successful restore that
    /// advanced the register offers the activation to the broadcast subsystem.
    /// Only meaningful alongside `with_active_register`; without it nothing
    /// advances, so nothing is offered.
    pub(crate) fn with_restore_broadcast(mut self, trigger: RestoreBroadcastTrigger) -> Self {
        self.restore_broadcast = Some(trigger);
        self
    }

    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        info!(entry_id = %entry_id, "restore.execute requested");
        if !self.mode.allow_os_write() {
            return Err(anyhow::anyhow!(
                "System clipboard writes disabled (UC_CLIPBOARD_MODE=passive)"
            ));
        }
        let snapshot = reconstruct_snapshot_from_entry(
            self.clipboard_repo.as_ref(),
            self.selection_repo.as_ref(),
            self.representation_repo.as_ref(),
            self.rep_processing_repo.as_ref(),
            self.payload_resolver.as_ref(),
            self.blob_store.as_ref(),
            entry_id,
        )
        .await
        .map_err(map_build_snapshot_error)?;
        // Capture the category set before the snapshot is moved into the write
        // boundary; the register advances only after the OS write succeeds,
        // keeping "register advanced ⟺ OS write succeeded".
        let categories = ClipboardContentCategorySet::from_snapshot(&snapshot);
        self.coordinator
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await?;
        if let (Some(advancer), Some(hash_lookup)) =
            (&self.active_register, &self.entry_snapshot_hash_lookup)
        {
            // The register's cross-device identity must be the entry's PERSISTED
            // `clipboard_event.snapshot_hash` — the value a peer resolves the
            // content by (`find_entry_id_by_snapshot_hash`). Recomputing it from
            // the reconstructed snapshot diverges for file entries (reconstruct
            // emits a fresh `text/uri-list` whose hash differs from the captured
            // file's), which would make every cross-device pull miss. A lookup
            // miss / error leaves the register untouched — best-effort, the OS
            // write already succeeded.
            match hash_lookup.get_entry_snapshot_hash(entry_id).await {
                Ok(Some(snapshot_hash)) => {
                    let state = advancer
                        .advance_local(snapshot_hash, entry_id.clone())
                        .await;
                    // Offer the just-activated state to the broadcast subsystem.
                    // The gate (`sync_on_restore` + per-device send filter) lives
                    // in the broadcaster; here we only hand off the activation +
                    // its categories. Fire-and-forget — never fails the restore.
                    if let Some(trigger) = &self.restore_broadcast {
                        trigger.offer(state, categories);
                    }
                }
                Ok(None) => {
                    info!(
                        entry_id = %entry_id,
                        "restore: no persisted snapshot_hash for entry; skipping active-register advance"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        entry_id = %entry_id,
                        error = %err,
                        "restore: snapshot_hash lookup failed; skipping active-register advance"
                    );
                }
            }
        }
        Ok(())
    }
}

/// Map [`BuildSnapshotError`] back onto `anyhow::Error` while preserving the
/// existing restore-facade contract:
///
/// - [`BuildSnapshotError::PasteRepUnavailable`] unwraps the typed
///   [`PayloadResolveError`] so that
///   `ClipboardRestoreFacade::map_restore_error` keeps downcasting it to
///   `PayloadUnavailable` (HTTP 410 Gone).
/// - Other variants flow through `anyhow::Error::new`; their `Display` impls
///   preserve the "not found" / "Failed to" substrings that the facade
///   matches against for `NotFound` / `Internal` mapping.
fn map_build_snapshot_error(err: BuildSnapshotError) -> anyhow::Error {
    match err {
        BuildSnapshotError::PasteRepUnavailable(payload_err) => anyhow::Error::new(payload_err),
        BuildSnapshotError::Repository(inner) => inner,
        other => anyhow::Error::new(other),
    }
}
