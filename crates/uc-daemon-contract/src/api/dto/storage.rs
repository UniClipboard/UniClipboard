//! DTOs for the storage management endpoints (ADR-008 §C.4).
//!
//! Relocated out of the webserver `src/api/storage.rs` inline definitions so the
//! generated TypeScript client and native consumers share one source of truth.
//! `ClearCacheErrorResponse` (the 400-path body) stays webserver-local in P1.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response payload for `GET /storage/stats`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageStatsDto {
    pub total_bytes: u64,
    pub database_bytes: u64,
    pub vault_bytes: u64,
    pub cache_bytes: u64,
    pub logs_bytes: u64,
}

/// Request payload for `POST /storage/clear-cache`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearCacheRequest {
    pub confirmed: bool,
}

/// Response payload for `POST /storage/clear-cache` on success.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClearCacheResponse {
    pub freed_bytes: u64,
}
