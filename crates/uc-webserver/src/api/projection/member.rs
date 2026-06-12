//! Member roster boundary projections: per-member sync preferences ↔ DTOs.
//!
//! The roster facade has its own `ContentTypesPatch` / `ContentTypesView`
//! types (distinct from the settings facade's), so the shared
//! `ContentTypesPatchDto` / `ContentTypesDto` carry one impl per target here
//! in addition to the settings ones.

use uc_application::facade::{
    ContentTypesPatch, ContentTypesView, MemberSyncPreferencesPatch, MemberSyncPreferencesView,
};

use super::{IntoApiDto, IntoDomain};
use crate::api::dto::member::MemberSyncPreferencesDto;
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

impl IntoApiDto<ContentTypesDto> for ContentTypesView {
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

impl IntoApiDto<MemberSyncPreferencesDto> for MemberSyncPreferencesView {
    fn into_api_dto(self) -> MemberSyncPreferencesDto {
        MemberSyncPreferencesDto {
            send_enabled: self.send_enabled,
            receive_enabled: self.receive_enabled,
            send_content_types: self.send_content_types.into_api_dto(),
            receive_content_types: self.receive_content_types.into_api_dto(),
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
