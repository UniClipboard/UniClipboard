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
    clipboard::ClipboardIntegrationMode,
    ids::EntryId,
    ports::{
        clipboard::{
            ClipboardPayloadResolverPort, GetClipboardEntryPort, GetRepresentationPort,
            UpdateRepresentationProcessingResultPort,
        },
        ClipboardSelectionRepositoryPort,
    },
};

use crate::clipboard_write::{
    ClipboardWriteCoordinator, ClipboardWriteIntent, LocalActiveRegisterAdvancer,
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
        }
    }

    /// Wire the active-clipboard register advancer. When set, a successful
    /// restore advances the cross-device register (best-effort).
    pub(crate) fn with_active_register(mut self, advancer: LocalActiveRegisterAdvancer) -> Self {
        self.active_register = Some(advancer);
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
        // Capture the content identity before the snapshot is moved into the
        // write boundary; the register advances only after the OS write
        // succeeds, keeping "register advanced ⟺ OS write succeeded".
        let content_hash = snapshot.snapshot_hash().to_string();
        self.coordinator
            .write(snapshot, ClipboardWriteIntent::LocalRestore)
            .await?;
        if let Some(advancer) = &self.active_register {
            advancer.advance_local(content_hash, entry_id.clone()).await;
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
