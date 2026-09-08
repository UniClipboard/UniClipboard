use super::DeviceMembershipDto;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceDeviceDto {
    pub device_id: String,
    pub display_name: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceMemberDto {
    pub device_id: String,
    pub display_name: String,
    pub is_local: bool,
    pub active: bool,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceImpactDto {
    /// Expected sync scope; not authorization or completed recovery.
    pub sync_scope_device_ids: Vec<String>,
    pub paused_device_ids: Vec<String>,
    pub pending_confirmation_device_ids: Vec<String>,
    pub requires_rejoin_device_ids: Vec<String>,
    pub local_device_outcome: DeviceMembershipDto,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupChoiceReasonKindDto {
    #[default]
    Unknown,
    PendingRemoval,
    DifferentRemovals,
    RemovalDecisionDisagreement,
    DivergedHistory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupChangeSideDto {
    Local,
    Remote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupChangeKindDto {
    AddedDevice,
    RemovedDevice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum DeviceGroupRemovalDecisionDto {
    Accepted,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChangeDto {
    pub side: DeviceGroupChangeSideDto,
    pub kind: DeviceGroupChangeKindDto,
    pub actor: DeviceGroupChoiceDeviceDto,
    pub target: DeviceGroupChoiceDeviceDto,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupDecisionDto {
    pub device: DeviceGroupChoiceDeviceDto,
    pub decision: DeviceGroupRemovalDecisionDto,
    pub target: DeviceGroupChoiceDeviceDto,
}

/// Verified language-neutral facts. Products localize templates, not device names.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DeviceGroupChoiceReasonDto {
    pub kind: DeviceGroupChoiceReasonKindDto,
    pub changes: Vec<DeviceGroupChangeDto>,
    pub decisions: Vec<DeviceGroupDecisionDto>,
    pub details_complete: bool,
}

impl std::fmt::Debug for DeviceGroupChoiceDeviceDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DeviceGroupChoiceDeviceDto(REDACTED)")
    }
}

impl std::fmt::Debug for DeviceGroupChoiceMemberDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGroupChoiceMemberDto")
            .field("is_local", &self.is_local)
            .field("active", &self.active)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for DeviceGroupChoiceImpactDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGroupChoiceImpactDto")
            .field("scope_count", &self.sync_scope_device_ids.len())
            .field("paused_count", &self.paused_device_ids.len())
            .field("pending_count", &self.pending_confirmation_device_ids.len())
            .field("local_device_outcome", &self.local_device_outcome)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for super::DeviceGroupChoiceOptionDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGroupChoiceOptionDto")
            .field("is_current_group", &self.is_current_group)
            .field("members_complete", &self.members_complete)
            .field("member_count", &self.members.len())
            .field("requires_re_pairing", &self.requires_re_pairing)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for super::DeviceGroupChoiceIssueDto {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceGroupChoiceIssueDto")
            .field("choice_count", &self.choices.len())
            .field("reason", &self.reason.kind)
            .finish_non_exhaustive()
    }
}
