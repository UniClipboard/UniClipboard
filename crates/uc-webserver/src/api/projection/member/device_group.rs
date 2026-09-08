use uc_daemon_contract::api::dto::member as dto;
use uc_engine as engine;

fn device(value: engine::DeviceGroupChoiceDeviceSummary) -> dto::DeviceGroupChoiceDeviceDto {
    dto::DeviceGroupChoiceDeviceDto {
        device_id: value.device_id,
        display_name: value.display_name,
    }
}

pub(super) fn reason(
    value: engine::DeviceGroupChoiceReasonSummary,
) -> dto::DeviceGroupChoiceReasonDto {
    use dto::DeviceGroupChoiceReasonKindDto as D;
    use engine::DeviceGroupChoiceReasonKind as E;
    dto::DeviceGroupChoiceReasonDto {
        kind: match value.kind {
            E::Unknown => D::Unknown,
            E::PendingRemoval => D::PendingRemoval,
            E::DifferentRemovals => D::DifferentRemovals,
            E::RemovalDecisionDisagreement => D::RemovalDecisionDisagreement,
            E::DivergedHistory => D::DivergedHistory,
        },
        changes: value
            .changes
            .into_iter()
            .map(|change| dto::DeviceGroupChangeDto {
                side: match change.side {
                    engine::DeviceGroupChangeSideSummary::Local => {
                        dto::DeviceGroupChangeSideDto::Local
                    }
                    engine::DeviceGroupChangeSideSummary::Remote => {
                        dto::DeviceGroupChangeSideDto::Remote
                    }
                },
                kind: match change.kind {
                    engine::DeviceGroupChangeKindSummary::AddedDevice => {
                        dto::DeviceGroupChangeKindDto::AddedDevice
                    }
                    engine::DeviceGroupChangeKindSummary::RemovedDevice => {
                        dto::DeviceGroupChangeKindDto::RemovedDevice
                    }
                },
                actor: device(change.actor),
                target: device(change.target),
            })
            .collect(),
        decisions: value
            .decisions
            .into_iter()
            .map(|decision| dto::DeviceGroupDecisionDto {
                device: device(decision.device),
                target: device(decision.target),
                decision: match decision.decision {
                    engine::DeviceGroupRemovalDecisionSummary::Accepted => {
                        dto::DeviceGroupRemovalDecisionDto::Accepted
                    }
                    engine::DeviceGroupRemovalDecisionSummary::Rejected => {
                        dto::DeviceGroupRemovalDecisionDto::Rejected
                    }
                },
            })
            .collect(),
        details_complete: value.details_complete,
    }
}

pub(super) fn option(
    value: engine::DeviceGroupChoiceOptionSummary,
) -> dto::DeviceGroupChoiceOptionDto {
    dto::DeviceGroupChoiceOptionDto {
        choice_id: value.choice_id,
        is_current_group: value.is_current_group,
        requires_re_pairing: value.requires_re_pairing,
        member_device_ids: value.member_device_ids,
        members_complete: value.members_complete,
        members: value
            .members
            .into_iter()
            .map(|member| dto::DeviceGroupChoiceMemberDto {
                device_id: member.device_id,
                display_name: member.display_name,
                is_local: member.is_local,
                active: member.active,
            })
            .collect(),
        source_device_ids: value.source_device_ids,
        impact: value.impact.map(|impact| dto::DeviceGroupChoiceImpactDto {
            sync_scope_device_ids: impact.sync_scope_device_ids,
            paused_device_ids: impact.paused_device_ids,
            pending_confirmation_device_ids: impact.pending_confirmation_device_ids,
            requires_rejoin_device_ids: impact.requires_rejoin_device_ids,
            local_device_outcome: super::device_membership(impact.local_device_outcome),
        }),
    }
}
