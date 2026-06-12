//! v2 setup boundary projections: `SpaceSetupFacade` results onto the
//! `/v2/setup/*` wire DTOs.

use uc_application::facade::space_setup::{
    MigrationPhaseKind, MigrationProgress, SwitchSpaceResult,
};
use uc_application::facade::{
    InitializeSpaceResult, IssuePairingInvitationResult, RedeemPairingInvitationResult,
    SetupStateView,
};
use uc_daemon_contract::api::dto::v2::setup::{
    CurrentInvitation, InitializeSpaceResponse, IssueInvitationResponse, MigrationPhaseDto,
    MigrationProgressResponse, RedeemResponse, SetupStateResponse, SwitchSpaceResponse,
};

use super::IntoApiDto;

impl IntoApiDto<InitializeSpaceResponse> for InitializeSpaceResult {
    fn into_api_dto(self) -> InitializeSpaceResponse {
        InitializeSpaceResponse {
            space_id: self.space_id.to_string(),
            self_device_id: self.self_device_id.to_string(),
            fingerprint: self.fingerprint.as_display().to_string(),
        }
    }
}

impl IntoApiDto<IssueInvitationResponse> for IssuePairingInvitationResult {
    fn into_api_dto(self) -> IssueInvitationResponse {
        IssueInvitationResponse {
            code: self.code.as_str().to_string(),
            expires_at_ms: self.expires_at.timestamp_millis(),
        }
    }
}

impl IntoApiDto<RedeemResponse> for RedeemPairingInvitationResult {
    fn into_api_dto(self) -> RedeemResponse {
        RedeemResponse {
            sponsor_device_id: self.sponsor_device_id.to_string(),
            sponsor_identity_fingerprint: self
                .sponsor_identity_fingerprint
                .as_display()
                .to_string(),
            space_id: self.space_id.to_string(),
            self_device_id: self.self_device_id.to_string(),
            self_identity_fingerprint: self.self_identity_fingerprint.as_display().to_string(),
        }
    }
}

impl IntoApiDto<SetupStateResponse> for SetupStateView {
    fn into_api_dto(self) -> SetupStateResponse {
        SetupStateResponse {
            has_completed: self.has_completed,
            current_invitation: self.current_invitation.map(|inv| CurrentInvitation {
                code: inv.code.as_str().to_string(),
                expires_at_ms: inv.expires_at.timestamp_millis(),
            }),
            device_name: self.device_name,
        }
    }
}

impl IntoApiDto<SwitchSpaceResponse> for SwitchSpaceResult {
    fn into_api_dto(self) -> SwitchSpaceResponse {
        SwitchSpaceResponse {
            sponsor_device_id: self.sponsor_device_id.to_string(),
            sponsor_identity_fingerprint: self
                .sponsor_identity_fingerprint
                .as_display()
                .to_string(),
            space_id: self.space_id.to_string(),
            self_device_id: self.self_device_id.to_string(),
            self_identity_fingerprint: self.self_identity_fingerprint.as_display().to_string(),
            migrated_records: self.migrated_records,
        }
    }
}

impl IntoApiDto<MigrationProgressResponse> for MigrationProgress {
    fn into_api_dto(self) -> MigrationProgressResponse {
        MigrationProgressResponse {
            phase: self.phase.map(IntoApiDto::into_api_dto),
            backup_record_count: self.backup_record_count,
        }
    }
}

