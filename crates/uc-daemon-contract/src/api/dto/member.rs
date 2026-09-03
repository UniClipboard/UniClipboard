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
pub struct DeviceGroupChoicesDto {
    pub revision: u64,
    pub device_trust: DeviceTrustSnapshotDto,
    pub issues: Vec<DeviceGroupChoiceIssueDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceIssueDto {
    pub issue_id: String,
    pub choices: Vec<DeviceGroupChoiceOptionDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceOptionDto {
    pub choice_id: String,
    pub is_current_group: bool,
    pub requires_re_pairing: bool,
    pub member_device_ids: Vec<String>,
    pub members_complete: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct ChooseDeviceGroupRequestDto {
    pub issue_id: String,
    pub choice_id: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub confirm_local_removal: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupChoiceOutcomeDto {
    Completed,
    Pending,
    RePairingRequired,
    AlreadyCompleted,
    StateChanged,
    LocalDeviceConfirmationRequired,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceResultDto {
    pub outcome: DeviceGroupChoiceOutcomeDto,
    pub current_revision: Option<u64>,
}

#[cfg(test)]
mod device_group_choice_dto_tests {
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
    fn query_response_preserves_opaque_issue_and_choice_ids() {
        let value = serde_json::to_value(DeviceGroupChoicesDto {
            revision: 7,
            device_trust: snapshot(),
            issues: vec![DeviceGroupChoiceIssueDto {
                issue_id: "p:issue-1".to_string(),
                choices: vec![DeviceGroupChoiceOptionDto {
                    choice_id: "keep".to_string(),
                    is_current_group: true,
                    requires_re_pairing: false,
                    member_device_ids: vec!["device-1".to_string()],
                    members_complete: true,
                }],
            }],
        })
        .expect("serialize device group choices");

        assert_eq!(value["revision"], 7);
        assert_eq!(value["deviceTrust"]["localDeviceId"], "local-device");
        assert_eq!(value["issues"][0]["issueId"], "p:issue-1");
        assert_eq!(value["issues"][0]["choices"][0]["choiceId"], "keep");
    }

    #[test]
    fn choice_request_and_result_use_the_current_query_revision() {
        let request = serde_json::to_value(ChooseDeviceGroupRequestDto {
            issue_id: "c:issue-1".to_string(),
            choice_id: "b:choice-1".to_string(),
            expected_revision: 9,
            confirm_local_removal: true,
        })
        .expect("serialize device group choice request");
        let result = serde_json::to_value(DeviceGroupChoiceResultDto {
            outcome: DeviceGroupChoiceOutcomeDto::StateChanged,
            current_revision: Some(10),
        })
        .expect("serialize device group choice result");

        assert_eq!(
            request,
            json!({
                "issueId": "c:issue-1",
                "choiceId": "b:choice-1",
                "expectedRevision": 9,
                "confirmLocalRemoval": true,
            })
        );
        assert_eq!(
            result,
            json!({
                "outcome": "state_changed",
                "currentRevision": 10,
            })
        );
    }
}
