//! Search boundary projections: engine summaries onto search DTOs.

use uc_engine::{SearchPageSummary, SearchStatusSummary, SearchTagSummary};

use super::IntoApiDto;
use crate::api::dto::search::{SearchResultDto, SearchStatusData, SearchTagDto};

impl IntoApiDto<SearchStatusData> for SearchStatusSummary {
    fn into_api_dto(self) -> SearchStatusData {
        SearchStatusData {
            state: self.state,
            reason: self.reason,
            last_rebuild_started_at_ms: self.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: self.last_rebuild_completed_at_ms,
        }
    }
}

impl IntoApiDto<Vec<SearchResultDto>> for SearchPageSummary {
    fn into_api_dto(self) -> Vec<SearchResultDto> {
        self.items
            .into_iter()
            .map(|result| SearchResultDto {
                entry_id: result.entry_id,
                content_type: result.content_type,
                active_time_ms: result.active_time_ms,
                tags: result.tags,
                text_preview: result.text_preview,
                char_count: result.char_count,
                mime_type: result.mime_type,
                file_extensions: result.file_extensions,
                file_names: result.file_names,
                file_paths: result.file_paths,
                link_urls: result.link_urls,
                source_device: result.source_device,
                payload_state: result.payload_state,
            })
            .collect()
    }
}

impl IntoApiDto<SearchTagDto> for SearchTagSummary {
    fn into_api_dto(self) -> SearchTagDto {
        SearchTagDto {
            tag_id: self.tag_id,
            count: self.count,
            is_builtin: self.is_builtin,
        }
    }
}
