//! Member roster boundary projections: per-member sync preferences ↔ DTOs.
//!
//! The engine interface has its own content-type patch and summary types
//! (distinct from the settings facade's), so the shared
//! `ContentTypesPatchDto` / `ContentTypesDto` carry one impl per target here
//! in addition to the settings ones.

use uc_engine::{
    ContentTypesPatch, ContentTypesSummary, DeviceCompatibilitySummary,
    DeviceGroupChoiceOptionSummary, DeviceGroupChoiceOutcomeSummary,
    DeviceGroupChoiceResultSummary, DeviceGroupChoicesSummary, DeviceGroupRelationshipSummary,
    DeviceMembershipSummary, DeviceReachabilitySummary, DeviceSyncRelationshipSummary,
    DeviceTrustActionSummary, DeviceTrustChangeSummary, DeviceTrustChoiceSummary,
    DeviceTrustImpactSummary, DeviceTrustRecoverySummary, DeviceTrustRelationshipSummary,
    DeviceTrustSnapshotSummary, DeviceTrustUnavailableReasonSummary, MemberProtectionStatusSummary,
    MemberProtectionSummary, MemberSyncPreferencesPatch, MemberSyncPreferencesSummary,
    SpaceProtectionModeSummary, SpaceProtectionSummary,
};

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::member::{
    DeviceCompatibilityDto, DeviceGroupChoiceIssueDto, DeviceGroupChoiceOptionDto,
    DeviceGroupChoiceOutcomeDto, DeviceGroupChoiceResultDto, DeviceGroupChoicesDto,
    DeviceGroupRelationshipDto, DeviceMembershipDto, DeviceReachabilityDto,
    DeviceSyncRelationshipDto, DeviceTrustActionDto, DeviceTrustChangeDto, DeviceTrustChoiceDto,
    DeviceTrustImpactDto, DeviceTrustRelationshipDto, DeviceTrustSnapshotDto,
    DeviceTrustUnavailableReasonDto, MemberProtectionDto, MemberProtectionStatusDto,
    MemberSyncPreferencesDto, PendingInboundMemberDto, SpaceProtectionDto, SpaceProtectionModeDto,
};
use crate::api::dto::settings::{ContentTypesDto, ContentTypesPatchDto};

impl IntoDomain<MemberSyncPreferencesPatch>
    for crate::api::dto::member::MemberSyncPreferencesPatchDto
{
    fn into_domain(self) -> MemberSyncPreferencesPatch {
        MemberSyncPreferencesPatch {
            send_enabled: self.send_enabled,
            receive_enabled: self.receive_enabled,
            send_content_types: self.send_content_types.map(IntoDomain::into_domain),
            receive_content_types: self.receive_content_types.map(IntoDomain::into_domain),
        }
    }
}

impl IntoDomain<ContentTypesPatch> for ContentTypesPatchDto {
    fn into_domain(self) -> ContentTypesPatch {
        ContentTypesPatch {
            text: self.text,
            image: self.image,
            link: self.link,
            file: self.file,
            code_snippet: self.code_snippet,
            rich_text: self.rich_text,
        }
    }
}

impl IntoApiDto<ContentTypesDto> for ContentTypesSummary {
    fn into_api_dto(self) -> ContentTypesDto {
        ContentTypesDto {
            text: self.text,
            image: self.image,
            link: self.link,
            file: self.file,
            code_snippet: self.code_snippet,
            rich_text: self.rich_text,
        }
    }
}

impl IntoApiDto<MemberSyncPreferencesDto> for MemberSyncPreferencesSummary {
    fn into_api_dto(self) -> MemberSyncPreferencesDto {
        MemberSyncPreferencesDto {
            send_enabled: self.send_enabled,
            receive_enabled: self.receive_enabled,
            send_content_types: self.send_content_types.into_api_dto(),
            receive_content_types: self.receive_content_types.into_api_dto(),
        }
    }
}

