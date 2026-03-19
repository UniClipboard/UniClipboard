//! Clipboard-related Tauri commands
//! 剪贴板相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use crate::models::{
    ClipboardEntriesResponse, ClipboardEntryDetail, ClipboardEntryProjection,
    ClipboardEntryResource, ClipboardImageItemDto, ClipboardItemDto, ClipboardItemResponse,
    ClipboardLinkItemDto, ClipboardStats, ClipboardTextItemDto,
};
use base64::Engine;
use std::sync::Arc;
use tauri::State;
use tracing::{info_span, Instrument};
use uc_app::usecases::clipboard::ClipboardIntegrationMode;
use uc_app::usecases::clipboard::ClipboardUseCases;
use uc_core::clipboard::link_utils::extract_domain;
use uc_core::ids::EntryId;
use uc_core::security::state::EncryptionState;
use uc_platform::ports::observability::TraceMetadata;

/// Get clipboard history entries (preview only)
/// 获取剪贴板历史条目（仅预览）
#[tauri::command]
pub async fn get_clipboard_entries(
    runtime: State<'_, Arc<AppRuntime>>,
    limit: Option<usize>,
    offset: Option<usize>,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardEntriesResponse, CommandError> {
    let resolved_limit = limit.unwrap_or(50);
    let resolved_offset = offset.unwrap_or(0);

    let span = info_span!(
        "command.clipboard.get_entries",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        limit = resolved_limit,
        offset = resolved_offset,
    );
    record_trace_fields(&span, &_trace);

    async move {
        // Check encryption session readiness to avoid decryption failures during startup
        let encryption_state = runtime.encryption_state().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to check encryption state");
            CommandError::InternalError(format!("Failed to check encryption state: {}", e))
        })?;

        let session_ready = runtime.is_encryption_ready().await;
        if should_return_not_ready(encryption_state, session_ready) {
            tracing::warn!(
                "Encryption initialized but session not ready yet, returning not-ready response. \
                 This typically happens during app startup before secure storage unlock completes."
            );
            return Ok(ClipboardEntriesResponse::NotReady);
        }

        let uc = runtime.usecases().list_entry_projections();
        let dtos = uc
            .execute(resolved_limit, resolved_offset)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to get clipboard entry projections");
                CommandError::InternalError(e.to_string())
            })?;

        // Map DTOs to command layer models
        let projections: Vec<ClipboardEntryProjection> = dtos
            .into_iter()
            .map(|dto| {
                let link_domains = dto
                    .link_urls
                    .as_ref()
                    .map(|urls| urls.iter().filter_map(|u| extract_domain(u)).collect());
                ClipboardEntryProjection {
                    id: dto.id,
                    preview: dto.preview,
                    has_detail: dto.has_detail,
                    size_bytes: dto.size_bytes,
                    captured_at: dto.captured_at,
                    content_type: dto.content_type,
                    thumbnail_url: dto.thumbnail_url,
                    is_encrypted: dto.is_encrypted,
                    is_favorited: dto.is_favorited,
                    updated_at: dto.updated_at,
                    active_time: dto.active_time,
                    file_transfer_status: dto.file_transfer_status,
                    file_transfer_reason: dto.file_transfer_reason,
                    link_urls: dto.link_urls,
                    link_domains,
                    file_sizes: dto.file_sizes,
                }
            })
            .collect();

        tracing::info!(count = projections.len(), "Retrieved clipboard entries");
        Ok(ClipboardEntriesResponse::Ready {
            entries: projections,
        })
    }
    .instrument(span)
    .await
}

fn should_return_not_ready(state: EncryptionState, session_ready: bool) -> bool {
    matches!(state, EncryptionState::Initialized) && !session_ready
}

/// Get aggregate clipboard statistics (total_items, total_size).
/// 获取剪贴板统计信息（总条目数和总大小）。
#[tauri::command]
pub async fn get_clipboard_stats(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardStats, CommandError> {
    let span = info_span!(
        "command.clipboard.get_stats",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let uc = runtime.usecases().list_entry_projections();
        let dtos = uc.execute(1_000, 0).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to list clipboard entry projections for stats");
            CommandError::InternalError(e.to_string())
        })?;

        let stats = ClipboardUseCases::compute_stats(&dtos);
        Ok(ClipboardStats {
            total_items: stats.total_items,
            total_size: stats.total_size,
        })
    }
    .instrument(span)
    .await
}

/// Toggle favorite state for a clipboard item.
/// 切换剪贴板条目的收藏状态。
#[tauri::command]
pub async fn toggle_favorite_clipboard_item(
    runtime: State<'_, Arc<AppRuntime>>,
    id: String,
    is_favorited: bool,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.clipboard.toggle_favorite",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %id,
        is_favorited,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let entry_id = EntryId::from(id.clone());

        let uc = runtime.usecases().toggle_favorite_clipboard_entry();
        match uc.execute(&entry_id, is_favorited).await {
            Ok(true) => {
                tracing::info!(
                    entry_id = %entry_id,
                    is_favorited,
                    "Toggled favorite for clipboard entry",
                );
                Ok(())
            }
            Ok(false) => {
                tracing::warn!(
                    entry_id = %entry_id,
                    is_favorited,
                    "Entry not found for favorite toggle",
                );
                Err(CommandError::NotFound("Entry not found".to_string()))
            }
            Err(e) => {
                tracing::error!(
                    entry_id = %entry_id,
                    error = %e,
                    "Failed to toggle favorite for clipboard entry",
                );
                Err(CommandError::InternalError(e.to_string()))
            }
        }
    }
    .instrument(span)
    .await
}

