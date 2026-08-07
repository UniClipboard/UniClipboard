//! DTOs for per-member sync preferences (phase 4b PR-2).
//!
//! 语义：映射 `SpaceMember.sync_preferences`（双向 `send_enabled` /
//! `receive_enabled` + 双套 `content_types`）。复用 `dto::settings` 下的
//! `ContentTypesDto` / `ContentTypesPatchDto`，两套字段形状一致。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::{ContentTypesDto, ContentTypesPatchDto};

/// Sync preferences recorded for a space member.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncPreferencesDto {
    pub send_enabled: bool,
    pub receive_enabled: bool,
    pub send_content_types: ContentTypesDto,
    pub receive_content_types: ContentTypesDto,
}

/// Partial sync preferences for PATCH /member/:device_id/sync-preferences.
///
/// 服务器侧 `get → merge → save` 后持久化；未提供的字段保留当前值。
/// 重置到默认值的调用方应显式传入所有字段的默认值。
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncPreferencesPatchDto {
    pub send_enabled: Option<bool>,
    pub receive_enabled: Option<bool>,
    pub send_content_types: Option<ContentTypesPatchDto>,
    pub receive_content_types: Option<ContentTypesPatchDto>,
}

/// Folded payload for `PATCH /member/:device_id/sync-preferences` (ADR-008 §0.1).
///
/// The current handler returns `success` as a top-level sibling of the
/// `{data,ts}` envelope. This DTO folds it INTO the payload so the endpoint can
/// return `ApiEnvelope<MemberSyncResultDto>` with no bespoke wrapper. P1 only
/// defines the type; the handler is rewired in P2.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberSyncResultDto {
    pub success: bool,
}

/// Engine-authoritative state of a Legacy-to-MLS space protection upgrade.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SpaceProtectionModeDto {
    Legacy,
    Migrating,
    Ready,
}

/// Protection state of one roster member in the current space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberProtectionStatusDto {
    LegacyUnprotected,
    Protected,
    AwaitingReadmission,
    RequiresReadmission,
    RecoveryRequired,
}

/// Recoverable Legacy bootstrap progress, owned and persisted by the Engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum LegacyBootstrapOutcomeDto {
    AwaitingReadmission,
    Complete,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LegacyBootstrapDto {
    pub bootstrap_id: String,
    pub outcome: LegacyBootstrapOutcomeDto,
    pub pending_readmission: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberProtectionDto {
    pub device_id: String,
    pub status: MemberProtectionStatusDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpaceProtectionDto {
    pub mode: SpaceProtectionModeDto,
    pub members: Vec<MemberProtectionDto>,
    pub legacy_bootstrap: Option<LegacyBootstrapDto>,
}

/// Coarse connection state for the active space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MembershipConvergenceStateDto {
    Complete,
    Converging,
    WaitingForUpgrade,
    Blocked,
}

/// Product-facing connection status. Engine-internal counters stay private.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MembershipConvergenceDto {
    pub state: MembershipConvergenceStateDto,
}

/// Result of starting a shared-device refresh round.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharedDeviceRefreshStartedDto {
    pub request_id: String,
}

/// Current phase of an Engine-owned shared-device refresh round.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedDeviceRefreshPhaseDto {
    Started,
    Discovering,
    Connecting,
    RoundCompleted,
}

/// Stable per-device state produced by the Engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum SharedDeviceRefreshDeviceStateDto {
    Discovered,
    Connecting,
    Connected,
    AlreadyPresent,
    WaitingForPeer,
    WaitingForUpdate,
    VersionIncompatible,
    Rejected,
}

/// One device row in a shared-device refresh snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharedDeviceRefreshDeviceDto {
    pub device_id: String,
    pub display_name: String,
    pub state: SharedDeviceRefreshDeviceStateDto,
}

/// Complete Engine-owned result for one shared-device refresh request.
///
/// Counts are mirrored verbatim from the Engine. The daemon and frontend must
/// not derive or correct device states from counts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SharedDeviceRefreshDto {
    pub request_id: String,
    pub phase: SharedDeviceRefreshPhaseDto,
    pub devices: Vec<SharedDeviceRefreshDeviceDto>,
    pub total_count: u64,
    pub discovered_count: u64,
    pub connecting_count: u64,
    pub connected_count: u64,
    pub already_present_count: u64,
    pub waiting_for_peer_count: u64,
    pub waiting_for_update_count: u64,
    pub version_incompatible_count: u64,
    pub rejected_count: u64,
    pub unavailable_source_count: u64,
}

/// Result of starting a secure Legacy member removal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SecureLegacyRemovalDto {
    pub bootstrap: LegacyBootstrapDto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum MemberRemovalOutcomeDto {
    LocalOnly,
    Recovering,
    Applied,
    Complete,
    RecoveryRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct MemberRemovalDto {
    pub revocation_id: Option<String>,
    pub outcome: MemberRemovalOutcomeDto,
    pub pending_recipients: u64,
    pub removed_device_ids: Vec<String>,
    pub pending_recipient_device_ids: Vec<String>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ContinueMemberRemovalRequest {
    pub revocation_id: String,
    pub permanently_lost_device_ids: Vec<String>,
}
