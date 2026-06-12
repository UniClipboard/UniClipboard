//! Search boundary projections: search facade views onto search DTOs.

use uc_application::facade::{SearchPageView, SearchStatusView};

use super::IntoApiDto;
use crate::api::dto::search::{SearchResultDto, SearchStatusData};

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
                text_preview: result.text_preview,
                mime_type: result.mime_type,
                file_extensions: result.file_extensions,
            })
            .collect()
    }
}