impl IntoApiDto<MemberProtectionDto> for MemberProtectionSummary {
    fn into_api_dto(self) -> MemberProtectionDto {
        MemberProtectionDto {
            device_id: self.device_id,
            status: match self.status {
                MemberProtectionStatusSummary::LegacyUnprotected => {
                    MemberProtectionStatusDto::LegacyUnprotected
                }
                MemberProtectionStatusSummary::Protected => MemberProtectionStatusDto::Protected,
                MemberProtectionStatusSummary::AwaitingReadmission => {
                    MemberProtectionStatusDto::AwaitingReadmission
                }
                MemberProtectionStatusSummary::RequiresReadmission => {
                    MemberProtectionStatusDto::RequiresReadmission
                }
                MemberProtectionStatusSummary::RecoveryRequired => {
                    MemberProtectionStatusDto::RecoveryRequired
                }
            },
        }
    }
}

impl IntoApiDto<SpaceProtectionDto> for SpaceProtectionSummary {
    fn into_api_dto(self) -> SpaceProtectionDto {
        SpaceProtectionDto {
            mode: match self.mode {
                SpaceProtectionModeSummary::Legacy => SpaceProtectionModeDto::Legacy,
                SpaceProtectionModeSummary::Migrating => SpaceProtectionModeDto::Migrating,
                SpaceProtectionModeSummary::Ready => SpaceProtectionModeDto::Ready,
            },
            members: self
                .members
                .into_iter()
                .map(IntoApiDto::into_api_dto)
                .collect(),
        }
    }
}

impl IntoApiDto<DeviceTrustSnapshotDto> for DeviceTrustSnapshotSummary {
    fn into_api_dto(self) -> DeviceTrustSnapshotDto {
        DeviceTrustSnapshotDto {
            revision: self.revision,
            local_device_id: self.local_device_id,
            local_membership: device_membership(self.local_membership),
            current_change: self.current_change.map(device_trust_change),
            current_join: self
                .current_join
                .map(crate::api::v2::setup::join_space_response),
            pending_inbound_member: self.pending_inbound_member.map(|member| {
                PendingInboundMemberDto {
                    device_id: member.device_id,
                    display_name: member.display_name,
                }
            }),
            devices: self
                .devices
                .into_iter()
                .map(device_trust_relationship)
                .collect(),
            recovery: match self.recovery {
                DeviceTrustRecoverySummary::NotAvailableInThisVersion => {
                    "not_available_in_this_version".to_string()
                }
            },
            allowed_actions: self
                .allowed_actions
                .into_iter()
                .map(device_trust_action)
                .collect(),
            blocked_reason: self.blocked_reason.map(device_trust_unavailable_reason),
            updated_at_ms: self.updated_at_ms,
        }
    }
}

impl IntoApiDto<DeviceGroupChoicesDto> for DeviceGroupChoicesSummary {
    fn into_api_dto(self) -> DeviceGroupChoicesDto {
        DeviceGroupChoicesDto {
            revision: self.revision,
            device_trust: self.device_trust.into_api_dto(),
            issues: self
                .issues
                .into_iter()
                .map(|issue| DeviceGroupChoiceIssueDto {
                    issue_id: issue.issue_id,
                    choices: issue
                        .choices
                        .into_iter()
                        .map(device_group_choice_option)
                        .collect(),
                })
                .collect(),
        }
    }
}

impl IntoApiDto<DeviceGroupChoiceResultDto> for DeviceGroupChoiceResultSummary {
    fn into_api_dto(self) -> DeviceGroupChoiceResultDto {
        DeviceGroupChoiceResultDto {
            outcome: match self.outcome {
                DeviceGroupChoiceOutcomeSummary::Completed => {
                    DeviceGroupChoiceOutcomeDto::Completed
                }
                DeviceGroupChoiceOutcomeSummary::Pending => DeviceGroupChoiceOutcomeDto::Pending,
                DeviceGroupChoiceOutcomeSummary::RePairingRequired => {
                    DeviceGroupChoiceOutcomeDto::RePairingRequired
                }
                DeviceGroupChoiceOutcomeSummary::AlreadyCompleted => {
                    DeviceGroupChoiceOutcomeDto::AlreadyCompleted
                }
                DeviceGroupChoiceOutcomeSummary::StateChanged => {
                    DeviceGroupChoiceOutcomeDto::StateChanged
                }
                DeviceGroupChoiceOutcomeSummary::LocalDeviceConfirmationRequired => {
                    DeviceGroupChoiceOutcomeDto::LocalDeviceConfirmationRequired
                }
            },
            current_revision: self.current_revision,
        }
    }
}

