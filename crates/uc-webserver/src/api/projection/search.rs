//! Search boundary projections: search facade views onto search DTOs.

use uc_application::facade::{SearchPageView, SearchStatusView, SearchTagView};

use super::IntoApiDto;
use crate::api::dto::search::{SearchResultDto, SearchStatusData, SearchTagDto};

impl IntoApiDto<SearchStatusData> for SearchStatusView {
    fn into_api_dto(self) -> SearchStatusData {
        SearchStatusData {
            state: self.state,
            reason: self.reason,
            last_rebuild_started_at_ms: self.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: self.last_rebuild_completed_at_ms,
        }
    }
}

impl IntoApiDto<Vec<SearchResultDto>> for SearchPageView {
    fn into_api_dto(self) -> Vec<SearchResultDto> {
        self.items
            .into_iter()
            .map(|result| SearchResultDto {
                entry_id: result.entry_id,
                content_type: result.content_type,
                active_time_ms: result.active_time_ms,
                tags: result.tags,
                text_preview: result.text_preview,
                mime_type: result.mime_type,
                file_extensions: result.file_extensions,
                file_names: result.file_names,
                link_urls: result.link_urls,
                source_device: result.source_device,
                payload_state: result.payload_state,
            })
            .collect()
    }
}

impl IntoApiDto<SearchTagDto> for SearchTagView {
    fn into_api_dto(self) -> SearchTagDto {
        SearchTagDto {
            tag_id: self.tag_id,
            count: self.count,
            is_builtin: self.is_builtin,
        }
    }
}
