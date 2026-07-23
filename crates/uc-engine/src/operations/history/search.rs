//! Shared search operations.

use crate::error_codes::*;

use tracing::error;
use uc_application::facade::{
    AppFacade, SearchFacadeError, SearchPageView, SearchQueryInput, SearchResultView,
};

use crate::{
    EngineError, EngineErrorCategory, EntrySummary, OperationResult, QueryHistoryInput,
    SearchEntriesInput, SearchPageSummary, SearchResultSummary, SearchStatusSummary,
    SearchTagSummary,
};

const QUERY_HISTORY_INVALID_INPUT_CODE: u32 = 1241;
const QUERY_HISTORY_UNAUTHORIZED_CODE: u32 = 1242;
const QUERY_HISTORY_UNAVAILABLE_CODE: u32 = 1243;
const QUERY_HISTORY_FAILED_CODE: u32 = 1244;
const HISTORY_CURSOR_PREFIX: &str = "uc-history-v1:";
const MAX_HISTORY_PAGE_SIZE: u32 = 200;

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

pub(crate) fn history_search_input(
    input: QueryHistoryInput,
) -> Result<SearchQueryInput, EngineError> {
    if input.limit == 0 || input.limit > MAX_HISTORY_PAGE_SIZE {
        return Err(query_history_invalid_input_error());
    }
    let offset = match input.cursor.as_deref() {
        None => 0,
        Some(cursor) => cursor
            .strip_prefix(HISTORY_CURSOR_PREFIX)
            .and_then(|value| value.parse::<u32>().ok())
            .ok_or_else(query_history_invalid_input_error)?,
    };

    Ok(SearchQueryInput {
        query: input.query.unwrap_or_default(),
        operator: None,
        time_preset: None,
        from_ms: None,
        to_ms: None,
        content_types: None,
        extensions: None,
        source_devices: None,
        tags: None,
        limit: input.limit,
        offset,
    })
}

pub(crate) fn history_page_result(
    page: SearchPageView,
    offset: u32,
    limit: u32,
) -> Result<OperationResult, EngineError> {
    let next_cursor = if page.has_more {
        let next_offset = offset
            .checked_add(limit)
            .ok_or_else(query_history_invalid_input_error)?;
        Some(format!("{HISTORY_CURSOR_PREFIX}{next_offset}"))
    } else {
        None
    };
    let entries = page
        .items
        .into_iter()
        .map(|item| EntrySummary {
            entry_id: item.entry_id,
            content_type: item.content_type,
            preview: item.text_preview,
            created_at_ms: item.active_time_ms,
        })
        .collect();

    Ok(OperationResult::HistoryPage {
        entries,
        next_cursor,
    })
}

pub(crate) fn map_query_history_error(error: SearchFacadeError) -> EngineError {
    match error {
        SearchFacadeError::InvalidQuery(_) | SearchFacadeError::BadRequest(_) => {
            query_history_invalid_input_error()
        }
        SearchFacadeError::SessionLocked => EngineError::new(
            QUERY_HISTORY_UNAUTHORIZED_CODE,
            EngineErrorCategory::Unauthorized,
            false,
        ),
        SearchFacadeError::IndexNotReady
        | SearchFacadeError::IndexRebuilding
        | SearchFacadeError::IndexUnavailable
        | SearchFacadeError::ServiceUnavailable(_) => EngineError::new(
            QUERY_HISTORY_UNAVAILABLE_CODE,
            EngineErrorCategory::Unavailable,
            true,
        ),
        SearchFacadeError::RebuildAlreadyRunning => EngineError::new(
            QUERY_HISTORY_UNAVAILABLE_CODE,
            EngineErrorCategory::Conflict,
            true,
        ),
        SearchFacadeError::Internal(_) => {
            error!(error = %error, "query history failed");
            EngineError::new(
                QUERY_HISTORY_FAILED_CODE,
                EngineErrorCategory::Internal,
                false,
            )
        }
    }
}

fn query_history_invalid_input_error() -> EngineError {
    EngineError::new(
        QUERY_HISTORY_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
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
