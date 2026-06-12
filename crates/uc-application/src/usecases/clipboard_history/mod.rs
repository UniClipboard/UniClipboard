//! Application-layer clipboard history use cases.
//!
//! Migrated out of `uc-app` so daemon/tauri composition roots no longer reach
//! into `uc-app::usecases` for clipboard history. Per
//! `uc-application/AGENTS.md` §11.4 every type here stays `pub(crate)` —
//! external callers reach them exclusively through `ClipboardHistoryFacade`.

pub(crate) mod cleanup;
pub(crate) mod clear_history;
pub(crate) mod delete_entry;
pub(crate) mod get_entry_detail;
pub(crate) mod get_entry_resource;
pub(crate) mod list_entry_projections;
pub(crate) mod reconcile_missing_files;
pub(crate) mod toggle_favorite;

pub(crate) use cleanup::{CleanupExpiredFilesUseCase, CleanupResult};
pub(crate) use clear_history::ClearClipboardHistoryUseCase;
pub(crate) use delete_entry::DeleteClipboardEntryUseCase;
pub(crate) use get_entry_detail::{EntryDetailResult, GetEntryDetailUseCase};
pub(crate) use get_entry_resource::{EntryResourceResult, GetEntryResourceUseCase};
pub(crate) use list_entry_projections::{
    EntryProjectionDto, ListClipboardEntryProjectionsUseCase, ListProjectionsError,
};
pub(crate) use reconcile_missing_files::{ReconcileMissingFilesUseCase, ReconcileResult};
pub(crate) use toggle_favorite::ToggleFavoriteClipboardEntryUseCase;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ClipboardStats {
    pub(crate) total_items: i64,
    pub(crate) total_size: i64,
}

pub(crate) fn compute_clipboard_stats(entries: &[EntryProjectionDto]) -> ClipboardStats {
    let total_items = entries.len() as i64;
    let total_size = entries.iter().map(|e| e.size_bytes).sum();
    ClipboardStats {
        total_items,
        total_size,
    }
}