impl IntoApiDto<MigrationPhaseDto> for MigrationPhaseKind {
    fn into_api_dto(self) -> MigrationPhaseDto {
        match self {
            MigrationPhaseKind::Prepared => MigrationPhaseDto::Prepared,
            MigrationPhaseKind::HandshakeDone => MigrationPhaseDto::HandshakeDone,
            MigrationPhaseKind::Swapped => MigrationPhaseDto::Swapped,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use chrono::{DateTime, Utc};
    use uc_application::facade::CurrentInvitation as FacadeCurrentInvitation;
    use uc_core::ids::{DeviceId, SpaceId};
    use uc_core::pairing::invitation::InvitationCode;
    use uc_core::security::IdentityFingerprint;

    fn fixed_fp() -> IdentityFingerprint {
        IdentityFingerprint::from_raw_string("ABCDEFGHIJKLMNOP").unwrap()
    }

    #[test]
    fn initialize_to_dto_flattens_domain_types_to_strings() {
        let dto: InitializeSpaceResponse = InitializeSpaceResult {
            space_id: SpaceId::from_str("space-1"),
            self_device_id: DeviceId::new("device-1"),
            fingerprint: fixed_fp(),
        }
        .into_api_dto();
        assert_eq!(dto.space_id, "space-1");
        assert_eq!(dto.self_device_id, "device-1");
        assert_eq!(dto.fingerprint, "ABCD-EFGH-IJKL-MNOP");
    }

    #[test]
    fn issue_to_dto_serialises_expiry_as_epoch_millis() {
        let expires = DateTime::parse_from_rfc3339("2026-04-25T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let dto: IssueInvitationResponse = IssuePairingInvitationResult {
            code: InvitationCode::new("ABCD-1234"),
            expires_at: expires,
        }
        .into_api_dto();
        assert_eq!(dto.code, "ABCD-1234");
        assert_eq!(dto.expires_at_ms, expires.timestamp_millis());
    }

    #[test]
    fn redeem_to_dto_carries_both_sides() {
        let dto: RedeemResponse = RedeemPairingInvitationResult {
            sponsor_device_id: DeviceId::new("sponsor-1"),
            sponsor_identity_fingerprint: fixed_fp(),
            space_id: SpaceId::from_str("space-1"),
            self_device_id: DeviceId::new("joiner-2"),
            self_identity_fingerprint: fixed_fp(),
        }
        .into_api_dto();
        assert_eq!(dto.sponsor_device_id, "sponsor-1");
        assert_eq!(dto.self_device_id, "joiner-2");
        assert_eq!(dto.space_id, "space-1");
        assert_eq!(dto.sponsor_identity_fingerprint, "ABCD-EFGH-IJKL-MNOP");
        assert_eq!(dto.self_identity_fingerprint, "ABCD-EFGH-IJKL-MNOP");
    }

    #[test]
    fn state_to_dto_with_pending_invitation() {
        let expires = DateTime::parse_from_rfc3339("2026-04-25T13:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let dto: SetupStateResponse = SetupStateView {
            has_completed: true,
            current_invitation: Some(FacadeCurrentInvitation {
                code: InvitationCode::new("WXYZ"),
                expires_at: expires,
            }),
            device_name: Some("MacBook".to_string()),
        }
        .into_api_dto();
        assert!(dto.has_completed);
        let inv = dto.current_invitation.expect("invitation present");
        assert_eq!(inv.code, "WXYZ");
        assert_eq!(inv.expires_at_ms, expires.timestamp_millis());
        assert_eq!(dto.device_name.as_deref(), Some("MacBook"));
    }

    #[test]
    fn state_to_dto_fresh_install_returns_none_branches() {
        let dto: SetupStateResponse = SetupStateView {
            has_completed: false,
            current_invitation: None,
            device_name: None,
        }
        .into_api_dto();
        assert!(!dto.has_completed);
        assert!(dto.current_invitation.is_none());
        assert!(dto.device_name.is_none());
    }

    #[test]
    fn switch_space_to_dto_carries_all_fields_including_migrated_records() {
        let dto: SwitchSpaceResponse = SwitchSpaceResult {
            sponsor_device_id: DeviceId::new("sponsor-1"),
            sponsor_identity_fingerprint: fixed_fp(),
            space_id: SpaceId::from_str("space-new"),
            self_device_id: DeviceId::new("joiner-2"),
            self_identity_fingerprint: fixed_fp(),
            migrated_records: 7,
        }
        .into_api_dto();
        assert_eq!(dto.sponsor_device_id, "sponsor-1");
        assert_eq!(dto.self_device_id, "joiner-2");
        assert_eq!(dto.space_id, "space-new");
        assert_eq!(dto.migrated_records, 7);
        assert_eq!(dto.sponsor_identity_fingerprint, "ABCD-EFGH-IJKL-MNOP");
    }

    #[test]
    fn migration_progress_to_dto_idle_returns_phase_none() {
        let dto: MigrationProgressResponse = MigrationProgress {
            phase: None,
            backup_record_count: 0,
        }
        .into_api_dto();
        assert!(dto.phase.is_none());
        assert_eq!(dto.backup_record_count, 0);
    }

    #[test]
    fn migration_progress_to_dto_maps_each_phase_kind() {
        for (kind, expected) in [
            (MigrationPhaseKind::Prepared, MigrationPhaseDto::Prepared),
            (
                MigrationPhaseKind::HandshakeDone,
                MigrationPhaseDto::HandshakeDone,
            ),
            (MigrationPhaseKind::Swapped, MigrationPhaseDto::Swapped),
        ] {
            let dto: MigrationProgressResponse = MigrationProgress {
                phase: Some(kind),
                backup_record_count: 3,
            }
            .into_api_dto();
            assert_eq!(dto.phase, Some(expected));
            assert_eq!(dto.backup_record_count, 3);
        }
    }
}
