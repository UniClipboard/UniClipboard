//! DTOs for the configuration-migration endpoints (export / import preview /
//! import staging).
//!
//! These wire shapes are shared by the generated TypeScript client and native
//! Rust consumers (one source of truth). Passwords arrive over the loopback,
//! session-JWT-gated API; handlers MUST never log these bodies.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Request body for `POST /config/export`.
///
/// No export password is taken: the bundle is sealed with the installation's own
/// key material, so opening it later requires the space passphrase. `target_path`
/// is the absolute destination the daemon writes the `.ucbundle` file to.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigRequest {
    pub target_path: String,
}

/// Response payload for `POST /config/export` on success.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExportConfigResponse {
    /// Absolute path the bundle was written to.
    pub path: String,
}

/// Request body for `POST /config/import/preview`.
///
/// Read-only: decrypts the bundle's manifest to surface descriptive metadata
/// for operator confirmation. The handler MUST never log this body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewImportRequest {
    pub password: String,
    pub source_path: String,
}

/// Response payload for `POST /config/import/preview`.
///
/// Carries only non-secret descriptive metadata read from the bundle manifest.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PreviewImportResponse {
    /// Application version string of the installation that produced the bundle.
    pub app_version: String,
    /// Storage layout the bundle was produced under (`portable` / `installed`).
    pub source_mode: String,
    /// Bundle creation time, milliseconds since the Unix epoch.
    pub created_at_unix_ms: i64,
    /// Profile the bundle's configuration belongs to.
    pub profile_id: String,
    /// Stable identity fingerprint of the producing device, for human
    /// confirmation. Adopting this bundle makes the target device present
    /// itself under this same identity.
    pub device_fingerprint: String,
}

/// Request body for `POST /config/import`.
///
/// `confirmed` is a deliberate gate: the import is a device-identity move, so
/// the caller must explicitly confirm. The handler MUST never log this body.
#[derive(Debug, Clone, Deserialize, Serialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportConfigRequest {
    pub password: String,
    pub source_path: String,
    pub confirmed: bool,
}

/// Response payload for `POST /config/import` on success (staged for the next
/// restart to apply on boot).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImportConfigResponse {
    /// Always `true` on success: the bundle was validated and staged.
    pub staged_ok: bool,
    /// `true` when applying the staged migration will require the operator to
    /// re-enter their passphrase to unlock after restart; `false` when the
    /// staged material is sufficient to unlock without further input.
    pub unlock_required_after_apply: bool,
}
