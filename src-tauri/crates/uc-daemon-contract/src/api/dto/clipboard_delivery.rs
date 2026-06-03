//! DTOs for the clipboard entry delivery-view endpoint
//! (`GET /clipboard/entries/{id}/delivery`, ADR-008 P3-1 / D15).
//!
//! Returns "where did this entry come from + per-peer sync status" for the
//! detail panel. Ported from the former GUI-only `clipboard_delivery` Tauri
//! command DTOs when the GUI moved onto the loopback API — the original
//! "GUI-only, never on the wire" rationale was a `GuiInProcess`-era artifact
//! that D2/D15 retire. The domain → DTO conversions live in `uc-webserver`
//! (this contract crate does not depend on `uc-application`).
//!
//! `tag` literals + camelCase field names are wire-identical to the previous
//! tauri-specta DTOs so the frontend discriminated unions are unchanged.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Mirror of the domain `EntryDeliveryView`: origin + every trusted peer's
/// latest delivery status.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryDeliveryViewDto {
    pub entry_id: String,
    pub source: EntrySourceDto,
    pub deliveries: Vec<EntryDeliveryTargetDto>,
}

/// Entry origin. `tag` drives the frontend discriminated union.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "tag", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EntrySourceDto {
    /// Captured on this device.
    Local,
    /// Pushed from a remote device. `deviceName` is resolved from the space
    /// member directory; `null` when unresolved (frontend falls back to a
    /// truncated `deviceId`).
    Remote {
        device_id: String,
        device_name: Option<String>,
    },
    /// Legacy entry predating delivery tracking — no reliable delivery info.
    Historical,
}

/// One trusted peer's current sync status for the entry.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct EntryDeliveryTargetDto {
    pub target_device_id: String,
    /// Human-readable name from the member directory; `null` when unresolved.
    pub target_device_name: Option<String>,
    pub status: EntryDeliveryStatusDto,
    /// Wire-level failure detail for UI tooltips; `null` on success / pending.
    pub reason_detail: Option<String>,
    /// `null` when `Pending` (never attempted). Epoch milliseconds.
    pub updated_at_ms: Option<i64>,
}

/// Per-target status: `tag` + (on failure) a `reason` sub-discriminator.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(tag = "tag", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum EntryDeliveryStatusDto {
    Pending,
    Delivered,
    Duplicate,
    Failed { reason: DeliveryFailureReasonDto },
}

/// Failure reason. i18n key convention: `delivery.failureReason.<variant>`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryFailureReasonDto {
    Offline,
    LocalPolicy,
    PeerRejected,
    Io,
    Internal,
}
