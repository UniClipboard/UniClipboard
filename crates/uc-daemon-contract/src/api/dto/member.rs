//! DTOs for per-member sync preferences (phase 4b PR-2).
//!
//! 语义：映射 `SpaceMember.sync_preferences`（双向 `send_enabled` /
//! `receive_enabled` + 双套 `content_types`）。复用 `dto::settings` 下的
//! `ContentTypesDto` / `ContentTypesPatchDto`，两套字段形状一致。

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

use super::settings::{ContentTypesDto, ContentTypesPatchDto};
use super::v2::setup::JoinSpaceResponse;

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

/// Current phase of the Engine-owned workspace convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceConvergencePhaseDto {
    LocallyApplied,
    Converging,
    Complete,
    RecoveryRequired,
}

/// Stable failure category for workspace convergence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceConvergenceFailureCategoryDto {
    SpaceMismatch,
    ContinuityGap,
    IdentityMismatch,
    DigestConflict,
    Unauthorized,
    VersionIncompatible,
    NoEffectiveMembers,
    Storage,
}

/// Complete Engine-owned workspace convergence state for the active space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceConvergenceDto {
    pub phase: WorkspaceConvergencePhaseDto,
    pub revision: u64,
    pub history_event_count: u64,
    pub effective_member_count: u64,
    pub pending_removal_decision_device_ids: Vec<String>,
    pub pending_removal_decision_event_id: Option<String>,
    pub diverged_peer_device_ids: Vec<String>,
    pub upgrade_required_peer_device_ids: Vec<String>,
    pub convergence_digest: Option<String>,
    pub updated_at_ms: i64,
    pub removed: bool,
    pub failure_category: Option<WorkspaceConvergenceFailureCategoryDto>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceMembershipDto {
    Active,
    Removed,
    Unavailable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceReachabilityDto {
    Online,
    Offline,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupRelationshipDto {
    Consistent,
    PendingLocalDecision,
    Diverged,
    Unverifiable,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceCompatibilityDto {
    Compatible,
    UpgradeRequired,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSyncRelationshipDto {
    Usable,
    WaitingForLocalDecision,
    PausedGroupDiverged,
    PausedUpgradeRequired,
    PausedUnverifiable,
    RemovedLocalDevice,
    RemovedPeerDevice,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceTrustChoiceDto {
    ApplyChange,
    KeepCurrentDeviceGroup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceTrustActionDto {
    ApplyCurrentChange,
    KeepCurrentDeviceGroup,
    ConfirmApplyRemovesLocalDevice,
    RejoinDeviceGroup,
    UpdateThisDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceTrustUnavailableReasonDto {
    NoCurrentChange,
    ChangeNoLongerCurrent,
    LocalDeviceConfirmationRequired,
    LocalDeviceRemoved,
    RecoveryNotAvailableInThisVersion,
    PeerUpgradeRequired,
    DeviceFactsUnverifiable,
    EngineUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTrustImpactDto {
    pub usable_device_ids: Vec<String>,
    pub paused_device_ids: Vec<String>,
    pub local_device_outcome: DeviceMembershipDto,
    pub requires_rejoin_device_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTrustChangeDto {
    pub change_id: String,
    pub proposed_by_device_id: String,
    pub target_device_ids: Vec<String>,
    pub includes_local_device: bool,
    pub apply_impact: DeviceTrustImpactDto,
    pub keep_current_impact: DeviceTrustImpactDto,
    pub allowed_choices: Vec<DeviceTrustChoiceDto>,
    pub blocked_reason: Option<DeviceTrustUnavailableReasonDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTrustRelationshipDto {
    pub device_id: String,
    pub display_name: String,
    pub is_local: bool,
    pub reachability: DeviceReachabilityDto,
    pub membership: DeviceMembershipDto,
    pub group_relationship: DeviceGroupRelationshipDto,
    pub compatibility: DeviceCompatibilityDto,
    pub sync_relationship: DeviceSyncRelationshipDto,
    pub available_actions: Vec<DeviceTrustActionDto>,
    pub blocked_reason: Option<DeviceTrustUnavailableReasonDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PendingInboundMemberDto {
    pub device_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTrustSnapshotDto {
    pub revision: u64,
    pub local_device_id: String,
    pub local_membership: DeviceMembershipDto,
    pub current_change: Option<DeviceTrustChangeDto>,
    pub current_join: Option<JoinSpaceResponse>,
    pub pending_inbound_member: Option<PendingInboundMemberDto>,
    pub devices: Vec<DeviceTrustRelationshipDto>,
    pub recovery: String,
    pub allowed_actions: Vec<DeviceTrustActionDto>,
    pub blocked_reason: Option<DeviceTrustUnavailableReasonDto>,
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DecideDeviceTrustRequestDto {
    pub change_id: String,
    pub choice: DeviceTrustChoiceDto,
    #[serde(default)]
    pub confirm_local_removal: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    tag = "kind"
)]
pub enum DeviceTrustDecisionDto {
    Applied {
        #[schema(rename = "changeId")]
        change_id: String,
        snapshot: DeviceTrustSnapshotDto,
    },
    KeptCurrentDeviceGroup {
        #[schema(rename = "changeId")]
        change_id: String,
        snapshot: DeviceTrustSnapshotDto,
    },
    AlreadyCompleted {
        #[schema(rename = "changeId")]
        change_id: String,
        #[schema(rename = "completedChoice")]
        completed_choice: DeviceTrustChoiceDto,
        snapshot: DeviceTrustSnapshotDto,
    },
    StateChanged {
        #[schema(rename = "currentChangeId")]
        current_change_id: Option<String>,
        snapshot: DeviceTrustSnapshotDto,
    },
    LocalDeviceConfirmationRequired {
        #[schema(rename = "changeId")]
        change_id: String,
        snapshot: DeviceTrustSnapshotDto,
    },
}

#[cfg(test)]
mod device_trust_decision_dto_tests {
    use serde_json::json;

    use super::*;

    fn snapshot() -> DeviceTrustSnapshotDto {
        DeviceTrustSnapshotDto {
            revision: 1,
            local_device_id: "local-device".to_string(),
            local_membership: DeviceMembershipDto::Active,
            current_change: None,
            current_join: None,
            pending_inbound_member: None,
            devices: Vec::new(),
            recovery: "not_available_in_this_version".to_string(),
            allowed_actions: Vec::new(),
            blocked_reason: None,
            updated_at_ms: 1,
        }
    }

    #[test]
    fn serializes_struct_variant_fields_in_camel_case() {
        let value = serde_json::to_value(DeviceTrustDecisionDto::AlreadyCompleted {
            change_id: "change-1".to_string(),
            completed_choice: DeviceTrustChoiceDto::ApplyChange,
            snapshot: snapshot(),
        })
        .expect("serialize device trust decision");

        assert_eq!(value["changeId"], json!("change-1"));
        assert_eq!(value["completedChoice"], json!("apply_change"));
        assert!(
            value.get("change_id").is_none(),
            "legacy snake_case field leaked: {value}"
        );
        assert!(
            value.get("completed_choice").is_none(),
            "legacy snake_case field leaked: {value}"
        );
    }
}