/// Get a single clipboard entry by entry_id, returning ClipboardEntriesResponse.
/// Uses execute_single for efficient single-entry lookup.
/// 通过 entry_id 获取单个剪贴板条目，使用 execute_single 高效查找。
#[tauri::command]
pub async fn get_clipboard_entry(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardEntriesResponse, CommandError> {
    let span = info_span!(
        "command.clipboard.get_entry_single",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &_trace);

    async move {
        // Check encryption session readiness
        let encryption_state = runtime.encryption_state().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to check encryption state");
            CommandError::InternalError(format!("Failed to check encryption state: {}", e))
        })?;
        let session_ready = runtime.is_encryption_ready().await;
        if should_return_not_ready(encryption_state, session_ready) {
            return Ok(ClipboardEntriesResponse::NotReady);
        }

        let uc = runtime.usecases().list_entry_projections();
        let projection = uc.execute_single(&entry_id).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get single entry projection");
            CommandError::InternalError(e.to_string())
        })?;

        let entries: Vec<ClipboardEntryProjection> = match projection {
            Some(dto) => {
                let link_domains = dto
                    .link_urls
                    .as_ref()
                    .map(|urls| urls.iter().filter_map(|u| extract_domain(u)).collect());
                vec![ClipboardEntryProjection {
                    id: dto.id,
                    preview: dto.preview,
                    has_detail: dto.has_detail,
                    size_bytes: dto.size_bytes,
                    captured_at: dto.captured_at,
                    content_type: dto.content_type,
                    thumbnail_url: dto.thumbnail_url,
                    is_encrypted: dto.is_encrypted,
                    is_favorited: dto.is_favorited,
                    updated_at: dto.updated_at,
                    active_time: dto.active_time,
                    file_transfer_status: dto.file_transfer_status,
                    file_transfer_reason: dto.file_transfer_reason,
                    link_urls: dto.link_urls,
                    link_domains,
                    file_sizes: dto.file_sizes,
                }]
            }
            None => vec![],
        };

        Ok(ClipboardEntriesResponse::Ready { entries })
    }
    .instrument(span)
    .await
}

/// Get a single clipboard item by ID, returning a response matching the
/// frontend ClipboardItemResponse contract. Returns Ok(None) when the entry
/// does not exist.
/// 获取单个剪贴板条目，返回与前端 ClipboardItemResponse 匹配的响应。
#[tauri::command]
pub async fn get_clipboard_item(
    runtime: State<'_, Arc<AppRuntime>>,
    id: String,
    full_content: Option<bool>,
    _trace: Option<TraceMetadata>,
) -> Result<Option<ClipboardItemResponse>, CommandError> {
    let resolved_full = full_content.unwrap_or(false);

    let span = info_span!(
        "command.clipboard.get_item",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %id,
        full_content = resolved_full,
    );
    record_trace_fields(&span, &_trace);

    async move {
        // Use list_entry_projections to find the entry and build a response
        // that matches the frontend contract. This reuses existing projection
        // infrastructure rather than duplicating query logic.
        let uc = runtime.usecases().list_entry_projections();
        let projections = uc.execute(1_000, 0).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to list projections for get_clipboard_item");
            CommandError::InternalError(e.to_string())
        })?;

        let projection = projections.into_iter().find(|p| p.id == id);

        match projection {
            None => {
                tracing::info!(entry_id = %id, "Clipboard item not found");
                Ok(None)
            }
            Some(proj) => {
                let content_type_lower = proj.content_type.to_ascii_lowercase();
                let is_image = content_type_lower.starts_with("image/");

                let item = if is_image {
                    ClipboardItemDto {
                        text: None,
                        image: Some(ClipboardImageItemDto {
                            thumbnail: proj.thumbnail_url.clone(),
                            size: proj.size_bytes,
                            width: 0,
                            height: 0,
                        }),
                        file: None,
                        link: None,
                        code: None,
                        unknown: None,
                    }
                } else if let Some(urls) = proj.link_urls {
                    let domains = urls.iter().filter_map(|u| extract_domain(u)).collect();
                    ClipboardItemDto {
                        text: None,
                        image: None,
                        file: None,
                        link: Some(ClipboardLinkItemDto { urls, domains }),
                        code: None,
                        unknown: None,
                    }
                } else {
                    ClipboardItemDto {
                        text: Some(ClipboardTextItemDto {
                            display_text: proj.preview.clone(),
                            has_detail: proj.has_detail,
                            size: proj.size_bytes,
                        }),
                        image: None,
                        file: None,
                        link: None,
                        code: None,
                        unknown: None,
                    }
                };

                let response = ClipboardItemResponse {
                    id: proj.id,
                    is_downloaded: true,
                    is_favorited: proj.is_favorited,
                    created_at: proj.captured_at,
                    updated_at: proj.updated_at,
                    active_time: proj.active_time,
                    item,
                };

                tracing::info!(entry_id = %id, "Retrieved clipboard item");
                Ok(Some(response))
            }
        }
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod restore_tests {
    use super::should_return_not_ready;
    use uc_core::security::state::EncryptionState;

    #[test]
    fn returns_not_ready_only_when_initialized_and_session_not_ready() {
        assert!(should_return_not_ready(EncryptionState::Initialized, false));
        assert!(!should_return_not_ready(EncryptionState::Initialized, true));
        assert!(!should_return_not_ready(
            EncryptionState::Uninitialized,
            false
        ));
    }
}

