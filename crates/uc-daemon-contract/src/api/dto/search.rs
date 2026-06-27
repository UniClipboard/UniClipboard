//! Shared transport DTOs for the search query, status, and rebuild HTTP endpoints.
//!
//! This is the single source of truth for search response envelopes.
//! The daemon re-exports these via `pub use uc_daemon_contract::api::dto::search::*;`.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Single search result — mirrors `SearchResult` with camelCase transport names.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchResultDto {
    pub entry_id: String,
    pub content_type: String,
    pub active_time_ms: i64,
    /// Derived/user-state tag ids (e.g. `"link"`, `"favorited"`). The favorite
    /// marker is expressed by the presence of `"favorited"`, not a separate flag.
    pub tags: Vec<String>,
    pub text_preview: Option<String>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    /// Display names of referenced files; empty when none.
    pub file_names: Vec<String>,
    /// Web URLs (http/https) carried by this entry; empty when none.
    pub link_urls: Vec<String>,
    /// Originating device id, or `null` when the source is unknown.
    pub source_device: Option<String>,
    /// `"Lost"` when the paste payload is unrecoverable, else `null`.
    pub payload_state: Option<String>,
}

/// Folded payload for `GET /search/query` (ADR-008 §0.1).
///
/// The current handler returns `total` and `hasMore` as top-level siblings of
/// the `{data,ts}` envelope (`data` is the items array). This DTO folds those
/// siblings INTO the payload (renaming `data` → `items`) so the endpoint can
/// return `ApiEnvelope<SearchQueryResultDto>` with no bespoke wrapper. P1 only
/// defines the type; the handler is rewired in P2.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryResultDto {
    pub items: Vec<SearchResultDto>,
    pub total: u32,
    pub has_more: bool,
    /// `"ready"` when served from the index, or `"degraded"` when the index was
    /// not ready and this filter-less browse was served from the main store
    /// (§4.7). Filtered/keyword queries never return `"degraded"` — they surface
    /// an `index_rebuilding` error instead.
    pub state: String,
}

/// A tag and its entry count for `GET /search/tags`. `is_builtin` marks the
/// reserved builtin tags (`link`/`favorited`/`image`); custom tags are present
/// only in unlocked sessions (§4.6).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchTagDto {
    pub tag_id: String,
    pub count: u32,
    pub is_builtin: bool,
}

/// Search index availability snapshot — the `ApiEnvelope` payload for
/// `GET /search/status` (ADR-008 §0.1).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatusData {
    /// One of: "ready", "rebuilding", "unavailable"
    pub state: String,
    /// Optional reason code (present when rebuilding or unavailable).
    pub reason: Option<String>,
    /// Millisecond timestamp of the last rebuild start (from `SearchIndexMeta`).
    pub last_rebuild_started_at_ms: Option<i64>,
    /// Millisecond timestamp of the last completed rebuild (from `SearchIndexMeta`).
    pub last_rebuild_completed_at_ms: Option<i64>,
}

/// Acceptance payload — the `ApiEnvelope` payload for `POST /search/rebuild`
/// (ADR-008 §0.1).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SearchRebuildAcceptedData {
    pub accepted: bool,
}
