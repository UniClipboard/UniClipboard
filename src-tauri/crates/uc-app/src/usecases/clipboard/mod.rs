pub mod clear_history;
pub mod get_entry_detail;
pub mod get_entry_resource;
pub mod integration_mode;
pub mod list_entry_projections;
pub mod resolve_blob_resource;
pub mod resolve_thumbnail_resource;
pub mod restore_clipboard_selection;
pub mod sync_inbound;
pub mod sync_outbound;
pub mod toggle_favorite_clipboard_entry;
pub mod touch_clipboard_entry;

pub use clear_history::{ClearClipboardHistory, ClearHistoryResult};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardStats {
    pub total_items: i64,
    pub total_size: i64,
}

pub struct ClipboardUseCases;

impl ClipboardUseCases {
    pub fn compute_stats(entries: &[EntryProjectionDto]) -> ClipboardStats {
        compute_clipboard_stats(entries)
    }
}

pub fn compute_clipboard_stats(entries: &[EntryProjectionDto]) -> ClipboardStats {
    let total_items = entries.len() as i64;
    let total_size = entries.iter().map(|e| e.size_bytes).sum();
    ClipboardStats {
        total_items,
        total_size,
    }
}

pub use integration_mode::ClipboardIntegrationMode;
pub use list_entry_projections::{
    EntryProjectionDto, ListClipboardEntryProjections, ListProjectionsError,
};