fn device_group_choice_option(
    option: DeviceGroupChoiceOptionSummary,
) -> DeviceGroupChoiceOptionDto {
    DeviceGroupChoiceOptionDto {
        choice_id: option.choice_id,
        is_current_group: option.is_current_group,
        requires_re_pairing: option.requires_re_pairing,
        member_device_ids: option.member_device_ids,
        members_complete: option.members_complete,
    }
}

fn device_trust_change(change: DeviceTrustChangeSummary) -> DeviceTrustChangeDto {
    DeviceTrustChangeDto {
        change_id: change.change_id,
        proposed_by_device_id: change.proposed_by_device_id,
        target_device_ids: change.target_device_ids,
        includes_local_device: change.includes_local_device,
        apply_impact: device_trust_impact(change.apply_impact),
        keep_current_impact: device_trust_impact(change.keep_current_impact),
        allowed_choices: change
            .allowed_choices
            .into_iter()
            .map(device_trust_choice)
            .collect(),
        blocked_reason: change.blocked_reason.map(device_trust_unavailable_reason),
    }
}

fn device_trust_impact(impact: DeviceTrustImpactSummary) -> DeviceTrustImpactDto {
    DeviceTrustImpactDto {
        usable_device_ids: impact.usable_device_ids,
        paused_device_ids: impact.paused_device_ids,
        local_device_outcome: device_membership(impact.local_device_outcome),
        requires_rejoin_device_ids: impact.requires_rejoin_device_ids,
    }
}

fn device_trust_relationship(
    relationship: DeviceTrustRelationshipSummary,
) -> DeviceTrustRelationshipDto {
    DeviceTrustRelationshipDto {
        device_id: relationship.device_id,
        display_name: relationship.display_name,
        is_local: relationship.is_local,
        reachability: match relationship.reachability {
            DeviceReachabilitySummary::Online => DeviceReachabilityDto::Online,
            DeviceReachabilitySummary::Offline => DeviceReachabilityDto::Offline,
            DeviceReachabilitySummary::Unknown => DeviceReachabilityDto::Unknown,
        },
        membership: device_membership(relationship.membership),
        group_relationship: match relationship.group_relationship {
            DeviceGroupRelationshipSummary::Consistent => DeviceGroupRelationshipDto::Consistent,
            DeviceGroupRelationshipSummary::PendingLocalDecision => {
                DeviceGroupRelationshipDto::PendingLocalDecision
            }
            DeviceGroupRelationshipSummary::Diverged => DeviceGroupRelationshipDto::Diverged,
            DeviceGroupRelationshipSummary::Unverifiable => {
                DeviceGroupRelationshipDto::Unverifiable
            }
            DeviceGroupRelationshipSummary::Unknown => DeviceGroupRelationshipDto::Unknown,
        },
        compatibility: match relationship.compatibility {
            DeviceCompatibilitySummary::Compatible => DeviceCompatibilityDto::Compatible,
            DeviceCompatibilitySummary::UpgradeRequired => DeviceCompatibilityDto::UpgradeRequired,
            DeviceCompatibilitySummary::Unknown => DeviceCompatibilityDto::Unknown,
        },
        sync_relationship: match relationship.sync_relationship {
            DeviceSyncRelationshipSummary::Usable => DeviceSyncRelationshipDto::Usable,
            DeviceSyncRelationshipSummary::WaitingForLocalDecision => {
                DeviceSyncRelationshipDto::WaitingForLocalDecision
            }
            DeviceSyncRelationshipSummary::PausedGroupDiverged => {
                DeviceSyncRelationshipDto::PausedGroupDiverged
            }
            DeviceSyncRelationshipSummary::PausedUpgradeRequired => {
                DeviceSyncRelationshipDto::PausedUpgradeRequired
            }
            DeviceSyncRelationshipSummary::PausedUnverifiable => {
                DeviceSyncRelationshipDto::PausedUnverifiable
            }
            DeviceSyncRelationshipSummary::RemovedLocalDevice => {
                DeviceSyncRelationshipDto::RemovedLocalDevice
            }
            DeviceSyncRelationshipSummary::RemovedPeerDevice => {
                DeviceSyncRelationshipDto::RemovedPeerDevice
            }
            DeviceSyncRelationshipSummary::Unknown => DeviceSyncRelationshipDto::Unknown,
        },
        available_actions: relationship
            .available_actions
            .into_iter()
            .map(device_trust_action)
            .collect(),
        blocked_reason: relationship
            .blocked_reason
            .map(device_trust_unavailable_reason),
    }
}

