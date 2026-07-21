//! Shared clipboard history operations.

use tracing::error;
use uc_application::facade::{
    AppFacade, ClipboardHistoryError, ClipboardListInput, EntryDetailView, EntryProjectionView,
    EntryResourceView,
};

use crate::{
    EngineError, EngineErrorCategory, HistoryClearSummary, HistoryEntryDetailSummary,
    HistoryEntryInput, HistoryEntryResourceSummary, HistoryEntrySummary, HistoryStatsSummary,
    ListHistoryEntriesInput, OperationResult, SetHistoryEntryFavoriteInput,
};

pub const HISTORY_INVALID_INPUT_CODE: u32 = 1401;
pub const HISTORY_NOT_FOUND_CODE: u32 = 1402;
pub const HISTORY_UNSUPPORTED_CONTENT_CODE: u32 = 1403;
pub const HISTORY_FAILED_CODE: u32 = 1404;
const MAX_HISTORY_LIST_SIZE: u32 = 1000;

pub async fn execute_list_history_entries(
    facade: &AppFacade,
    input: ListHistoryEntriesInput,
) -> Result<OperationResult, EngineError> {
    if input.limit == 0 || input.limit > MAX_HISTORY_LIST_SIZE {
        return Err(invalid_input_error());
    }
    let offset = usize::try_from(input.offset).map_err(|_| invalid_input_error())?;
    let entries = facade
        .clipboard_history
        .list_entries(ClipboardListInput {
            limit: input.limit as usize,
            offset,
        })
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryEntries(
        entries.into_iter().map(history_entry_summary).collect(),
    ))
}

pub async fn execute_get_history_entry(
    facade: &AppFacade,
    input: HistoryEntryInput,
) -> Result<OperationResult, EngineError> {
    validate_entry_id(&input.entry_id)?;
    let detail = facade
        .clipboard_history
        .get_entry(&input.entry_id)
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryEntry(history_entry_detail(detail)))
}

pub async fn execute_delete_history_entry(
    facade: &AppFacade,
    input: HistoryEntryInput,
) -> Result<OperationResult, EngineError> {
    validate_entry_id(&input.entry_id)?;
    facade
        .clipboard_history
        .delete_entry(&input.entry_id)
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryEntryDeleted)
}

pub async fn execute_set_history_entry_favorite(
    facade: &AppFacade,
    input: SetHistoryEntryFavoriteInput,
) -> Result<OperationResult, EngineError> {
    validate_entry_id(&input.entry_id)?;
    let found = facade
        .clipboard_history
        .toggle_favorite(&input.entry_id, input.is_favorited)
        .await
        .map_err(map_history_error)?;
    if !found {
        return Err(not_found_error());
    }
    Ok(OperationResult::HistoryEntryFavoriteSet)
}

pub async fn execute_query_history_stats(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let stats = facade
        .clipboard_history
        .stats()
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryStats(HistoryStatsSummary {
        total_items: stats.total_items,
        total_size: stats.total_size,
    }))
}

pub async fn execute_get_history_entry_resource(
    facade: &AppFacade,
    input: HistoryEntryInput,
) -> Result<OperationResult, EngineError> {
    validate_entry_id(&input.entry_id)?;
    let resource = facade
        .clipboard_history
        .get_entry_resource(&input.entry_id)
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryEntryResource(
        history_entry_resource(resource),
    ))
}

pub async fn execute_clear_history(facade: &AppFacade) -> Result<OperationResult, EngineError> {
    let result = facade
        .clipboard_history
        .clear_history()
        .await
        .map_err(map_history_error)?;
    Ok(OperationResult::HistoryCleared(HistoryClearSummary {
        deleted_count: result.deleted_count,
        failed_entry_ids: result
            .failed_entries
            .into_iter()
            .map(|(entry_id, _)| entry_id)
            .collect(),
    }))
}

fn history_entry_summary(entry: EntryProjectionView) -> HistoryEntrySummary {
    HistoryEntrySummary {
        entry_id: entry.id,
        preview: entry.preview,
        has_detail: entry.has_detail,
        size_bytes: entry.size_bytes,
        captured_at_ms: entry.captured_at,
        content_type: entry.content_type,
        thumbnail_url: entry.thumbnail_url,
        is_encrypted: entry.is_encrypted,
        is_favorited: entry.is_favorited,
        updated_at_ms: entry.updated_at,
        active_time_ms: entry.active_time,
        file_transfer_status: entry.file_transfer_status,
        file_transfer_reason: entry.file_transfer_reason,
        content_tags: entry.content_tags,
        link_urls: entry.link_urls,
        link_domains: entry.link_domains,
        file_sizes: entry.file_sizes,
        image_width: entry.image_width,
        image_height: entry.image_height,
        payload_state: entry.payload_state,
    }
}

fn history_entry_detail(detail: EntryDetailView) -> HistoryEntryDetailSummary {
    HistoryEntryDetailSummary {
        entry_id: detail.id,
        content: detail.content,
        size_bytes: detail.size_bytes,
        created_at_ms: detail.created_at_ms,
        active_time_ms: detail.active_time_ms,
        mime_type: detail.mime_type,
    }
}

fn history_entry_resource(resource: EntryResourceView) -> HistoryEntryResourceSummary {
    HistoryEntryResourceSummary {
        blob_id: resource.blob_id,
        mime_type: resource.mime_type,
        size_bytes: resource.size_bytes,
        url: resource.url,
        inline_data: resource.inline_data,
    }
}

fn validate_entry_id(entry_id: &str) -> Result<(), EngineError> {
    if entry_id.trim().is_empty() {
        return Err(invalid_input_error());
    }
    Ok(())
}

fn invalid_input_error() -> EngineError {
    EngineError::new(
        HISTORY_INVALID_INPUT_CODE,
        EngineErrorCategory::InvalidInput,
        false,
    )
}

fn not_found_error() -> EngineError {
    EngineError::new(HISTORY_NOT_FOUND_CODE, EngineErrorCategory::NotFound, false)
}

fn map_history_error(error: ClipboardHistoryError) -> EngineError {
    match error {
        ClipboardHistoryError::NotFound => not_found_error(),
        ClipboardHistoryError::UnsupportedContent => EngineError::new(
            HISTORY_UNSUPPORTED_CONTENT_CODE,
            EngineErrorCategory::Conflict,
            false,
        ),
        ClipboardHistoryError::Internal(_) => {
            error!("clipboard history operation failed");
            EngineError::new(HISTORY_FAILED_CODE, EngineErrorCategory::Internal, false)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_failures_have_stable_categories_without_exposing_details() {
        let missing = map_history_error(ClipboardHistoryError::NotFound);
        let unsupported = map_history_error(ClipboardHistoryError::UnsupportedContent);
        let internal = map_history_error(ClipboardHistoryError::Internal(
            "/private/path/secret.txt".into(),
        ));

        assert_eq!(missing.category(), EngineErrorCategory::NotFound);
        assert_eq!(unsupported.category(), EngineErrorCategory::Conflict);
        assert_eq!(internal.category(), EngineErrorCategory::Internal);
        assert!(!internal.to_string().contains("secret.txt"));
        assert_ne!(missing.code(), unsupported.code());
        assert_ne!(unsupported.code(), internal.code());
    }
}
