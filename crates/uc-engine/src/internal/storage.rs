//! Shared engine-owned storage maintenance operations.

use tracing::error;
use uc_application::facade::{AppFacade, StorageFacadeError, StorageStatsView};

use crate::{EngineError, EngineErrorCategory, OperationResult, StorageStatsSummary};

pub const QUERY_STORAGE_STATS_FAILED_CODE: u32 = 1341;
pub const CLEAR_STORAGE_CACHE_FAILED_CODE: u32 = 1342;

pub async fn execute_query_storage_stats(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    facade
        .storage
        .stats()
        .await
        .map(storage_stats_result)
        .map_err(map_storage_error)
}

pub async fn execute_clear_storage_cache(
    facade: &AppFacade,
) -> Result<OperationResult, EngineError> {
    let result = facade
        .storage
        .clear_cache()
        .await
        .map_err(map_storage_error)?;
    Ok(OperationResult::StorageCacheCleared {
        freed_bytes: result.freed_bytes,
    })
}

pub(crate) fn storage_stats_result(view: StorageStatsView) -> OperationResult {
    OperationResult::StorageStats(StorageStatsSummary {
        total_bytes: view.total_bytes,
        database_bytes: view.database_bytes,
        vault_bytes: view.vault_bytes,
        cache_bytes: view.cache_bytes,
        logs_bytes: view.logs_bytes,
    })
}

pub(crate) fn map_storage_error(error: StorageFacadeError) -> EngineError {
    let code = match error {
        StorageFacadeError::Stats(_) => QUERY_STORAGE_STATS_FAILED_CODE,
        StorageFacadeError::ClearCache(_) => CLEAR_STORAGE_CACHE_FAILED_CODE,
    };
    error!(code, "engine storage operation failed");
    EngineError::new(code, EngineErrorCategory::Internal, false)
}
