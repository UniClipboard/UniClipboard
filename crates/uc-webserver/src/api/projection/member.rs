//! Member roster boundary projections: per-member sync preferences ↔ DTOs.
//!
//! The engine interface has its own content-type patch and summary types
//! (distinct from the settings facade's), so the shared
//! `ContentTypesPatchDto` / `ContentTypesDto` carry one impl per target here
//! in addition to the settings ones.

use uc_engine::{
    ContentTypesPatch, ContentTypesSummary, LegacyBootstrapOutcome, LegacyBootstrapSummary,
    MemberProtectionStatusSummary, MemberProtectionSummary, MemberSyncPreferencesPatch,
    MemberSyncPreferencesSummary, SpaceProtectionModeSummary, SpaceProtectionSummary,
};

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::member::{
    LegacyBootstrapDto, LegacyBootstrapOutcomeDto, MemberProtectionDto, MemberProtectionStatusDto,
    MemberSyncPreferencesDto, SpaceProtectionDto, SpaceProtectionModeDto,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::dto::member::MemberSyncPreferencesPatchDto;

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
}