/// Deletes a clipboard entry identified by `entry_id`.
///
/// This command converts the provided `entry_id` to the domain `EntryId` type and invokes the runtime's
/// delete clipboard-entry use case; on success it returns without value, otherwise it returns a stringified error.
///
/// # Examples
///
/// ```no_run
/// # use std::sync::Arc;
/// # async fn example(runtime: tauri::State<'_, Arc<uc_tauri::bootstrap::AppRuntime>>) {
/// // Tauri provides `State<Arc<AppRuntime>>` when invoking commands from the frontend.
/// let result = uc_tauri::commands::clipboard::delete_clipboard_entry(
///     runtime,
///     "entry-id-123".to_string(),
///     None,
/// )
/// .await;
/// match result {
///     Ok(()) => println!("Deleted"),
///     Err(e) => eprintln!("Delete failed: {}", e),
/// }
/// # }
/// ```
#[tauri::command]
pub async fn delete_clipboard_entry(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.clipboard.delete_entry",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let parsed_id = uc_core::ids::EntryId::from(entry_id.clone());
        let use_case = runtime.usecases().delete_clipboard_entry();
        use_case.execute(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to delete entry");
            CommandError::InternalError(e.to_string())
        })?;

        tracing::info!(entry_id = %entry_id, "Deleted clipboard entry");
        Ok(())
    }
    .instrument(span)
    .await
}

/// Get full clipboard entry detail
/// 获取剪贴板条目完整详情
#[tauri::command]
pub async fn get_clipboard_entry_detail(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardEntryDetail, CommandError> {
    let span = info_span!(
        "command.clipboard.get_entry_detail",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let parsed_id = uc_core::ids::EntryId::from(entry_id.clone());
        let use_case = runtime.usecases().get_entry_detail();
        let result = use_case.execute(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to get entry detail");
            CommandError::InternalError(e.to_string())
        })?;

        let detail = ClipboardEntryDetail {
            id: result.id,
            content: result.content,
            size_bytes: result.size_bytes,
            content_type: result.mime_type.unwrap_or_else(|| "unknown".to_string()),
            is_favorited: false,
            updated_at: result.created_at_ms,
            active_time: result.active_time_ms,
        };

        tracing::info!(entry_id = %entry_id, "Retrieved clipboard entry detail");
        Ok(detail)
    }
    .instrument(span)
    .await
}

