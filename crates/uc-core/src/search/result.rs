//! SearchResult and RebuildProgress — output types for SearchIndexPort.

use crate::ids::EntryId;
use crate::search::document::ContentType;
use serde::{Deserialize, Serialize};

/// Single search result row — carries the full metadata needed to render
/// a ClipboardItemRow in the UI without a second API call (per D-01).
///
/// Fields are exactly those specified in D-01:
/// entry_id, content_type, active_time_ms, text_preview, mime_type, file_extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub entry_id: EntryId,
    pub content_type: ContentType,
    pub active_time_ms: i64,
    /// Truncated preview (~80 chars) — truncation logic lives in Phase 89 use case, not here.
    pub text_preview: Option<String>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
}

/// Paged output from `SearchIndexPort::search()`.
///
/// Carries all pagination metadata so the route layer does not need a
/// separate count query or fake `has_more` inference (per D-02, Phase 92).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultsPage {
    /// Matching entries for the requested page window.
    pub items: Vec<SearchResult>,
    /// Total count of matching entries across all pages (computed before pagination).
    pub total: u32,
    /// Whether more pages follow the current page window.
    pub has_more: bool,
}

/// Stage of a full index rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebuildStage {
    Started,
    Indexing,
    Complete,
    Failed,
}

/// Progress update emitted by `SearchIndexPort::rebuild()` through an mpsc channel.
///
/// The daemon subscribes to the channel and forwards events over WebSocket —
/// uc-core has no WebSocket knowledge (D-07). This mirrors the TransferProgress
/// pattern already in the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildProgress {
    pub stage: RebuildStage,
    /// Number of entries indexed so far.
    pub indexed: u32,
    /// Total entries to index (0 if unknown at start).
    pub total: u32,
}
