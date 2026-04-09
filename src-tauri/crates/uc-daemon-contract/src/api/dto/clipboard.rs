//! DTOs for clipboard API endpoints.
//!
//! All response payloads use `camelCase` field names (via `#[serde(rename_all = "camelCase")]`)
//! to match the frontend TypeScript interface conventions.

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use uc_app::usecases::clipboard::get_entry_detail::EntryDetailResult;
use uc_app::usecases::clipboard::get_entry_resource::EntryResourceResult;
use uc_app::usecases::clipboard::list_entry_projections::EntryProjectionDto;
use uc_app::usecases::clipboard::{clear_history::ClearHistoryResult, ClipboardStats};

// ── Entry projection ──────────────────────────────────────────────

/// Clipboard entry projection — lightweight summary for list views.
/// Matches the frontend `ClipboardEntryDto` interface.
#[derive(Debug, Clone, Serialize, ToSchema)]
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
}

impl From<EntryProjectionDto> for EntryProjectionResponseDto {
    fn from(dto: EntryProjectionDto) -> Self {
        Self {
            id: dto.id,
            preview: dto.preview,
            has_detail: dto.has_detail,
            size_bytes: dto.size_bytes,
            captured_at: dto.captured_at,
            content_type: dto.content_type,
            thumbnail_url: dto.thumbnail_url,
            is_encrypted: dto.is_encrypted,
            is_favorited: dto.is_favorited,
            updated_at: dto.updated_at,
            active_time: dto.active_time,
            file_transfer_status: dto.file_transfer_status,
            file_transfer_reason: dto.file_transfer_reason,
            link_urls: dto.link_urls,
            link_domains: dto.link_domains,
            file_sizes: dto.file_sizes,
        }
    }
}

// ── List entries response ─────────────────────────────────────────

/// GET /clipboard/entries response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ListEntriesResponse {
    pub data: Vec<EntryProjectionResponseDto>,
    pub ts: i64,
}

// ── Entry detail ──────────────────────────────────────────────────

/// Full entry detail (text content).
/// Matches the frontend `EntryDetail` interface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryDetailDto {
    pub id: String,
    pub content: String,
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub mime_type: Option<String>,
}

impl From<EntryDetailResult> for EntryDetailDto {
    fn from(r: EntryDetailResult) -> Self {
        Self {
            id: r.id,
            content: r.content,
            size_bytes: r.size_bytes,
            created_at_ms: r.created_at_ms,
            active_time_ms: r.active_time_ms,
            mime_type: r.mime_type,
        }
    }
}

/// GET /clipboard/entries/:id response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetEntryDetailResponse {
    pub data: EntryDetailDto,
    pub ts: i64,
}

// ── Entry resource ────────────────────────────────────────────────

/// Resource metadata (blob URL or inline data).
/// Matches the frontend `ClipboardEntryResource` interface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryResourceDto {
    pub blob_id: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: Option<String>,
    /// Base64-encoded inline data (when content is stored inline, not in blob).
    pub inline_data: Option<String>,
}

impl From<EntryResourceResult> for EntryResourceDto {
    fn from(r: EntryResourceResult) -> Self {
        Self {
            blob_id: r.blob_id.map(|id| id.to_string()),
            mime_type: r.mime_type,
            size_bytes: r.size_bytes,
            url: r.url,
            inline_data: r.inline_data.map(|bytes| STANDARD.encode(bytes)),
        }
    }
}

/// GET /clipboard/entries/:id/resource response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetEntryResourceResponse {
    pub data: EntryResourceDto,
    pub ts: i64,
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

impl From<ClipboardStats> for ClipboardStatsDto {
    fn from(s: ClipboardStats) -> Self {
        Self {
            total_items: s.total_items,
            total_size: s.total_size,
        }
    }
}

/// GET /clipboard/stats response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetClipboardStatsResponse {
    pub data: ClipboardStatsDto,
    pub ts: i64,
}

// ── Clear history ─────────────────────────────────────────────────

/// POST /clipboard/entries/clear response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearHistoryResponse {
    pub data: ClearHistoryResultDto,
    pub ts: i64,
}

/// Result of clearing clipboard history.
/// Matches the frontend `ClearHistoryResult` interface.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearHistoryResultDto {
    pub deleted_count: u64,
    pub failed_entries: Vec<(String, String)>,
}

impl From<ClearHistoryResult> for ClearHistoryResultDto {
    fn from(r: ClearHistoryResult) -> Self {
        Self {
            deleted_count: r.deleted_count,
            failed_entries: r.failed_entries,
        }
    }
}

// ── Toggle favorite ───────────────────────────────────────────────

/// POST /clipboard/entries/:id/favorite request body.
#[derive(Debug, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleFavoriteRequest {
    pub is_favorited: bool,
}

/// POST /clipboard/entries/:id/favorite response.
#[derive(Debug, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ToggleFavoriteResponse {
    pub data: ToggleFavoriteResultDto,
    pub ts: i64,
}

/// Result of toggling favorite state.
#[derive(Debug, Serialize, ToSchema)]
pub struct ToggleFavoriteResultDto {
    pub success: bool,
}
