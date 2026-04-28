//! DTOs for the upgrade detection API endpoints.
//!
//! See `uc-application::facade::upgrade` for the underlying use case
//! semantics. The wire format mirrors `UpgradeStatus` with a discriminator
//! field `kind` so the frontend can switch on it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Response wrapper for `GET /upgrade/status`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct GetUpgradeStatusResponse {
    pub data: UpgradeStatusDto,
    pub ts: i64,
}

/// Discriminated union mirroring `uc_application::facade::UpgradeStatus`.
///
/// Wire encoding uses `kind` discriminator with snake_case variants to
/// keep parity with the CLI JSON output produced by `uniclip upgrade
/// status --json`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpgradeStatusDto {
    /// First time the app is launched on this profile.
    FreshInstall { current: String },
    /// Cursor matches the running build; no action needed.
    NoChange { current: String },
    /// Cursor lags the running build (or is missing on a setup-completed
    /// profile). `from = None` means the previous version is unknown
    /// (pre-cursor era / corrupt cursor fallback).
    Upgraded { from: Option<String>, to: String },
    /// Cursor leads the running build — the user rolled back.
    Downgraded { from: String, to: String },
}

/// Response wrapper for `POST /upgrade/ack`.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AckUpgradeResponse {
    pub data: AckUpgradePayload,
    pub ts: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AckUpgradePayload {
    pub acknowledged: String,
}
