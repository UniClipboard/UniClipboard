//! Storage management Tauri commands
//! 存储管理相关的 Tauri 命令

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use serde::Serialize;
use tracing::{info_span, Instrument};
use uc_platform::ports::observability::TraceMetadata;

/// Storage statistics response.
/// 存储统计信息响应。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageStats {
    /// Database file size in bytes
    pub database_bytes: u64,
    /// Blob vault directory size in bytes
    pub vault_bytes: u64,
    /// Cache directory size in bytes
    pub cache_bytes: u64,
    /// Logs directory size in bytes
    pub logs_bytes: u64,
    /// Total size in bytes
    pub total_bytes: u64,
    /// Application data directory path (for "Open in Finder")
    pub data_dir: String,
}

/// Get storage statistics for the application.
/// 获取应用的存储统计信息。
#[tauri::command]
pub async fn get_storage_stats(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<StorageStats, CommandError> {
    let span = info_span!(
        "command.storage.get_stats",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let result = runtime
            .usecases()
            .get_storage_stats()
            .execute()
            .await
            .map_err(|e| CommandError::InternalError(e.to_string()))?;

        Ok(StorageStats {
            database_bytes: result.database_bytes,
            vault_bytes: result.vault_bytes,
            cache_bytes: result.cache_bytes,
            logs_bytes: result.logs_bytes,
            total_bytes: result.total_bytes,
            data_dir: result.data_dir,
        })
    }
    .instrument(span)
    .await
}

/// Clear cache directory (thumbnails, temporary files).
/// 清除缓存目录（缩略图、临时文件）。
#[tauri::command]
pub async fn clear_cache(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<u64, CommandError> {
    let span = info_span!(
        "command.storage.clear_cache",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        runtime
            .usecases()
            .clear_cache()
            .execute()
            .await
            .map_err(|e| CommandError::InternalError(e.to_string()))
    }
    .instrument(span)
    .await
}

/// Clear all clipboard history (entries, events, representations, blobs).
/// 清除所有剪贴板历史（条目、事件、表示、Blob）。
#[tauri::command]
pub async fn clear_all_clipboard_history(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<u64, CommandError> {
    let span = info_span!(
        "command.storage.clear_all_history",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        // 1. Get all entry IDs using pagination (max limit is 1000)
        let uc = runtime.usecases().list_clipboard_entries();
        let mut entries = Vec::new();
        let mut offset = 0usize;
        let batch_size = 1000usize;

        loop {
            let batch = uc.execute(batch_size, offset).await.map_err(|e| {
                tracing::error!(error = %e, "Failed to list entries for bulk delete");
                CommandError::InternalError(e.to_string())
            })?;

            if batch.is_empty() {
                break;
            }

            let batch_len = batch.len();
            entries.extend(batch);
            offset += batch_len;

            // If we got fewer than batch_size, we've fetched everything
            if batch_len < batch_size {
                break;
            }
        }

        let total = entries.len() as u64;
        tracing::info!(
            total_entries = total,
            "Starting bulk clipboard history deletion"
        );

        // 2. Delete each entry
        let delete_uc = runtime.usecases().delete_clipboard_entry();
        let mut deleted = 0u64;
        for entry in &entries {
            match delete_uc.execute(&entry.entry_id).await {
                Ok(()) => deleted += 1,
                Err(e) => {
                    tracing::warn!(
                        entry_id = %entry.entry_id,
                        error = %e,
                        "Failed to delete entry during bulk clear"
                    );
                }
            }
        }

        tracing::info!(deleted, total, "Clipboard history cleared");
        Ok(deleted)
    }
    .instrument(span)
    .await
}

/// Open the application data directory in the system file manager.
/// 在系统文件管理器中打开应用数据目录。
#[tauri::command]
pub async fn open_data_directory(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_data_dir",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        runtime
            .usecases()
            .open_data_directory()
            .execute()
            .await
            .map_err(|e| {
                if e.to_string().contains("does not exist") {
                    CommandError::NotFound(e.to_string())
                } else {
                    CommandError::InternalError(e.to_string())
                }
            })
    }
    .instrument(span)
    .await
}
