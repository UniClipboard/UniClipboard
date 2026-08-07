//! Member roster boundary projections: per-member sync preferences ↔ DTOs.
//!
//! The engine interface has its own content-type patch and summary types
//! (distinct from the settings facade's), so the shared
//! `ContentTypesPatchDto` / `ContentTypesDto` carry one impl per target here
//! in addition to the settings ones.

use uc_engine::{
    ContentTypesPatch, ContentTypesSummary, LegacyBootstrapOutcome, LegacyBootstrapSummary,
    MemberProtectionStatusSummary, MemberProtectionSummary, MemberRevocationOutcome,
    MemberRevocationSummary, MemberSyncPreferencesPatch, MemberSyncPreferencesSummary,
    MembershipConvergenceStateSummary, MembershipConvergenceSummary,
    SharedDeviceRefreshDeviceStateSummary, SharedDeviceRefreshDeviceSummary,
    SharedDeviceRefreshPhaseSummary, SharedDeviceRefreshStartedSummary, SharedDeviceRefreshSummary,
    SpaceProtectionModeSummary, SpaceProtectionSummary,
};

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::member::{
    LegacyBootstrapDto, LegacyBootstrapOutcomeDto, MemberProtectionDto, MemberProtectionStatusDto,
    MemberRemovalDto, MemberRemovalOutcomeDto, MemberSyncPreferencesDto, MembershipConvergenceDto,
    MembershipConvergenceStateDto, SharedDeviceRefreshDeviceDto, SharedDeviceRefreshDeviceStateDto,
    SharedDeviceRefreshDto, SharedDeviceRefreshPhaseDto, SharedDeviceRefreshStartedDto,
    SpaceProtectionDto, SpaceProtectionModeDto,
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

impl IntoApiDto<LegacyBootstrapDto> for LegacyBootstrapSummary {
    fn into_api_dto(self) -> LegacyBootstrapDto {
        LegacyBootstrapDto {
            bootstrap_id: self.bootstrap_id,
            outcome: match self.outcome {
                LegacyBootstrapOutcome::AwaitingReadmission => {
                    LegacyBootstrapOutcomeDto::AwaitingReadmission
                }
                LegacyBootstrapOutcome::Complete => LegacyBootstrapOutcomeDto::Complete,
                LegacyBootstrapOutcome::RecoveryRequired => {
                    LegacyBootstrapOutcomeDto::RecoveryRequired
                }
            },
            pending_readmission: self.pending_readmission,
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
            legacy_bootstrap: self.legacy_bootstrap.map(IntoApiDto::into_api_dto),
        }
    }
}

impl IntoApiDto<MembershipConvergenceDto> for MembershipConvergenceSummary {
    fn into_api_dto(self) -> MembershipConvergenceDto {
        MembershipConvergenceDto {
            state: match self.state {
                MembershipConvergenceStateSummary::Complete => {
                    MembershipConvergenceStateDto::Complete
                }
                MembershipConvergenceStateSummary::Converging => {
                    MembershipConvergenceStateDto::Converging
                }
                MembershipConvergenceStateSummary::WaitingForUpgrade => {
                    MembershipConvergenceStateDto::WaitingForUpgrade
                }
                MembershipConvergenceStateSummary::Blocked => {
                    MembershipConvergenceStateDto::Blocked
                }
            },
        }
    }
}

impl IntoApiDto<SharedDeviceRefreshStartedDto> for SharedDeviceRefreshStartedSummary {
    fn into_api_dto(self) -> SharedDeviceRefreshStartedDto {
        SharedDeviceRefreshStartedDto {
            request_id: self.request_id,
        }
    }
}

impl IntoApiDto<SharedDeviceRefreshDto> for SharedDeviceRefreshSummary {
    fn into_api_dto(self) -> SharedDeviceRefreshDto {
        SharedDeviceRefreshDto {
            request_id: self.request_id,
            phase: match self.phase {
                SharedDeviceRefreshPhaseSummary::Started => SharedDeviceRefreshPhaseDto::Started,
                SharedDeviceRefreshPhaseSummary::Discovering => {
                    SharedDeviceRefreshPhaseDto::Discovering
                }
                SharedDeviceRefreshPhaseSummary::Connecting => {
                    SharedDeviceRefreshPhaseDto::Connecting
                }
                SharedDeviceRefreshPhaseSummary::RoundCompleted => {
                    SharedDeviceRefreshPhaseDto::RoundCompleted
                }
            },
            devices: self
                .devices
                .into_iter()
                .map(
                    |device: SharedDeviceRefreshDeviceSummary| SharedDeviceRefreshDeviceDto {
                        device_id: device.device_id,
                        display_name: device.display_name,
                        state: match device.state {
                            SharedDeviceRefreshDeviceStateSummary::Discovered => {
                                SharedDeviceRefreshDeviceStateDto::Discovered
                            }
                            SharedDeviceRefreshDeviceStateSummary::Connecting => {
                                SharedDeviceRefreshDeviceStateDto::Connecting
                            }
                            SharedDeviceRefreshDeviceStateSummary::Connected => {
                                SharedDeviceRefreshDeviceStateDto::Connected
                            }
                            SharedDeviceRefreshDeviceStateSummary::AlreadyPresent => {
                                SharedDeviceRefreshDeviceStateDto::AlreadyPresent
                            }
                            SharedDeviceRefreshDeviceStateSummary::WaitingForPeer => {
                                SharedDeviceRefreshDeviceStateDto::WaitingForPeer
                            }
                            SharedDeviceRefreshDeviceStateSummary::WaitingForUpdate => {
                                SharedDeviceRefreshDeviceStateDto::WaitingForUpdate
                            }
                            SharedDeviceRefreshDeviceStateSummary::VersionIncompatible => {
                                SharedDeviceRefreshDeviceStateDto::VersionIncompatible
                            }
                            SharedDeviceRefreshDeviceStateSummary::Rejected => {
                                SharedDeviceRefreshDeviceStateDto::Rejected
                            }
                        },
                    },
                )
                .collect(),
            total_count: self.total_count,
            discovered_count: self.discovered_count,
            connecting_count: self.connecting_count,
            connected_count: self.connected_count,
            already_present_count: self.already_present_count,
            waiting_for_peer_count: self.waiting_for_peer_count,
            waiting_for_update_count: self.waiting_for_update_count,
            version_incompatible_count: self.version_incompatible_count,
            rejected_count: self.rejected_count,
            unavailable_source_count: self.unavailable_source_count,
        }
    }
}

impl IntoApiDto<MemberRemovalDto> for MemberRevocationSummary {
    fn into_api_dto(self) -> MemberRemovalDto {
        MemberRemovalDto {
            revocation_id: self.revocation_id,
            outcome: match self.outcome {
                MemberRevocationOutcome::LocalOnly => MemberRemovalOutcomeDto::LocalOnly,
                MemberRevocationOutcome::Recovering => MemberRemovalOutcomeDto::Recovering,
                MemberRevocationOutcome::Applied => MemberRemovalOutcomeDto::Applied,
                MemberRevocationOutcome::Complete => MemberRemovalOutcomeDto::Complete,
                MemberRevocationOutcome::RecoveryRequired => {
                    MemberRemovalOutcomeDto::RecoveryRequired
                }
            },
            pending_recipients: self.pending_recipients,
            removed_device_ids: self.removed_device_ids,
            pending_recipient_device_ids: self.pending_recipient_device_ids,
            updated_at_ms: self.updated_at_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dto::member::{
        MemberRemovalDto, MemberRemovalOutcomeDto, MemberSyncPreferencesPatchDto,
        SharedDeviceRefreshDeviceDto, SharedDeviceRefreshDeviceStateDto, SharedDeviceRefreshDto,
        SharedDeviceRefreshPhaseDto, SharedDeviceRefreshStartedDto,
    };

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
    fn convergence_mapping_exposes_only_the_coarse_state() {
        let summary = MembershipConvergenceSummary {
            state: MembershipConvergenceStateSummary::WaitingForUpgrade,
            pending_count: 7,
            waiting_for_peer_count: 2,
            waiting_for_update_count: 1,
            version_incompatible_count: 3,
            blocked_count: 0,
            rejected_count: 1,
        };

        let mapped: MembershipConvergenceDto = summary.into_api_dto();

        assert_eq!(
            mapped.state,
            MembershipConvergenceStateDto::WaitingForUpgrade
        );
        assert_eq!(
            serde_json::to_value(mapped).expect("serialize convergence DTO"),
            serde_json::json!({ "state": "waiting_for_upgrade" })
        );
    }

    #[test]
    fn shared_device_refresh_mapping_preserves_engine_order_and_counts() {
        let started: SharedDeviceRefreshStartedDto = SharedDeviceRefreshStartedSummary {
            request_id: "refresh-1".into(),
        }
        .into_api_dto();
        assert_eq!(started.request_id, "refresh-1");

        let summary = SharedDeviceRefreshSummary {
            request_id: "refresh-1".into(),
            phase: SharedDeviceRefreshPhaseSummary::RoundCompleted,
            devices: vec![SharedDeviceRefreshDeviceSummary {
                device_id: "peer-1".into(),
                display_name: "Windows workstation".into(),
                state: SharedDeviceRefreshDeviceStateSummary::Connected,
            }],
            total_count: 1,
            discovered_count: 0,
            connecting_count: 0,
            connected_count: 1,
            already_present_count: 0,
            waiting_for_peer_count: 0,
            waiting_for_update_count: 0,
            version_incompatible_count: 0,
            rejected_count: 0,
            unavailable_source_count: 0,
        };

        let mapped: SharedDeviceRefreshDto = summary.into_api_dto();

        assert_eq!(mapped.phase, SharedDeviceRefreshPhaseDto::RoundCompleted);
        assert_eq!(mapped.devices.len(), 1);
        assert_eq!(
            mapped.devices,
            vec![SharedDeviceRefreshDeviceDto {
                device_id: "peer-1".into(),
                display_name: "Windows workstation".into(),
                state: SharedDeviceRefreshDeviceStateDto::Connected,
            }]
        );
        assert_eq!(mapped.total_count, 1);
        assert_eq!(mapped.connected_count, 1);
        assert_eq!(mapped.unavailable_source_count, 0);
    }

    #[test]
    fn member_removal_mapping_keeps_pending_devices_and_timestamp() {
        let summary = MemberRevocationSummary {
            revocation_id: Some("removal-1".into()),
            outcome: MemberRevocationOutcome::Applied,
            pending_recipients: 1,
            removed_device_ids: vec!["removed-device".into()],
            pending_recipient_device_ids: vec!["retained-device".into()],
            updated_at_ms: 42,
        };

        let mapped: MemberRemovalDto = summary.into_api_dto();

        assert_eq!(mapped.outcome, MemberRemovalOutcomeDto::Applied);
        assert_eq!(mapped.pending_recipient_device_ids, vec!["retained-device"]);
        assert_eq!(mapped.updated_at_ms, 42);
    }

    #[test]
    fn member_removal_mapping_exposes_recovering_without_pending_devices() {
        let summary = MemberRevocationSummary {
            revocation_id: Some("removal-recovering".into()),
            outcome: MemberRevocationOutcome::Recovering,
            pending_recipients: 0,
            removed_device_ids: vec!["removed-device".into()],
            pending_recipient_device_ids: Vec::new(),
            updated_at_ms: 42,
        };

        let mapped: MemberRemovalDto = summary.into_api_dto();

        assert_eq!(
            serde_json::to_value(mapped).expect("serialize member removal")["outcome"],
            "recovering"
        );
    }
}