fn device_membership(value: DeviceMembershipSummary) -> DeviceMembershipDto {
    match value {
        DeviceMembershipSummary::Active => DeviceMembershipDto::Active,
        DeviceMembershipSummary::Removed => DeviceMembershipDto::Removed,
        DeviceMembershipSummary::Unavailable => DeviceMembershipDto::Unavailable,
        DeviceMembershipSummary::Unknown => DeviceMembershipDto::Unknown,
    }
}

fn device_trust_choice(value: DeviceTrustChoiceSummary) -> DeviceTrustChoiceDto {
    match value {
        DeviceTrustChoiceSummary::ApplyChange => DeviceTrustChoiceDto::ApplyChange,
        DeviceTrustChoiceSummary::KeepCurrentDeviceGroup => {
            DeviceTrustChoiceDto::KeepCurrentDeviceGroup
        }
    }
}

fn device_trust_action(value: DeviceTrustActionSummary) -> DeviceTrustActionDto {
    match value {
        DeviceTrustActionSummary::ApplyCurrentChange => DeviceTrustActionDto::ApplyCurrentChange,
        DeviceTrustActionSummary::KeepCurrentDeviceGroup => {
            DeviceTrustActionDto::KeepCurrentDeviceGroup
        }
        DeviceTrustActionSummary::ConfirmApplyRemovesLocalDevice => {
            DeviceTrustActionDto::ConfirmApplyRemovesLocalDevice
        }
        DeviceTrustActionSummary::RejoinDeviceGroup => DeviceTrustActionDto::RejoinDeviceGroup,
        DeviceTrustActionSummary::UpdateThisDevice => DeviceTrustActionDto::UpdateThisDevice,
    }
}

