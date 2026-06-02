//! DTOs for the upgrade detection API endpoints.
//!
//! See `uc-application::facade::upgrade` for the underlying use case
//! semantics. The wire format mirrors `UpgradeStatus` with a discriminator
//! field `kind` so the frontend can switch on it.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Discriminated union mirroring `uc_application::facade::UpgradeStatus`.
///
/// Wire encoding uses `kind` discriminator with snake_case variants to
/// keep parity with the CLI JSON output produced by `uniclip upgrade
/// status --json`.
///
/// 防御性补丁(issue #606 followup):同时声明 `rename_all_fields`,
/// 避免未来新增多词字段(如 `target_version`)时 wire 字段名漂回
/// snake_case 与上层契约不一致。当前字段都是单词,加这个对 wire 无影响,
/// 但锁定未来添加字段的默认风格。详见 `docs/agent/rust-tauri-rules.md`
/// 的 "Enum Wire Serialization" 一节。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "snake_case"
)]
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

/// Payload for `POST /upgrade/ack`, wrapped by `AckUpgradeEnvelope`
/// (`ApiEnvelope<AckUpgradePayload>`).
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct AckUpgradePayload {
    pub acknowledged: String,
}
