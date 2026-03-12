//! Storage management Tauri commands
//! 存储管理相关的 Tauri 命令

use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use serde::Serialize;
use std::path::Path;
use tracing::{info_span, Instrument};
use uc_app::app_paths::AppPaths;
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;
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

/// Recursively calculate directory size in bytes.
/// 递归计算目录大小（字节数）。
async fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }

    if path.is_file() {
        return tokio::fs::metadata(path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
    }

    let mut total: u64 = 0;
    let mut entries = match tokio::fs::read_dir(path).await {
        Ok(entries) => entries,
        Err(_) => return 0,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let entry_path = entry.path();
        if entry_path.is_dir() {
            total += Box::pin(dir_size(&entry_path)).await;
        } else {
            total += tokio::fs::metadata(&entry_path)
                .await
                .map(|m| m.len())
                .unwrap_or(0);
        }
    }

    total
}

/// Get storage statistics for the application.
/// 获取应用的存储统计信息。
#[tauri::command]
pub async fn get_storage_stats(
    _trace: Option<TraceMetadata>,
) -> Result<StorageStats, CommandError> {
    let span = info_span!(
        "command.storage.get_stats",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let app_dirs = DirsAppDirsAdapter::new()
            .get_app_dirs()
            .map_err(|e| CommandError::InternalError(format!("Failed to resolve app dirs: {}", e)))?;

        let paths = AppPaths::from_app_dirs(&app_dirs);

        // Compute sizes concurrently
        let (database_bytes, vault_bytes, cache_bytes, logs_bytes) = tokio::join!(
            dir_size(&paths.db_path),
            dir_size(&paths.vault_dir),
            dir_size(&paths.cache_dir),
            dir_size(&paths.logs_dir),
        );

        let total_bytes = database_bytes + vault_bytes + cache_bytes + logs_bytes;
        let data_dir = app_dirs.app_data_root.to_string_lossy().to_string();

        tracing::info!(
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            total_bytes,
            "Storage stats computed"
        );

        Ok(StorageStats {
            database_bytes,
            vault_bytes,
            cache_bytes,
            logs_bytes,
            total_bytes,
            data_dir,
        })
    }
    .instrument(span)
    .await
}

/// Clear cache directory (thumbnails, temporary files).
/// 清除缓存目录（缩略图、临时文件）。
#[tauri::command]
pub async fn clear_cache(
    _trace: Option<TraceMetadata>,
) -> Result<u64, CommandError> {
    let span = info_span!(
        "command.storage.clear_cache",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let app_dirs = DirsAppDirsAdapter::new()
            .get_app_dirs()
            .map_err(|e| CommandError::InternalError(format!("Failed to resolve app dirs: {}", e)))?;

        let paths = AppPaths::from_app_dirs(&app_dirs);
        let freed = dir_size(&paths.cache_dir).await;

        if paths.cache_dir.exists() {
            // Remove contents but keep the directory
            let mut entries = tokio::fs::read_dir(&paths.cache_dir)
                .await
                .map_err(|e| CommandError::InternalError(format!("Failed to read cache dir: {}", e)))?;

            while let Ok(Some(entry)) = entries.next_entry().await {
                let entry_path = entry.path();
                if entry_path.is_dir() {
                    if let Err(e) = tokio::fs::remove_dir_all(&entry_path).await {
                        tracing::warn!(path = %entry_path.display(), error = %e, "Failed to remove cache subdirectory");
                    }
                } else if let Err(e) = tokio::fs::remove_file(&entry_path).await {
                    tracing::warn!(path = %entry_path.display(), error = %e, "Failed to remove cache file");
                }
            }
        }

        tracing::info!(freed_bytes = freed, "Cache cleared");
        Ok(freed)
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
        tracing::info!(total_entries = total, "Starting bulk clipboard history deletion");

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
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.storage.open_data_dir",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let app_dirs = DirsAppDirsAdapter::new()
            .get_app_dirs()
            .map_err(|e| CommandError::InternalError(format!("Failed to resolve app dirs: {}", e)))?;

        let dir = &app_dirs.app_data_root;
        if !dir.exists() {
            return Err(CommandError::NotFound("Data directory does not exist".to_string()));
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(dir)
                .spawn()
                .map_err(|e| CommandError::InternalError(format!("Failed to open directory: {}", e)))?;
        }

        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(dir)
                .spawn()
                .map_err(|e| CommandError::InternalError(format!("Failed to open directory: {}", e)))?;
        }

        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(dir)
                .spawn()
                .map_err(|e| CommandError::InternalError(format!("Failed to open directory: {}", e)))?;
        }

        tracing::info!(dir = %dir.display(), "Opened data directory");
        Ok(())
    }
    .instrument(span)
    .await
}
