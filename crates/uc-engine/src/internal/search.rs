//! Shared search operations.

use tracing::error;
use uc_application::facade::{AppFacade, SearchFacadeError, SearchQueryInput, SearchResultView};

use crate::{
    EngineError, EngineErrorCategory, OperationResult, SearchEntriesInput, SearchPageSummary,
    SearchResultSummary, SearchStatusSummary, SearchTagSummary,
};

pub const SEARCH_INVALID_QUERY_CODE: u32 = 1391;
pub const SEARCH_BAD_REQUEST_CODE: u32 = 1392;
pub const SEARCH_SESSION_LOCKED_CODE: u32 = 1393;
pub const SEARCH_INDEX_NOT_READY_CODE: u32 = 1394;
pub const SEARCH_INDEX_REBUILDING_CODE: u32 = 1395;
pub const SEARCH_INDEX_UNAVAILABLE_CODE: u32 = 1396;
pub const SEARCH_SERVICE_UNAVAILABLE_CODE: u32 = 1397;
pub const SEARCH_REBUILD_ALREADY_RUNNING_CODE: u32 = 1398;
pub const SEARCH_FAILED_CODE: u32 = 1399;

pub async fn execute_search_entries(
    facade: &AppFacade,
    input: SearchEntriesInput,
) -> Result<OperationResult, EngineError> {
    let page = facade
        .search
        .query(SearchQueryInput {
            query: input.query,
            operator: input.operator,
            time_preset: input.time_preset,
            from_ms: input.from_ms,
            to_ms: input.to_ms,
            content_types: input.content_types,
            extensions: input.extensions,
            source_devices: input.source_devices,
            tags: input.tags,
            limit: input.limit,
            offset: input.offset,
        })
        .await
        .map_err(map_search_error)?;

    Ok(OperationResult::SearchPage(SearchPageSummary {
        total: page.total,
        has_more: page.has_more,
        state: page.state,
        items: page.items.into_iter().map(search_result).collect(),
    }))
}

pub async fn execute_query_search_tags(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let tags = facade
        .search
        .tags()
        .await
        .map_err(map_search_error)?
        .into_iter()
        .map(|tag| SearchTagSummary {
            tag_id: tag.tag_id,
            count: tag.count,
            is_builtin: tag.is_builtin,
        })
        .collect();
    Ok(OperationResult::SearchTags(tags))
}

pub async fn execute_query_search_status(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let status = facade.search.status().await.map_err(map_search_error)?;
    Ok(OperationResult::SearchStatus(SearchStatusSummary {
        state: status.state,
        reason: status.reason,
        last_rebuild_started_at_ms: status.last_rebuild_started_at_ms,
        last_rebuild_completed_at_ms: status.last_rebuild_completed_at_ms,
    }))
}

pub async fn execute_rebuild_search_index(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let accepted = facade
        .search
        .request_rebuild()
        .await
        .map_err(map_search_error)?;
    Ok(OperationResult::SearchRebuildAccepted {
        accepted: accepted.accepted,
    })
}

fn search_result(result: SearchResultView) -> SearchResultSummary {
    SearchResultSummary {
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
    }
}

fn map_search_error(error: SearchFacadeError) -> EngineError {
    let error_message = error.to_string();
    let (code, category, retryable, variant, log_details) = match error {
        SearchFacadeError::InvalidQuery(_) => (
            SEARCH_INVALID_QUERY_CODE,
            EngineErrorCategory::InvalidInput,
            false,
            "invalid_query",
            false,
        ),
        SearchFacadeError::BadRequest(_) => (
            SEARCH_BAD_REQUEST_CODE,
            EngineErrorCategory::InvalidInput,
            false,
            "bad_request",
            false,
        ),
        SearchFacadeError::SessionLocked => (
            SEARCH_SESSION_LOCKED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
            "session_locked",
            false,
        ),
        SearchFacadeError::IndexNotReady => (
            SEARCH_INDEX_NOT_READY_CODE,
            EngineErrorCategory::Unavailable,
            true,
            "index_not_ready",
            false,
        ),
        SearchFacadeError::IndexRebuilding => (
            SEARCH_INDEX_REBUILDING_CODE,
            EngineErrorCategory::Unavailable,
            true,
            "index_rebuilding",
            false,
        ),
        SearchFacadeError::IndexUnavailable => (
            SEARCH_INDEX_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
            "index_unavailable",
            false,
        ),
        SearchFacadeError::ServiceUnavailable(_) => (
            SEARCH_SERVICE_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
            "service_unavailable",
            true,
        ),
        SearchFacadeError::RebuildAlreadyRunning => (
            SEARCH_REBUILD_ALREADY_RUNNING_CODE,
            EngineErrorCategory::Conflict,
            false,
            "rebuild_already_running",
            false,
        ),
        SearchFacadeError::Internal(_) => (
            SEARCH_FAILED_CODE,
            EngineErrorCategory::Internal,
            false,
            "internal",
            true,
        ),
    };
    if log_details {
        error!(variant, error = %error_message, "search operation failed");
    }
    EngineError::new(code, category, retryable)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_failures_keep_stable_categories_and_distinct_codes() {
        let invalid = map_search_error(SearchFacadeError::InvalidQuery("private query".into()));
        let locked = map_search_error(SearchFacadeError::SessionLocked);
        let rebuilding = map_search_error(SearchFacadeError::IndexRebuilding);
        let conflict = map_search_error(SearchFacadeError::RebuildAlreadyRunning);

        assert_eq!(invalid.category(), EngineErrorCategory::InvalidInput);
        assert_eq!(locked.category(), EngineErrorCategory::Unauthorized);
        assert_eq!(rebuilding.category(), EngineErrorCategory::Unavailable);
        assert_eq!(conflict.category(), EngineErrorCategory::Conflict);
        assert_ne!(invalid.code(), locked.code());
        assert_ne!(locked.code(), rebuilding.code());
        assert_ne!(rebuilding.code(), conflict.code());
    }
}