/// Get clipboard entry resource metadata
/// 获取剪贴板条目资源元信息
#[tauri::command]
pub async fn get_clipboard_entry_resource(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<ClipboardEntryResource, CommandError> {
    let span = info_span!(
        "command.clipboard.get_entry_resource",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let parsed_id = uc_core::ids::EntryId::from(entry_id.clone());
        let use_case = runtime.usecases().get_entry_resource();
        let result = use_case.execute(&parsed_id).await.map_err(|e| {
            tracing::error!(
                error = %e,
                entry_id = %entry_id,
                "Failed to get entry resource"
            );
            CommandError::InternalError(e.to_string())
        })?;

        let resource = ClipboardEntryResource {
            blob_id: result.blob_id.map(|b| b.to_string()),
            mime_type: result.mime_type.unwrap_or_else(|| "unknown".to_string()),
            size_bytes: result.size_bytes,
            url: result.url,
            inline_data: result
                .inline_data
                .map(|bytes| base64::engine::general_purpose::STANDARD.encode(&bytes)),
        };

        tracing::info!(entry_id = %entry_id, "Retrieved clipboard entry resource");
        Ok(resource)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn sync_clipboard_items(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    sync_clipboard_items_impl(runtime.as_ref(), _trace).await
}

async fn sync_clipboard_items_impl(
    runtime: &AppRuntime,
    trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!(
        "command.clipboard.sync_items",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &trace);

    async move {
        if matches!(
            runtime.clipboard_integration_mode(),
            ClipboardIntegrationMode::Passive
        ) {
            return Err(CommandError::ValidationError("Clipboard sync disabled in passive mode".to_string()));
        }

        let outbound_sync_uc = runtime.usecases().sync_outbound_clipboard();
        match tokio::task::spawn_blocking(move || {
            outbound_sync_uc.execute_current_snapshot(uc_core::ClipboardChangeOrigin::LocalCapture)
        })
        .await
        {
            Ok(Ok(())) => Ok(true),
            Ok(Err(err)) => {
                tracing::warn!(error = %err, "Outbound clipboard sync command failed");
                Err(CommandError::InternalError(format!("sync failed: {err}")))
            }
            Err(join_err) if join_err.is_cancelled() => {
                tracing::warn!("Outbound clipboard sync task cancelled");
                Err(CommandError::Cancelled("sync task cancelled".to_string()))
            }
            Err(join_err) => {
                tracing::warn!(error = %join_err, "Outbound clipboard sync command task join failed");
                Err(CommandError::InternalError(format!("sync task panic: {join_err}")))
            }
        }
    }
    .instrument(span)
    .await
}

/// Restore clipboard entry to system clipboard.
/// 将历史剪贴板条目恢复到系统剪贴板。
#[tauri::command]
pub async fn restore_clipboard_entry(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    restore_clipboard_entry_impl(runtime.as_ref(), entry_id, _trace).await
}

async fn restore_clipboard_entry_impl(
    runtime: &AppRuntime,
    entry_id: String,
    trace: Option<TraceMetadata>,
) -> Result<bool, CommandError> {
    let span = info_span!(
        "command.clipboard.restore_entry",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        entry_id = %entry_id,
    );
    record_trace_fields(&span, &trace);

    async move {
        let parsed_id = EntryId::from(entry_id.clone());

        let restore_uc = runtime.usecases().restore_clipboard_selection();
        let snapshot = restore_uc.build_snapshot(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to build restore snapshot");
            CommandError::InternalError(e.to_string())
        })?;

        let touch_uc = runtime.usecases().touch_clipboard_entry();
        let touched = touch_uc.execute(&parsed_id).await.map_err(|e| {
            tracing::error!(error = %e, entry_id = %entry_id, "Failed to update entry active time");
            CommandError::InternalError(e.to_string())
        })?;

        if !touched {
            tracing::warn!(entry_id = %entry_id, "Entry not found when touching active time");
            return Err(CommandError::NotFound("Entry not found".to_string()));
        }

        let outbound_snapshot = snapshot.clone();
        restore_uc.restore_snapshot(snapshot).await.map_err(|err| {
            tracing::error!(error = %err, entry_id = %entry_id, "Failed to write restore snapshot");
            CommandError::InternalError(err.to_string())
        })?;

        let outbound_sync_uc = runtime.usecases().sync_outbound_clipboard();
        match tokio::task::spawn_blocking(move || {
            outbound_sync_uc.execute(outbound_snapshot, uc_core::ClipboardChangeOrigin::LocalRestore, None, vec![])
        })
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                tracing::warn!(error = %err, entry_id = %entry_id, "Restore outbound sync failed");
            }
            Err(err) => {
                tracing::warn!(error = %err, entry_id = %entry_id, "Restore outbound sync task join failed");
            }
        }

        if let Some(app) = runtime.app_handle().as_ref() {
            if let Err(err) = crate::events::forward_clipboard_event(
                app,
                crate::events::ClipboardEvent::NewContent {
                    entry_id: entry_id.clone(),
                    preview: "Clipboard restored".to_string(),
                    origin: "local".to_string(),
                },
            ) {
                tracing::warn!(error = %err, entry_id = %entry_id, "Failed to emit restore event");
            }
        } else {
            tracing::debug!("AppHandle not available, skipping restore event emission");
        }

        Ok(true)
    }
    .instrument(span)
    .await
}

