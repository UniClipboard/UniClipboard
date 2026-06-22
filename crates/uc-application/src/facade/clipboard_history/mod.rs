use std::path::PathBuf;
use std::sync::Arc;

use thiserror::Error;
use uc_core::blob::ports::BlobReaderPort;
use uc_core::clipboard::link_utils::extract_domain;
use uc_core::ids::{EntryId, EventId, FormatId, RepresentationId};
use uc_core::ports::blob::BlobTransferPort;
use uc_core::ports::clipboard::{
    ClipboardPayloadResolverPort, SaveClipboardEntryPort, ThumbnailRepositoryPort,
};
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::{
    CacheFsPort, ClipboardEventWriterPort, ClipboardSelectionRepositoryPort, ClockPort,
    DeviceIdentityPort, GetEntryTransferSummaryPort, SettingsPort,
};

use crate::deps::{ClipboardEntryPorts, ClipboardRepresentationPorts};
use uc_core::{
    ClipboardEntry, ClipboardEvent, ClipboardSelection, ClipboardSelectionDecision, MimeType,
    ObservedClipboardRepresentation, PayloadAvailability, PersistedClipboardRepresentation,
    SelectionPolicyVersion, SystemClipboardSnapshot,
};

use crate::usecases::clipboard_history::{
    compute_clipboard_stats, CleanupExpiredFilesUseCase, CleanupResult,
    ClearClipboardHistoryUseCase, DeleteClipboardEntryUseCase, EntryDetailResult,
    EntryProjectionDto, EntryResourceResult, GetEntryDetailUseCase, GetEntryResourceUseCase,
    ListClipboardEntryProjectionsUseCase, ListProjectionsError, ReconcileMissingFilesUseCase,
    ReconcileResult, ToggleFavoriteClipboardEntryUseCase,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardListInput {
    pub limit: usize,
    pub offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryProjectionView {
    pub id: String,
    pub preview: String,
    pub has_detail: bool,
    pub size_bytes: i64,
    pub captured_at: i64,
    pub content_type: String,
    pub thumbnail_url: Option<String>,
    pub is_encrypted: bool,
    pub is_favorited: bool,
    pub updated_at: i64,
    pub active_time: i64,
    pub file_transfer_status: Option<String>,
    pub file_transfer_reason: Option<String>,
    pub link_urls: Option<Vec<String>>,
    pub link_domains: Option<Vec<String>>,
    pub file_sizes: Option<Vec<i64>>,
    pub image_width: Option<i32>,
    pub image_height: Option<i32>,
    /// `paste_rep` 的 payload_state, 仅在 `Lost` 时输出。其他状态为 `None`。
    /// 前端按此判断"该 entry 点了能不能粘贴" —— 粘贴行为基于 paste_rep,
    /// 而 list 里的 preview 基于 preview_rep, 两者可能不同。
    pub payload_state: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryDetailView {
    pub id: String,
    pub content: String,
    pub size_bytes: i64,
    pub created_at_ms: i64,
    pub active_time_ms: i64,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryResourceView {
    pub blob_id: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub url: Option<String>,
    pub inline_data: Option<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClipboardStatsView {
    pub total_items: i64,
    pub total_size: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClearHistoryResultView {
    pub deleted_count: u64,
    pub failed_entries: Vec<(String, String)>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CleanupResultView {
    pub files_removed: u32,
    pub bytes_reclaimed: u64,
    pub entries_deleted: u32,
    pub orphans_removed: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReconcileResultView {
    pub entries_scanned: u32,
    pub entries_deleted: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, Error, PartialEq, Eq)]
pub enum ClipboardHistoryError {
    #[error("entry not found")]
    NotFound,
    #[error("unsupported clipboard content")]
    UnsupportedContent,
    #[error("clipboard history operation failed: {0}")]
    Internal(String),
}

/// Dependency bundle for `ClipboardHistoryFacade`.
///
/// Composition roots (daemon, tauri runtime, tests) construct this from their
/// wiring deps and pass it once to `ClipboardHistoryFacade::new`. The facade
/// then owns the use cases internally; no per-call gateway adapter is needed.
pub struct ClipboardHistoryFacadeDeps {
    pub entry_ports: ClipboardEntryPorts,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub representation_ports: ClipboardRepresentationPorts,
    pub event_writer: Arc<dyn ClipboardEventWriterPort>,
    pub payload_resolver: Arc<dyn ClipboardPayloadResolverPort>,
    pub blob_store: Arc<dyn BlobReaderPort>,
    pub thumbnail_repo: Arc<dyn ThumbnailRepositoryPort>,
    pub file_transfer_repo: Arc<dyn GetEntryTransferSummaryPort>,
    pub search_index: Option<Arc<dyn SearchIndexPort>>,
    pub file_cache_dir: Option<PathBuf>,
    /// 删除剪贴板条目时释放对应的 iroh-blobs tag。`None` 表示该装配场景
    /// 不接入 blob 系统（例如某些纯文本 / mock 测试场景），此时 untag
    /// 直接跳过；后台 GC 仍会按既定节奏处理脏 metadata（panic 防御不
    /// 依赖此 port 的存在，仅依赖 cache 文件与 GUI/sqlite 状态一致）。
    pub blob_transfer: Option<Arc<dyn BlobTransferPort>>,
    /// `cleanup_expired_files` 读取 `file_sync.file_auto_cleanup` 与
    /// `file_sync.file_retention_hours`。
    pub settings: Arc<dyn SettingsPort>,
    /// `seed_text_entry` 用：构造 `ClipboardEvent.source_device`。
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    /// `seed_text_entry` 用：构造 `captured_at_ms` / `created_at_ms`。
    pub clock: Arc<dyn ClockPort>,
    /// `reconcile_missing_files` / `cleanup_expired_files` 等用例需要查询
    /// cache 目录及其下路径是否真实存在；走 port 而不是 `std::fs`，让 uc-app
    /// 保持基础设施无关。
    pub cache_fs: Arc<dyn CacheFsPort>,
}

pub struct ClipboardHistoryFacade {
    list_uc: ListClipboardEntryProjectionsUseCase,
    detail_uc: GetEntryDetailUseCase,
    resource_uc: GetEntryResourceUseCase,
    toggle_favorite_uc: ToggleFavoriteClipboardEntryUseCase,
    delete_uc: DeleteClipboardEntryUseCase,
    clear_uc: ClearClipboardHistoryUseCase,
    cleanup_uc: Option<CleanupExpiredFilesUseCase>,
    reconcile_uc: Option<ReconcileMissingFilesUseCase>,
    /// debug seed 路径需要的额外 ports，常态业务不直接消费。
    seed_event_writer: Arc<dyn ClipboardEventWriterPort>,
    seed_entry_repo: Arc<dyn SaveClipboardEntryPort>,
    seed_device_identity: Arc<dyn DeviceIdentityPort>,
    seed_clock: Arc<dyn ClockPort>,
}

impl ClipboardHistoryFacade {
    pub fn new(deps: ClipboardHistoryFacadeDeps) -> Self {
        let ClipboardHistoryFacadeDeps {
            entry_ports,
            selection_repo,
            representation_ports,
            event_writer,
            payload_resolver,
            blob_store,
            thumbnail_repo,
            file_transfer_repo,
            search_index,
            file_cache_dir,
            blob_transfer,
            settings,
            device_identity,
            clock,
            cache_fs,
        } = deps;
        let ClipboardEntryPorts {
            get: entry_get,
            list: entry_list,
            save: entry_save,
            touch: _entry_touch,
            delete: entry_delete,
            find_by_snapshot_hash: _entry_find,
            get_snapshot_hash: _entry_snapshot_hash,
        } = entry_ports;
        let ClipboardRepresentationPorts {
            get: rep_get,
            get_by_blob_id: _rep_get_by_blob_id,
            list_for_event: rep_list_for_event,
            update_processing_result: _rep_update,
        } = representation_ports;
        // seed 路径要在主 use case 装配之后单独留一份 Arc 引用——其它字段
        // 都被 move 进各 use case 内部，不能在外面继续 clone。
        let seed_event_writer = Arc::clone(&event_writer);
        let seed_entry_repo = Arc::clone(&entry_save);
        let seed_device_identity = Arc::clone(&device_identity);
        let seed_clock = Arc::clone(&clock);
        // device_identity / clock 在常态 use case 里目前不消费，避免出现
        // 未使用 binding 警告——下面的 _ 让 clippy 闭嘴；后续 use case 真
        // 用上时再展开。
        let _ = device_identity;
        let _ = clock;

        let list_uc = ListClipboardEntryProjectionsUseCase::new(
            entry_list.clone(),
            selection_repo.clone(),
            rep_get.clone(),
            thumbnail_repo,
            file_transfer_repo,
        );

        let detail_uc = GetEntryDetailUseCase::new(
            entry_get.clone(),
            selection_repo.clone(),
            rep_get.clone(),
            blob_store,
            payload_resolver.clone(),
        );

        let resource_uc = GetEntryResourceUseCase::new(
            entry_get.clone(),
            selection_repo.clone(),
            rep_get.clone(),
            payload_resolver,
        );

        let toggle_favorite_uc = ToggleFavoriteClipboardEntryUseCase::new(entry_get.clone());

        let mut delete_uc = DeleteClipboardEntryUseCase::from_ports(
            entry_get.clone(),
            entry_delete.clone(),
            selection_repo.clone(),
            event_writer.clone(),
            rep_list_for_event.clone(),
        );
        if let Some(dir) = file_cache_dir.clone() {
            delete_uc = delete_uc.with_file_cache_dir(dir);
        }
        if let Some(idx) = search_index.clone() {
            delete_uc = delete_uc.with_search_index(idx);
        }
        if let Some(bt) = blob_transfer.clone() {
            delete_uc = delete_uc.with_blob_transfer(bt);
        }

        let mut clear_uc = ClearClipboardHistoryUseCase::from_ports(
            entry_list.clone(),
            entry_get.clone(),
            entry_delete.clone(),
            selection_repo.clone(),
            event_writer.clone(),
            rep_list_for_event.clone(),
        );
        if let Some(dir) = file_cache_dir.clone() {
            clear_uc = clear_uc.with_file_cache_dir(dir);
        }
        if let Some(idx) = search_index.clone() {
            clear_uc = clear_uc.with_search_index(idx);
        }
        if let Some(bt) = blob_transfer.clone() {
            clear_uc = clear_uc.with_blob_transfer(bt);
        }

        // cleanup 与 reconcile 都只在装配方传入了 file_cache_dir 时才有
        // 意义：没有 cache 目录就没有可扫描的文件 / 可消解的漂移条目。两者
        // 共享同一组底层 ports，所以这里把 Arc clone 一份给 cleanup，原始
        // ownership 留给 reconcile（后写入字段）。
        let cleanup_uc = file_cache_dir.clone().map(|dir| {
            // Clone cache_fs here since reconcile (built below) takes ownership
            // of the original. All of cleanup's on-disk work routes through it.
            let mut uc = CleanupExpiredFilesUseCase::new(
                settings,
                dir,
                entry_list.clone(),
                entry_get.clone(),
                entry_delete.clone(),
                selection_repo.clone(),
                event_writer.clone(),
                rep_list_for_event.clone(),
                cache_fs.clone(),
            );
            if let Some(idx) = search_index.clone() {
                uc = uc.with_search_index(idx);
            }
            if let Some(bt) = blob_transfer.clone() {
                uc = uc.with_blob_transfer(bt);
            }
            uc
        });

        let reconcile_uc = file_cache_dir.map(|dir| {
            let mut uc = ReconcileMissingFilesUseCase::new(
                dir,
                entry_list,
                entry_get,
                entry_delete,
                selection_repo,
                event_writer,
                rep_list_for_event,
                cache_fs,
            );
            if let Some(idx) = search_index {
                uc = uc.with_search_index(idx);
            }
            if let Some(bt) = blob_transfer {
                uc = uc.with_blob_transfer(bt);
            }
            uc
        });

        Self {
            list_uc,
            detail_uc,
            resource_uc,
            toggle_favorite_uc,
            delete_uc,
            clear_uc,
            cleanup_uc,
            reconcile_uc,
            seed_event_writer,
            seed_entry_repo,
            seed_device_identity,
            seed_clock,
        }
    }

    /// 调试 / 测试用：直接落库一条文本剪贴板条目。
    ///
    /// 走 `event_writer` decorator 链——所以 `inline_data` 会被
    /// `EncryptingClipboardEventWriter` 用当前 session master_key 加密。
    /// 之后 `entry_repo.save_entry_and_selection` 写入 `clipboard_entry`
    /// 行，让 `list_entries` / `get_entry` 能看到这条记录。
    ///
    /// 这是 switch-space E2E 数据完整性验证的种子方法（CLI
    /// `uniclip dev seed-clipboard` 命令的后端）；常态业务路径走
    /// `CaptureClipboardUseCase`，不要和这个方法混用。
    pub async fn seed_text_entry(&self, text: &str) -> Result<String, ClipboardHistoryError> {
        let event_id = EventId::new();
        let entry_id = EntryId::new();
        let rep_id = RepresentationId::new();
        let now = self.seed_clock.now_ms();
        let device_id = self.seed_device_identity.current_device_id();
        let bytes = text.as_bytes().to_vec();
        let total_size = bytes.len() as i64;

        // 用 SystemClipboardSnapshot 算 snapshot_hash——和 CaptureClipboardUseCase
        // 走同一份 hash 算法，与生产端 inbound dedup 路径一致。
        let snapshot = SystemClipboardSnapshot {
            ts_ms: now,
            representations: vec![ObservedClipboardRepresentation::new(
                rep_id.clone(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                bytes.clone(),
            )],
            file_content_digests: Vec::new(),
        };
        let snapshot_hash = snapshot.snapshot_hash();

        let event = ClipboardEvent::new(event_id.clone(), now, device_id, snapshot_hash);

        // PersistedClipboardRepresentation::new_with_state 把 inline_data
        // 标记为 Inline（可立即读出），与"text 默认 inline"语义一致。
        let rep = PersistedClipboardRepresentation::new_with_state(
            rep_id.clone(),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            total_size,
            Some(bytes),
            None,
            PayloadAvailability::Inline,
            None,
        )
        .map_err(|e| ClipboardHistoryError::Internal(format!("build representation: {e}")))?;

        self.seed_event_writer
            .insert_event(&event, &vec![rep])
            .await
            .map_err(|e| ClipboardHistoryError::Internal(format!("insert_event: {e}")))?;

        // ClipboardEntry 与 ClipboardSelection 是搭配的——entry_repo 的
        // save_entry_and_selection 会同时写两张表。selection 里只有一个
        // representation，全部字段都指向 rep_id。
        let entry = ClipboardEntry::new(entry_id.clone(), event_id, now, None, total_size);
        let selection = ClipboardSelectionDecision::new(
            entry_id.clone(),
            ClipboardSelection {
                primary_rep_id: rep_id.clone(),
                secondary_rep_ids: Vec::new(),
                preview_rep_id: rep_id.clone(),
                paste_rep_id: rep_id,
                policy_version: SelectionPolicyVersion::V1,
            },
        );

        self.seed_entry_repo
            .save_entry_and_selection(&entry, &selection)
            .await
            .map_err(|e| {
                ClipboardHistoryError::Internal(format!("save_entry_and_selection: {e}"))
            })?;

        Ok(entry_id.to_string())
    }

    pub async fn list_entries(
        &self,
        input: ClipboardListInput,
    ) -> Result<Vec<EntryProjectionView>, ClipboardHistoryError> {
        let entries = self
            .list_uc
            .execute(input.limit, input.offset)
            .await
            .map_err(map_list_error)?;
        Ok(entries.into_iter().map(projection_to_view).collect())
    }

    pub async fn get_entry(
        &self,
        entry_id: &str,
    ) -> Result<EntryDetailView, ClipboardHistoryError> {
        let parsed_id = EntryId::from(entry_id);
        let detail = self
            .detail_uc
            .execute(&parsed_id)
            .await
            .map_err(map_history_error)?;
        Ok(detail_to_view(detail))
    }

    pub async fn delete_entry(&self, entry_id: &str) -> Result<(), ClipboardHistoryError> {
        let parsed_id = EntryId::from(entry_id);
        self.delete_uc
            .execute(&parsed_id)
            .await
            .map_err(map_history_error)
    }

    pub async fn toggle_favorite(
        &self,
        entry_id: &str,
        is_favorited: bool,
    ) -> Result<bool, ClipboardHistoryError> {
        let parsed_id = EntryId::from(entry_id);
        self.toggle_favorite_uc
            .execute(&parsed_id, is_favorited)
            .await
            .map_err(|err| ClipboardHistoryError::Internal(err.to_string()))
    }

    pub async fn stats(&self) -> Result<ClipboardStatsView, ClipboardHistoryError> {
        let entries = self
            .list_uc
            .execute(10_000, 0)
            .await
            .map_err(map_list_error)?;
        let stats = compute_clipboard_stats(&entries);
        Ok(ClipboardStatsView {
            total_items: stats.total_items,
            total_size: stats.total_size,
        })
    }

    pub async fn get_entry_resource(
        &self,
        entry_id: &str,
    ) -> Result<EntryResourceView, ClipboardHistoryError> {
        let parsed_id = EntryId::from(entry_id);
        let resource = self
            .resource_uc
            .execute(&parsed_id)
            .await
            .map_err(map_history_error)?;
        Ok(resource_to_view(resource))
    }

    /// Run a single sweep of the file-cache directory: every expired file
    /// is routed through `delete_entry` (so iroh-blobs tags + sqlite rows
    /// + cache files are cleaned together), or removed as an orphan if no
    /// entry claims it.
    ///
    /// Returns `Ok(default())` when the facade was assembled without a
    /// `file_cache_dir` (typical for headless test contexts) — there is no
    /// cache to clean.
    pub async fn cleanup_expired_files(&self) -> Result<CleanupResultView, ClipboardHistoryError> {
        let Some(uc) = self.cleanup_uc.as_ref() else {
            return Ok(CleanupResultView::default());
        };
        let result = uc
            .execute()
            .await
            .map_err(|e| ClipboardHistoryError::Internal(e.to_string()))?;
        Ok(cleanup_to_view(result))
    }

    /// Drop every DB entry whose cache-managed `file://` path no longer
    /// exists on disk. Companion to [`Self::cleanup_expired_files`] —
    /// cleanup walks the cache dir, reconcile walks the entry list, so
    /// together they close both directions of cache↔DB drift. Should be
    /// invoked once on startup before any code path observes a hash.
    ///
    /// Returns `Ok(default())` when the facade was assembled without a
    /// `file_cache_dir` (headless / test contexts have nothing to drift).
    pub async fn reconcile_missing_files(
        &self,
    ) -> Result<ReconcileResultView, ClipboardHistoryError> {
        let Some(uc) = self.reconcile_uc.as_ref() else {
            return Ok(ReconcileResultView::default());
        };
        let result = uc
            .execute()
            .await
            .map_err(|e| ClipboardHistoryError::Internal(e.to_string()))?;
        Ok(reconcile_to_view(result))
    }

    pub async fn clear_history(&self) -> Result<ClearHistoryResultView, ClipboardHistoryError> {
        let result = self
            .clear_uc
            .execute()
            .await
            .map_err(|err| ClipboardHistoryError::Internal(err.to_string()))?;
        Ok(ClearHistoryResultView {
            deleted_count: result.deleted_count,
            failed_entries: result.failed_entries,
        })
    }
}

fn projection_to_view(entry: EntryProjectionDto) -> EntryProjectionView {
    let link_domains = entry
        .link_urls
        .as_ref()
        .map(|urls| urls.iter().filter_map(|url| extract_domain(url)).collect());
    EntryProjectionView {
        id: entry.id,
        preview: entry.preview,
        has_detail: entry.has_detail,
        size_bytes: entry.size_bytes,
        captured_at: entry.captured_at,
        content_type: entry.content_type,
        thumbnail_url: entry.thumbnail_url,
        is_encrypted: entry.is_encrypted,
        is_favorited: entry.is_favorited,
        updated_at: entry.updated_at,
        active_time: entry.active_time,
        file_transfer_status: entry.file_transfer_status,
        file_transfer_reason: entry.file_transfer_reason,
        link_urls: entry.link_urls,
        link_domains,
        file_sizes: entry.file_sizes,
        image_width: entry.image_width,
        image_height: entry.image_height,
        payload_state: entry.payload_state,
    }
}

fn detail_to_view(detail: EntryDetailResult) -> EntryDetailView {
    EntryDetailView {
        id: detail.id,
        content: detail.content,
        size_bytes: detail.size_bytes,
        created_at_ms: detail.created_at_ms,
        active_time_ms: detail.active_time_ms,
        mime_type: detail.mime_type,
    }
}

fn resource_to_view(resource: EntryResourceResult) -> EntryResourceView {
    EntryResourceView {
        blob_id: resource.blob_id.map(|id| id.to_string()),
        mime_type: resource.mime_type,
        size_bytes: resource.size_bytes,
        url: resource.url,
        inline_data: resource.inline_data,
    }
}

fn cleanup_to_view(result: CleanupResult) -> CleanupResultView {
    CleanupResultView {
        files_removed: result.files_removed,
        bytes_reclaimed: result.bytes_reclaimed,
        entries_deleted: result.entries_deleted,
        orphans_removed: result.orphans_removed,
        errors: result.errors,
    }
}

fn reconcile_to_view(result: ReconcileResult) -> ReconcileResultView {
    ReconcileResultView {
        entries_scanned: result.entries_scanned,
        entries_deleted: result.entries_deleted,
        errors: result.errors,
    }
}

fn map_history_error(err: anyhow::Error) -> ClipboardHistoryError {
    let message = err.to_string();
    let lower = message.to_lowercase();
    if lower.contains("not found") {
        ClipboardHistoryError::NotFound
    } else if lower.contains("not text content") || lower.contains("not text") {
        ClipboardHistoryError::UnsupportedContent
    } else {
        ClipboardHistoryError::Internal(message)
    }
}

fn map_list_error(err: ListProjectionsError) -> ClipboardHistoryError {
    ClipboardHistoryError::Internal(err.to_string())
}

// --- FileCacheHygienePort implementation ---

use uc_core::ports::file_cache_hygiene::{
    CleanupResult as CoreCleanupResult, FileCacheHygieneError, FileCacheHygienePort,
    ReconcileResult as CoreReconcileResult,
};

#[async_trait::async_trait]
impl FileCacheHygienePort for ClipboardHistoryFacade {
    async fn reconcile_missing_files(&self) -> Result<CoreReconcileResult, FileCacheHygieneError> {
        self.reconcile_missing_files()
            .await
            .map(|v| CoreReconcileResult {
                entries_scanned: v.entries_scanned,
                entries_deleted: v.entries_deleted,
                errors: v.errors,
            })
            .map_err(|e| FileCacheHygieneError(e.to_string()))
    }

    async fn cleanup_expired_files(&self) -> Result<CoreCleanupResult, FileCacheHygieneError> {
        self.cleanup_expired_files()
            .await
            .map(|v| CoreCleanupResult {
                files_removed: v.files_removed,
                bytes_reclaimed: v.bytes_reclaimed,
                entries_deleted: v.entries_deleted,
                orphans_removed: v.orphans_removed,
                errors: v.errors,
            })
            .map_err(|e| FileCacheHygieneError(e.to_string()))
    }
}