fn device_trust_unavailable_reason(
    value: DeviceTrustUnavailableReasonSummary,
) -> DeviceTrustUnavailableReasonDto {
    match value {
        DeviceTrustUnavailableReasonSummary::NoCurrentChange => {
            DeviceTrustUnavailableReasonDto::NoCurrentChange
        }
        DeviceTrustUnavailableReasonSummary::ChangeNoLongerCurrent => {
            DeviceTrustUnavailableReasonDto::ChangeNoLongerCurrent
        }
        DeviceTrustUnavailableReasonSummary::LocalDeviceConfirmationRequired => {
            DeviceTrustUnavailableReasonDto::LocalDeviceConfirmationRequired
        }
        DeviceTrustUnavailableReasonSummary::LocalDeviceRemoved => {
            DeviceTrustUnavailableReasonDto::LocalDeviceRemoved
        }
        DeviceTrustUnavailableReasonSummary::RecoveryNotAvailableInThisVersion => {
            DeviceTrustUnavailableReasonDto::RecoveryNotAvailableInThisVersion
        }
        DeviceTrustUnavailableReasonSummary::PeerUpgradeRequired => {
            DeviceTrustUnavailableReasonDto::PeerUpgradeRequired
        }
        DeviceTrustUnavailableReasonSummary::DeviceFactsUnverifiable => {
            DeviceTrustUnavailableReasonDto::DeviceFactsUnverifiable
        }
        DeviceTrustUnavailableReasonSummary::EngineUnavailable => {
            DeviceTrustUnavailableReasonDto::EngineUnavailable
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dto::member::{DeviceTrustSnapshotDto, MemberSyncPreferencesPatchDto};
    use uc_engine::DeviceGroupChoiceIssueSummary;

    #[test]
    fn patch_mapping_preserves_omitted_fields_as_none() {
        let patch = MemberSyncPreferencesPatchDto {
            send_enabled: Some(false),
            receive_enabled: None,
            send_content_types: None,
            receive_content_types: None,
        };
        let mapped: MemberSyncPreferencesPatch = patch.into_domain();

        assert_eq!(mapped.send_enabled, Some(false));
        assert_eq!(mapped.receive_enabled, None);
        assert!(mapped.send_content_types.is_none());
        assert!(mapped.receive_content_types.is_none());
    }

    #[test]
    fn patch_mapping_keeps_partial_content_type_shape() {
        let patch = MemberSyncPreferencesPatchDto {
            send_enabled: None,
            receive_enabled: None,
            send_content_types: Some(ContentTypesPatchDto {
                text: Some(true),
                image: None,
                link: None,
                file: None,
                code_snippet: None,
                rich_text: None,
            }),
            receive_content_types: None,
        };
        let mapped: MemberSyncPreferencesPatch = patch.into_domain();
        let send = mapped.send_content_types.expect("send patch");
        assert_eq!(send.text, Some(true));
        assert_eq!(send.image, None);
    }

    #[test]
    fn device_trust_snapshot_preserves_engine_wire_fields() {
        let summary = DeviceTrustSnapshotSummary {
            revision: 7,
            local_device_id: "device-local".to_string(),
            local_membership: DeviceMembershipSummary::Active,
            current_change: Some(DeviceTrustChangeSummary {
                change_id: "change-1".to_string(),
                proposed_by_device_id: "device-d".to_string(),
                target_device_ids: vec!["device-local".to_string()],
                includes_local_device: true,
                apply_impact: DeviceTrustImpactSummary {
                    usable_device_ids: vec!["device-d".to_string()],
                    paused_device_ids: Vec::new(),
                    local_device_outcome: DeviceMembershipSummary::Removed,
                    requires_rejoin_device_ids: vec!["device-local".to_string()],
                },
                keep_current_impact: DeviceTrustImpactSummary {
                    usable_device_ids: vec!["device-local".to_string()],
                    paused_device_ids: vec!["device-d".to_string()],
                    local_device_outcome: DeviceMembershipSummary::Active,
                    requires_rejoin_device_ids: Vec::new(),
                },
                allowed_choices: vec![
                    DeviceTrustChoiceSummary::ApplyChange,
                    DeviceTrustChoiceSummary::KeepCurrentDeviceGroup,
                ],
                blocked_reason: Some(
                    DeviceTrustUnavailableReasonSummary::LocalDeviceConfirmationRequired,
                ),
            }),
            current_join: None,
            pending_inbound_member: None,
            devices: Vec::new(),
            recovery: DeviceTrustRecoverySummary::NotAvailableInThisVersion,
            allowed_actions: vec![DeviceTrustActionSummary::KeepCurrentDeviceGroup],
            blocked_reason: Some(
                DeviceTrustUnavailableReasonSummary::LocalDeviceConfirmationRequired,
            ),
            updated_at_ms: 42,
        };

        let mapped: DeviceTrustSnapshotDto = summary.into_api_dto();

        assert_eq!(mapped.local_device_id, "device-local");
        assert_eq!(mapped.revision, 7);
        let change = mapped.current_change.expect("pending device trust change");
        assert_eq!(change.change_id, "change-1");
        assert!(change.includes_local_device);
        assert_eq!(change.target_device_ids, vec!["device-local"]);
        assert_eq!(
            change.apply_impact.local_device_outcome,
            DeviceMembershipDto::Removed
        );
    }

    #[test]
    fn device_group_choices_preserve_engine_owned_ids_and_revision() {
        let summary = DeviceGroupChoicesSummary {
            revision: 9,
            device_trust: DeviceTrustSnapshotSummary::empty_unavailable("device-local".to_string()),
            issues: vec![DeviceGroupChoiceIssueSummary {
                issue_id: "c:issue-1".to_string(),
                choices: vec![DeviceGroupChoiceOptionSummary {
                    choice_id: "b:choice-1".to_string(),
                    is_current_group: false,
                    requires_re_pairing: true,
                    member_device_ids: vec!["device-a".to_string(), "device-b".to_string()],
                    members_complete: true,
                }],
            }],
        };

        let mapped: DeviceGroupChoicesDto = summary.into_api_dto();

        assert_eq!(mapped.revision, 9);
        assert_eq!(mapped.issues[0].issue_id, "c:issue-1");
        assert_eq!(mapped.issues[0].choices[0].choice_id, "b:choice-1");
        assert!(mapped.issues[0].choices[0].requires_re_pairing);
    }
}