/// Copy file references from a clipboard entry to the system clipboard.
/// 将剪贴板条目中的文件引用复制到系统剪贴板。
///
/// Used when user right-clicks a file entry in Dashboard and selects "Copy".
/// Validates file existence before writing -- returns error if any file is deleted.
#[tauri::command]
pub async fn copy_file_to_clipboard(
    runtime: State<'_, Arc<AppRuntime>>,
    entry_id: String,
) -> Result<(), String> {
    let uc = runtime.usecases().copy_file_to_clipboard();
    let id = EntryId::from(entry_id);
    uc.execute(&id).await.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::{restore_clipboard_entry_impl, sync_clipboard_items_impl};
    use crate::bootstrap::AppRuntime;
    use crate::commands::error::CommandError;
    use crate::test_utils::noop_network_ports;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use tokio::sync::mpsc;
    use uc_app::AppDeps;
    use uc_core::clipboard::{
        ClipboardEntry, ClipboardSelection, ClipboardSelectionDecision, MimeType,
        PersistedClipboardRepresentation, SelectionPolicyVersion, SystemClipboardSnapshot,
    };
    use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
    use uc_core::ports::clipboard::{
        ClipboardPayloadResolverPort, GeneratedThumbnail, ProcessingUpdateOutcome,
        RepresentationCachePort, ResolvedClipboardPayload, SpoolQueuePort, SpoolRequest,
        ThumbnailGeneratorPort, ThumbnailRepositoryPort,
    };
    use uc_core::ports::security::encryption_state::EncryptionStatePort;
    use uc_core::ports::security::key_scope::KeyScopePort;
    use uc_core::ports::*;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionError, KdfParams, Kek, KeyScope, KeySlot,
        MasterKey, Passphrase,
    };
    use uc_core::security::state::{EncryptionState, EncryptionStateError};
    use uc_core::{Blob, BlobId, ContentHash, DeviceId};
    use uc_infra::clipboard::InMemoryClipboardChangeOrigin;
    use uc_platform::ports::{WatcherControlError, WatcherControlPort};

    struct MockEntryRepository {
        entry: Option<ClipboardEntry>,
        touch_result: bool,
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    struct EnvVarGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let original = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, original }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    struct MockSelectionRepository {
        selection: Option<ClipboardSelectionDecision>,
    }

    struct MockRepresentationRepository {
        reps: HashMap<RepresentationId, PersistedClipboardRepresentation>,
    }

    struct MockSystemClipboard {
        calls: Arc<Mutex<Vec<&'static str>>>,
    }

    struct MockDeviceIdentity;

    struct NoopClipboard;
    struct NoopPort;

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn get_entry(&self, _entry_id: &EntryId) -> anyhow::Result<Option<ClipboardEntry>> {
            Ok(self.entry.clone())
        }

        async fn list_entries(
            &self,
            _limit: usize,
            _offset: usize,
        ) -> anyhow::Result<Vec<ClipboardEntry>> {
            Ok(vec![])
        }

        async fn touch_entry(
            &self,
            _entry_id: &EntryId,
            _active_time_ms: i64,
        ) -> anyhow::Result<bool> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push("touch");
            }
            Ok(self.touch_result)
        }

        async fn delete_entry(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for MockSelectionRepository {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            Ok(self.selection.clone())
        }

        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepository {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            representation_id: &RepresentationId,
        ) -> anyhow::Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.reps.get(representation_id).cloned())
        }

        async fn get_representation_by_id(
            &self,
            representation_id: &RepresentationId,
        ) -> anyhow::Result<Option<PersistedClipboardRepresentation>> {
            Ok(self.reps.get(representation_id).cloned())
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &BlobId,
        ) -> anyhow::Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &BlobId,
        ) -> anyhow::Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> anyhow::Result<ProcessingUpdateOutcome> {
            Ok(ProcessingUpdateOutcome::NotFound)
        }
    }

    impl SystemClipboardPort for MockSystemClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            if let Ok(mut calls) = self.calls.lock() {
                calls.push("write");
            }
            Ok(())
        }
    }

    impl SystemClipboardPort for NoopClipboard {
        fn read_snapshot(&self) -> anyhow::Result<SystemClipboardSnapshot> {
            Ok(SystemClipboardSnapshot {
                ts_ms: 0,
                representations: vec![],
            })
        }

        fn write_snapshot(&self, _snapshot: SystemClipboardSnapshot) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device-test")
        }
    }

    #[async_trait]
    impl ClipboardEventWriterPort for NoopPort {
        async fn insert_event(
            &self,
            _event: &uc_core::ClipboardEvent,
            _representations: &Vec<uc_core::PersistedClipboardRepresentation>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn delete_event_and_representations(
            &self,
            _event_id: &uc_core::ids::EventId,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl SelectRepresentationPolicyPort for NoopPort {
        fn select(
            &self,
            _snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<
            uc_core::clipboard::ClipboardSelection,
            uc_core::clipboard::PolicyError,
        > {
            Err(uc_core::clipboard::PolicyError::NoUsableRepresentation)
        }
    }

    #[async_trait]
    impl ClipboardRepresentationNormalizerPort for NoopPort {
        async fn normalize(
            &self,
            _observed: &uc_core::clipboard::ObservedClipboardRepresentation,
        ) -> anyhow::Result<uc_core::PersistedClipboardRepresentation> {
            Err(anyhow::anyhow!("noop"))
        }
    }

    #[async_trait]
    impl RepresentationCachePort for NoopPort {
        async fn put(&self, _rep_id: &uc_core::ids::RepresentationId, _bytes: Vec<u8>) {}

        async fn get(&self, _rep_id: &uc_core::ids::RepresentationId) -> Option<Vec<u8>> {
            None
        }

        async fn mark_completed(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn mark_spooling(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn remove(&self, _rep_id: &uc_core::ids::RepresentationId) {}
    }

    #[async_trait]
    impl SpoolQueuePort for NoopPort {
        async fn enqueue(&self, _request: SpoolRequest) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardPayloadResolverPort for NoopPort {
        async fn resolve(
            &self,
            _representation: &uc_core::clipboard::PersistedClipboardRepresentation,
        ) -> anyhow::Result<ResolvedClipboardPayload> {
            Err(anyhow::anyhow!("NoopPayloadResolver"))
        }
    }

    #[async_trait]
    impl EncryptionPort for NoopPort {
        async fn derive_kek(
            &self,
            _passphrase: &Passphrase,
            _salt: &[u8],
            _kdf: &KdfParams,
        ) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn wrap_master_key(
            &self,
            _kek: &Kek,
            _master_key: &MasterKey,
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn unwrap_master_key(
            &self,
            _kek: &Kek,
            _wrapped: &EncryptedBlob,
        ) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn encrypt_blob(
            &self,
            _master_key: &MasterKey,
            _plaintext: &[u8],
            _aad: &[u8],
            _aead: EncryptionAlgo,
        ) -> Result<EncryptedBlob, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn decrypt_blob(
            &self,
            _master_key: &MasterKey,
            _encrypted: &EncryptedBlob,
            _aad: &[u8],
        ) -> Result<Vec<u8>, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }
    }

    #[async_trait]
    impl EncryptionSessionPort for NoopPort {
        async fn is_ready(&self) -> bool {
            false
        }

        async fn get_master_key(&self) -> Result<MasterKey, EncryptionError> {
            Err(EncryptionError::NotInitialized)
        }

        async fn set_master_key(&self, _master_key: MasterKey) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn clear(&self) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl EncryptionStatePort for NoopPort {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Err(EncryptionStateError::LoadError("noop".to_string()))
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    #[async_trait]
    impl KeyScopePort for NoopPort {
        async fn current_scope(
            &self,
        ) -> Result<KeyScope, uc_core::ports::security::key_scope::ScopeError> {
            Err(uc_core::ports::security::key_scope::ScopeError::FailedToGetCurrentScope)
        }
    }

    #[async_trait]
    impl KeyMaterialPort for NoopPort {
        async fn load_kek(&self, _scope: &KeyScope) -> Result<Kek, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_kek(&self, _scope: &KeyScope, _kek: &Kek) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_kek(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn load_keyslot(&self, _scope: &KeyScope) -> Result<KeySlot, EncryptionError> {
            Err(EncryptionError::KeyNotFound)
        }

        async fn store_keyslot(&self, _keyslot: &KeySlot) -> Result<(), EncryptionError> {
            Ok(())
        }

        async fn delete_keyslot(&self, _scope: &KeyScope) -> Result<(), EncryptionError> {
            Ok(())
        }
    }

    #[async_trait]
    impl WatcherControlPort for NoopPort {
        async fn start_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }

        async fn stop_watcher(&self) -> Result<(), WatcherControlError> {
            Ok(())
        }
    }

    #[async_trait]
    impl DeviceRepositoryPort for NoopPort {
        async fn find_by_id(
            &self,
            _id: &uc_core::device::DeviceId,
        ) -> Result<Option<uc_core::device::Device>, uc_core::ports::errors::DeviceRepositoryError>
        {
            Ok(None)
        }

        async fn save(
            &self,
            _device: uc_core::device::Device,
        ) -> Result<(), uc_core::ports::errors::DeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _id: &uc_core::device::DeviceId,
        ) -> Result<(), uc_core::ports::errors::DeviceRepositoryError> {
            Ok(())
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<uc_core::device::Device>, uc_core::ports::errors::DeviceRepositoryError>
        {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for NoopPort {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<Option<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<uc_core::network::PairedDevice>, PairedDeviceRepositoryError> {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: uc_core::network::PairedDevice,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::PeerId,
            _state: uc_core::network::PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::PeerId,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &uc_core::PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardTransportPort for NoopPort {
        async fn send_clipboard(
            &self,
            _peer_id: &str,
            _encrypted_data: Arc<[u8]>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Arc<[u8]>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(
            &self,
        ) -> anyhow::Result<
            tokio::sync::mpsc::Receiver<(uc_core::network::ClipboardMessage, Option<Vec<u8>>)>,
        > {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl PeerDirectoryPort for NoopPort {
        async fn get_discovered_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
            Ok(vec![])
        }

        async fn get_connected_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "noop".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl PairingTransportPort for NoopPort {
        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(
            &self,
            _message: uc_core::network::PairingMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn close_pairing_session(
            &self,
            _session_id: String,
            _reason: Option<String>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unpair_device(&self, _peer_id: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl NetworkEventPort for NoopPort {
        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::NetworkEvent>> {
            let (_tx, rx) = mpsc::channel(1);
            Ok(rx)
        }
    }

    #[async_trait]
    impl uc_core::ports::NetworkControlPort for NoopPort {
        async fn start_network(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl uc_core::ports::SetupStatusPort for NoopPort {
        async fn get_status(&self) -> anyhow::Result<uc_core::setup::SetupStatus> {
            Ok(uc_core::setup::SetupStatus::default())
        }

        async fn set_status(&self, _status: &uc_core::setup::SetupStatus) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl uc_core::ports::SecureStoragePort for NoopPort {
        fn get(&self, _key: &str) -> Result<Option<Vec<u8>>, uc_core::ports::SecureStorageError> {
            Ok(None)
        }

        fn set(&self, _key: &str, _value: &[u8]) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }

        fn delete(&self, _key: &str) -> Result<(), uc_core::ports::SecureStorageError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BlobStorePort for NoopPort {
        async fn put(
            &self,
            _blob_id: &BlobId,
            _data: &[u8],
        ) -> anyhow::Result<(std::path::PathBuf, Option<i64>)> {
            Ok((std::path::PathBuf::from("/tmp/noop"), None))
        }

        async fn get(&self, _blob_id: &BlobId) -> anyhow::Result<Vec<u8>> {
            Ok(vec![])
        }
    }

    #[async_trait]
    impl BlobRepositoryPort for NoopPort {
        async fn insert_blob(&self, _blob: &Blob) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_hash(&self, _content_hash: &ContentHash) -> anyhow::Result<Option<Blob>> {
            Ok(None)
        }
    }

    #[async_trait]
    impl BlobWriterPort for NoopPort {
        async fn write_if_absent(
            &self,
            _content_id: &ContentHash,
            _plaintext_bytes: &[u8],
        ) -> anyhow::Result<Blob> {
            Err(anyhow::anyhow!("noop blob writer"))
        }
    }

    #[async_trait]
    impl ThumbnailRepositoryPort for NoopPort {
        async fn get_by_representation_id(
            &self,
            _representation_id: &uc_core::ids::RepresentationId,
        ) -> anyhow::Result<Option<uc_core::clipboard::ThumbnailMetadata>> {
            Ok(None)
        }

        async fn insert_thumbnail(
            &self,
            _metadata: &uc_core::clipboard::ThumbnailMetadata,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ThumbnailGeneratorPort for NoopPort {
        async fn generate_thumbnail(
            &self,
            _image_bytes: &[u8],
        ) -> anyhow::Result<GeneratedThumbnail> {
            Err(anyhow::anyhow!("noop thumbnail generator"))
        }

        async fn generate_thumbnail_from_rgba(
            &self,
            _rgba_bytes: &[u8],
            _width: u32,
            _height: u32,
        ) -> anyhow::Result<GeneratedThumbnail> {
            self.generate_thumbnail(&[]).await
        }
    }

    #[async_trait]
    impl SettingsPort for NoopPort {
        async fn load(&self) -> anyhow::Result<uc_core::settings::model::Settings> {
            Err(anyhow::anyhow!("noop settings"))
        }

        async fn save(&self, _settings: &uc_core::settings::model::Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    impl ClockPort for NoopPort {
        fn now_ms(&self) -> i64 {
            0
        }
    }

    impl ContentHashPort for NoopPort {
        fn hash_bytes(&self, _bytes: &[u8]) -> anyhow::Result<ContentHash> {
            Err(anyhow::anyhow!("noop hash"))
        }
    }

    impl uc_core::ports::FileManagerPort for NoopPort {
        fn open_directory(
            &self,
            _path: &std::path::Path,
        ) -> Result<(), uc_core::ports::FileManagerError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl uc_core::ports::CacheFsPort for NoopPort {
        async fn exists(&self, _path: &std::path::Path) -> bool {
            false
        }
        async fn read_dir(
            &self,
            _path: &std::path::Path,
        ) -> anyhow::Result<Vec<uc_core::ports::CacheFsDirEntry>> {
            Ok(vec![])
        }
        async fn remove_dir_all(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn remove_file(&self, _path: &std::path::Path) -> anyhow::Result<()> {
            Ok(())
        }
        async fn dir_size(&self, _path: &std::path::Path) -> anyhow::Result<u64> {
            Ok(0)
        }
    }

    fn test_storage_paths() -> uc_app::app_paths::AppPaths {
        uc_app::app_paths::AppPaths {
            db_path: std::path::PathBuf::from("/tmp/uniclipboard-test/uniclipboard.db"),
            vault_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/vault"),
            settings_path: std::path::PathBuf::from("/tmp/uniclipboard-test/settings.json"),
            logs_dir: std::path::PathBuf::from("/tmp/uniclipboard-test/logs"),
            cache_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache"),
            file_cache_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache/file-cache"),
            spool_dir: std::path::PathBuf::from("/tmp/uniclipboard-test-cache/spool"),
            app_data_root: std::path::PathBuf::from("/tmp/uniclipboard-test"),
        }
    }

    #[tokio::test]
    async fn restore_entry_returns_error_before_clipboard_write_when_touch_fails() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let entry_id = EntryId::from("entry-1");
        let event_id = EventId::from("event-1");
        let rep_id = RepresentationId::from("rep-1");

        let entry = ClipboardEntry::new(entry_id.clone(), event_id.clone(), 0, None, 5);
        let selection = ClipboardSelection {
            primary_rep_id: rep_id.clone(),
            secondary_rep_ids: vec![],
            preview_rep_id: rep_id.clone(),
            paste_rep_id: rep_id.clone(),
            policy_version: SelectionPolicyVersion::V1,
        };
        let selection = ClipboardSelectionDecision::new(entry_id.clone(), selection);
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType::text_plain()),
            5,
            Some(b"hello".to_vec()),
            None,
        );

        let mut reps = HashMap::new();
        reps.insert(rep_id, rep);

        let (worker_tx, _worker_rx) = mpsc::channel(1);
        let deps = AppDeps {
            clipboard: uc_app::ClipboardPorts {
                clipboard: Arc::new(NoopClipboard),
                system_clipboard: Arc::new(MockSystemClipboard {
                    calls: calls.clone(),
                }),
                clipboard_entry_repo: Arc::new(MockEntryRepository {
                    entry: Some(entry),
                    touch_result: false,
                    calls: calls.clone(),
                }),
                clipboard_event_repo: Arc::new(NoopPort),
                representation_repo: Arc::new(MockRepresentationRepository { reps }),
                representation_normalizer: Arc::new(NoopPort),
                selection_repo: Arc::new(MockSelectionRepository {
                    selection: Some(selection),
                }),
                representation_policy: Arc::new(NoopPort),
                representation_cache: Arc::new(NoopPort),
                spool_queue: Arc::new(NoopPort),
                clipboard_change_origin: Arc::new(InMemoryClipboardChangeOrigin::new()),
                worker_tx,
                payload_resolver: Arc::new(NoopPort),
            },
            security: uc_app::SecurityPorts {
                encryption: Arc::new(NoopPort),
                encryption_session: Arc::new(NoopPort),
                encryption_state: Arc::new(NoopPort),
                key_scope: Arc::new(NoopPort),
                secure_storage: Arc::new(NoopPort),
                key_material: Arc::new(NoopPort),
            },
            device: uc_app::DevicePorts {
                device_repo: Arc::new(NoopPort),
                device_identity: Arc::new(MockDeviceIdentity),
                paired_device_repo: Arc::new(NoopPort),
            },
            network_ports: noop_network_ports(),
            network_control: Arc::new(NoopPort),
            setup_status: Arc::new(NoopPort),
            storage: uc_app::StoragePorts {
                blob_store: Arc::new(NoopPort),
                blob_repository: Arc::new(NoopPort),
                blob_writer: Arc::new(NoopPort),
                thumbnail_repo: Arc::new(NoopPort),
                thumbnail_generator: Arc::new(NoopPort),
                file_transfer_repo: Arc::new(uc_core::ports::NoopFileTransferRepositoryPort),
            },
            settings: Arc::new(NoopPort),
            system: uc_app::SystemPorts {
                clock: Arc::new(NoopPort),
                hash: Arc::new(NoopPort),
                file_manager: Arc::new(NoopPort),
                cache_fs: Arc::new(NoopPort),
            },
        };

        let runtime = AppRuntime::new(deps, test_storage_paths());
        let result = restore_clipboard_entry_impl(&runtime, entry_id.to_string(), None).await;

        let err = result.expect_err("touch_result=false should produce NotFound");
        assert!(
            matches!(err, CommandError::NotFound(_)),
            "expected NotFound, got: {err:?}"
        );
        let calls = calls.lock().unwrap().clone();
        assert_eq!(calls, vec!["touch"]);
    }

    #[tokio::test]
    async fn sync_clipboard_items_returns_error_in_passive_mode() {
        let _guard = crate::bootstrap::clipboard_integration_mode::clipboard_mode_env_lock()
            .lock()
            .expect("env lock");
        let _mode_guard = EnvVarGuard::set("UC_CLIPBOARD_MODE", "passive");

        let (worker_tx, _worker_rx) = mpsc::channel(1);
        let deps = AppDeps {
            clipboard: uc_app::ClipboardPorts {
                clipboard: Arc::new(NoopClipboard),
                system_clipboard: Arc::new(NoopClipboard),
                clipboard_entry_repo: Arc::new(MockEntryRepository {
                    entry: None,
                    touch_result: true,
                    calls: Arc::new(Mutex::new(Vec::new())),
                }),
                clipboard_event_repo: Arc::new(NoopPort),
                representation_repo: Arc::new(MockRepresentationRepository {
                    reps: HashMap::new(),
                }),
                representation_normalizer: Arc::new(NoopPort),
                selection_repo: Arc::new(MockSelectionRepository { selection: None }),
                representation_policy: Arc::new(NoopPort),
                representation_cache: Arc::new(NoopPort),
                spool_queue: Arc::new(NoopPort),
                clipboard_change_origin: Arc::new(InMemoryClipboardChangeOrigin::new()),
                worker_tx,
                payload_resolver: Arc::new(NoopPort),
            },
            security: uc_app::SecurityPorts {
                encryption: Arc::new(NoopPort),
                encryption_session: Arc::new(NoopPort),
                encryption_state: Arc::new(NoopPort),
                key_scope: Arc::new(NoopPort),
                secure_storage: Arc::new(NoopPort),
                key_material: Arc::new(NoopPort),
            },
            device: uc_app::DevicePorts {
                device_repo: Arc::new(NoopPort),
                device_identity: Arc::new(MockDeviceIdentity),
                paired_device_repo: Arc::new(NoopPort),
            },
            network_ports: noop_network_ports(),
            network_control: Arc::new(NoopPort),
            setup_status: Arc::new(NoopPort),
            storage: uc_app::StoragePorts {
                blob_store: Arc::new(NoopPort),
                blob_repository: Arc::new(NoopPort),
                blob_writer: Arc::new(NoopPort),
                thumbnail_repo: Arc::new(NoopPort),
                thumbnail_generator: Arc::new(NoopPort),
                file_transfer_repo: Arc::new(uc_core::ports::NoopFileTransferRepositoryPort),
            },
            settings: Arc::new(NoopPort),
            system: uc_app::SystemPorts {
                clock: Arc::new(NoopPort),
                hash: Arc::new(NoopPort),
                file_manager: Arc::new(NoopPort),
                cache_fs: Arc::new(NoopPort),
            },
        };

        let runtime = Arc::new(AppRuntime::new(deps, test_storage_paths()));
        let result = sync_clipboard_items_impl(runtime.as_ref(), None).await;

        let err = result.expect_err("passive mode should return error");
        assert!(
            matches!(err, CommandError::ValidationError(_)),
            "expected ValidationError, got: {err:?}"
        );
    }
}
