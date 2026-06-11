//! DTOs for clipboard API endpoints.
//!
//! All response payloads use `camelCase` field names (via `#[serde(rename_all = "camelCase")]`)
//! to match the frontend TypeScript interface conventions.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// ── Entry projection ──────────────────────────────────────────────

/// Clipboard entry projection — lightweight summary for list views.
/// Matches the frontend `ClipboardEntryDto` interface.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryProjectionResponseDto {
    pub id: String,
    pub preview: String,
    pub has_detail: bool,
    pub size_bytes: i64,
    pub captured_at: i64,
    pub content_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    pub is_encrypted: bool,
    pub is_favorited: bool,
    pub updated_at: i64,
    pub active_time: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_transfer_status: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_transfer_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_domains: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_sizes: Option<Vec<i64>>,
    /// Original image width in pixels (only for image entries).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_width: Option<i32>,
    /// Original image height in pixels (only for image entries).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_height: Option<i32>,
    /// `paste_rep` 的 payload_state, 仅在 `Lost` 时输出。前端用此把"内容已
    /// 丢失"的 entry 灰显, 让用户在点击粘贴前就知道这条记录已不可用。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload_state: Option<String>,
}

// ── Entry detail ──────────────────────────────────────────────────

/// Full entry detail (text content).
/// Matches the frontend `EntryDetail` interface.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetailDto {
    pub id: String,
    pub content: String,
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub mime_type: Option<String>,
}

// ── Entry resource ────────────────────────────────────────────────

/// Resource metadata (blob URL or inline data).
/// Matches the frontend `ClipboardEntryResource` interface.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryResourceDto {
    pub blob_id: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: Option<String>,
    /// Base64-encoded inline data (when content is stored inline, not in blob).
    pub inline_data: Option<String>,
}

// ── Clipboard stats ───────────────────────────────────────────────

/// Aggregate clipboard statistics.
/// Matches the frontend `ClipboardStats` interface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStatsDto {
    pub total_items: i64,
    pub total_size: i64,
}

// ── Clear history ─────────────────────────────────────────────────

/// Result of clearing clipboard history.
/// Matches the frontend `ClearHistoryResult` interface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearHistoryResultDto {
    pub deleted_count: u64,
    pub failed_entries: Vec<(String, String)>,
}

// ── Toggle favorite ───────────────────────────────────────────────

/// POST /clipboard/entries/:id/favorite request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleFavoriteRequest {
    pub is_favorited: bool,
}

/// Result of toggling favorite state.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToggleFavoriteResultDto {
    pub success: bool,
}
