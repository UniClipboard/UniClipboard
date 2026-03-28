use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use futures::future::try_join_all;
use tracing::{debug, info, info_span, warn, Instrument};
use uc_observability::stages;

use uc_core::ids::{EntryId, EventId};
use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort, SpoolRequest};
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationNormalizerPort,
    DeviceIdentityPort, SelectRepresentationPolicyPort,
};
use uc_core::{
    ClipboardChangeOrigin, ClipboardEntry, ClipboardEvent, ClipboardSelectionDecision,
    PayloadAvailability, SystemClipboardSnapshot,
};

/// Capture clipboard content and create persistent entries.
///
/// 捕获剪贴板内容并创建持久化条目。
///
/// # Behavior / 行为
/// - 1. Use the provided snapshot from the platform layer (事实)
/// - 2. Generate ClipboardEvent with timestamp (时间点)
/// - 3. Normalize snapshot representations (类型转换)
/// - 4. Apply representation selection policy (策略决策)
/// - 5. Create ClipboardEntry for user consumption (用户可见结果)
///
/// - 1. 使用平台层提供的快照（事实）
/// - 2. 生成带时间戳的剪贴板事件（时间点）
/// - 3. 规范化快照表示形式（类型转换）
/// - 4. 应用表示形式选择策略（策略决策）
/// - 5. 为用户消费创建剪贴板条目（用户可见结果）
///
/// # Architecture / 架构
///
/// This use case uses **trait objects** (`Arc<dyn Port>`) instead of generic type parameters.
/// This is the recommended pattern for use cases in the uc-app layer.
///
/// 此用例使用 **trait 对象** (`Arc<dyn Port>`) 而不是泛型类型参数。
/// 这是 uc-app 层用例的推荐模式。
pub struct CaptureClipboardUseCase {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
    device_identity: Arc<dyn DeviceIdentityPort>,
    representation_cache: Arc<dyn RepresentationCachePort>,
    spool_queue: Arc<dyn SpoolQueuePort>,
}

impl CaptureClipboardUseCase {
    /// Create a new CaptureClipboardUseCase with all required dependencies.
    ///
    /// 创建包含所有必需依赖项的新 CaptureClipboardUseCase 实例。
    ///
    /// # Parameters / 参数
    /// - `entry_repo`: Clipboard entry persistence
    /// - `event_writer`: Event and representation storage
    /// - `representation_policy`: Selection strategy for optimal representation
    /// - `representation_normalizer`: Type conversion from platform to domain
    /// - `device_identity`: Current device identification
    /// - `representation_cache`: Cache for representation metadata
    /// - `spool_queue`: Queue for disk spool requests
    ///
    /// - `entry_repo`: 剪贴板条目持久化
    /// - `event_writer`: 事件和表示形式存储
    /// - `representation_policy`: 最佳表示形式的选择策略
    /// - `representation_normalizer`: 从平台到域的类型转换
    /// - `device_identity`: 当前设备标识
    /// - `representation_cache`: 表示形式元数据缓存
    /// - `spool_queue`: 磁盘假脱机请求队列
    pub fn new(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
        representation_normalizer: Arc<dyn ClipboardRepresentationNormalizerPort>,
        device_identity: Arc<dyn DeviceIdentityPort>,
        representation_cache: Arc<dyn RepresentationCachePort>,
        spool_queue: Arc<dyn SpoolQueuePort>,
    ) -> Self {
        Self {
            entry_repo,
            event_writer,
            representation_policy,
            representation_normalizer,
            device_identity,
            representation_cache,
            spool_queue,
        }
    }

    /// Execute the clipboard capture workflow with a pre-captured snapshot.
    ///
    /// 执行剪贴板捕获工作流，使用预先捕获的快照。
    ///
    /// # Behavior / 行为
    /// - Uses the provided snapshot instead of reading from platform clipboard
    /// - Creates event and materializes all representations
    /// - Applies selection policy to determine optimal representation
    /// - Persists both event evidence and user-facing entry
    ///
    /// - 使用提供的快照而不是从平台剪贴板读取
    /// - 创建事件并物化所有表示形式
    /// - 应用选择策略确定最佳表示形式
    /// - 持久化事件证据和用户可见条目
    ///
    /// # Parameters / 参数
    /// - `snapshot`: Pre-captured clipboard snapshot from platform layer
    ///               来自平台层的预捕获剪贴板快照
    ///
    /// # Returns / 返回值
    /// - Persisted clipboard `EntryId`
    /// - 持久化剪贴板条目的 `EntryId`
    ///
    /// # When to Use / 使用时机
    /// - Called from clipboard change callback (snapshot already read)
    /// - 从剪贴板变化回调调用时（快照已读取）
    /// - Avoids redundant system clipboard reads
    /// - 避免重复读取系统剪贴板
    pub async fn execute(&self, snapshot: SystemClipboardSnapshot) -> Result<EntryId> {
        self.execute_with_origin(snapshot, ClipboardChangeOrigin::LocalCapture)
            .await?
            .ok_or_else(|| anyhow::anyhow!("local capture should always persist an entry"))
    }

    pub async fn execute_with_origin(
        &self,
        snapshot: SystemClipboardSnapshot,
        origin: ClipboardChangeOrigin,
    ) -> Result<Option<EntryId>> {
        let span = info_span!(
            "usecase.capture_clipboard.execute",
            source = "callback",
            origin = ?origin,
            representations = snapshot.representations.len(),
        );
        async move {
            if origin == ClipboardChangeOrigin::LocalRestore {
                info!(origin = ?origin, "Skipping clipboard capture");
                return Ok(None);
            }
            if !Self::has_supported_representation(&snapshot) {
                info!(
                    origin = ?origin,
                    representation_count = snapshot.representations.len(),
                    "Skipping clipboard capture because snapshot has no supported representations"
                );
                return Ok(None);
            }
            info!("Starting clipboard capture with provided snapshot");

            let event_id = EventId::new();
            let captured_at_ms = snapshot.ts_ms;
            let source_device = self.device_identity.current_device_id();
            let snapshot_hash = snapshot.snapshot_hash();

            // 1. 生成 event + snapshot representations
            let new_event = ClipboardEvent::new(
                event_id.clone(),
                captured_at_ms,
                source_device,
                snapshot_hash,
            );

            // 3. Normalize representations
            let normalized_reps = async {
                let normalized_futures: Vec<_> = snapshot
                    .representations
                    .iter()
                    .map(|rep| self.representation_normalizer.normalize(rep))
                    .collect();
                try_join_all(normalized_futures).await
            }
            .instrument(info_span!("normalize", stage = stages::NORMALIZE))
            .await?;

            async {
                self.event_writer
                    .insert_event(&new_event, &normalized_reps)
                    .await
            }
            .instrument(info_span!("persist_event", stage = stages::PERSIST_EVENT))
            .await?;

            // Cache representations for immediate access by the background blob worker.
            // This must happen before persist_entry so the worker gets a cache hit
            // when it is notified (via try_send in spool_blobs below).
            async {
                for rep in &normalized_reps {
                    if rep.payload_state() == PayloadAvailability::Staged {
                        if let Some(observed) =
                            snapshot.representations.iter().find(|o| o.id == rep.id)
                        {
                            self.representation_cache
                                .put(&rep.id, observed.bytes.clone())
                                .await;
                        }
                    }
                }
                Ok::<(), anyhow::Error>(())
            }
            .instrument(info_span!(
                "cache_representations",
                stage = stages::CACHE_REPRESENTATIONS
            ))
            .await?;

            // 4. policy.select(snapshot)
            let (entry_id, new_selection) = {
                let _guard = info_span!("select_policy", stage = stages::SELECT_POLICY).entered();
                let entry_id = EntryId::new();
                let selection = self.representation_policy.select(&snapshot)?;
                let new_selection = ClipboardSelectionDecision::new(entry_id.clone(), selection);
                (entry_id, new_selection)
            };

            // 5. entry_repo.insert_entry
            //
            // Persist the entry BEFORE spool writes so the entry appears in the
            // dashboard immediately. Spool writes (below) can take many seconds for
            // large images (e.g., macOS TIFF representations of 30-100 MB), and must
            // not block the user-visible entry creation path.
            async {
                let created_at_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_err(|e| anyhow::anyhow!("Failed to get system time: {}", e))?
                    .as_millis() as i64;
                let total_size = snapshot.total_size_bytes();

                let new_entry = ClipboardEntry::new(
                    entry_id.clone(),
                    event_id.clone(),
                    created_at_ms,
                    Self::generate_title(&snapshot),
                    total_size,
                );
                self.entry_repo
                    .save_entry_and_selection(&new_entry, &new_selection)
                    .await
            }
            .instrument(info_span!("persist_entry", stage = stages::PERSIST_ENTRY))
            .await?;

            info!(event_id = %event_id, entry_id = %entry_id, "Clipboard capture completed");

            // Queue large representations for durable spool-to-disk in a background task.
            // The entry is already persisted and bytes are in the in-memory cache, so the
            // background blob worker will get a cache hit immediately. Spool writes only
            // provide durability (survive process exit) — they must not block the callback.
            let spool_queue = Arc::clone(&self.spool_queue);
            let spool_reps: Vec<_> = normalized_reps
                .iter()
                .filter(|rep| rep.payload_state() == PayloadAvailability::Staged)
                .filter_map(|rep| {
                    snapshot
                        .representations
                        .iter()
                        .find(|o| o.id == rep.id)
                        .map(|observed| SpoolRequest {
                            rep_id: rep.id.clone(),
                            bytes: observed.bytes.clone(),
                        })
                })
                .collect();

            if !spool_reps.is_empty() {
                tokio::spawn(
                    async move {
                        for req in spool_reps {
                            let rep_id = req.rep_id.clone();
                            if let Err(err) = spool_queue.enqueue(req).await {
                                warn!(
                                    representation_id = %rep_id,
                                    error = %err,
                                    "Failed to enqueue spool request; blob will be lost if process exits before worker runs"
                                );
                            }
                        }
                    }
                    .instrument(info_span!("spool_blobs", stage = stages::SPOOL_BLOBS)),
                );
            }

            Ok(Some(entry_id))
        }
        .instrument(span)
        .await
    }

    /// Generate a title from the clipboard snapshot for display.
    /// 从剪贴板快照生成用于显示的标题。
    ///
    /// Tries to extract text content from text/plain representations,
    /// falling back to a size-based description if no text is found.
    ///
    /// 尝试从 text/plain 表示中提取文本内容，
    /// 如果没有找到文本，则回退到基于大小的描述。
    fn generate_title(snapshot: &SystemClipboardSnapshot) -> Option<String> {
        const MAX_TITLE_LENGTH: usize = 200;

        // Try to find text/plain representation
        // 尝试找到 text/plain 表示
        for rep in &snapshot.representations {
            if let Some(mime) = &rep.mime {
                let mime_str = mime.as_str();
                // Check for text MIME types (text/plain, public.utf8-plain-text, etc.)
                // 检查文本 MIME 类型
                if mime_str.eq_ignore_ascii_case("text/plain")
                    || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
                    || mime_str.eq_ignore_ascii_case("text/plain;charset=utf-8")
                    || mime_str.starts_with("text/")
                {
                    // Try to convert bytes to UTF-8 string
                    // 尝试将字节转换为 UTF-8 字符串
                    if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            // Truncate if too long and add ellipsis
                            // 如果太长则截断并添加省略号
                            // Use char_indices() to find a safe character boundary
                            // 使用 char_indices() 找到安全的字符边界
                            let char_count = trimmed.chars().count();
                            if char_count > MAX_TITLE_LENGTH {
                                let truncate_at = trimmed
                                    .char_indices()
                                    .nth(MAX_TITLE_LENGTH)
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(trimmed.len());
                                let truncated = &trimmed[..truncate_at];
                                return Some(format!("{}...", truncated));
                            }
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }

        // Fallback: no text representation found
        // 回退：没有找到文本表示
        debug!("No text representation found in snapshot, title will be None");
        None
    }

    fn has_supported_representation(snapshot: &SystemClipboardSnapshot) -> bool {
        let result = snapshot
            .representations
            .iter()
            .any(Self::is_supported_representation);

        debug!(
            repr_count = snapshot.representations.len(),
            format_ids = ?snapshot
                .representations
                .iter()
                .map(|r| r.format_id.to_string())
                .collect::<Vec<_>>(),
            mimes = ?snapshot
                .representations
                .iter()
                .map(|r| r.mime.as_ref().map(|m| m.as_str().to_string()))
                .collect::<Vec<_>>(),
            result,
            "has_supported_representation evaluated",
        );

        result
    }

    fn is_supported_representation(rep: &uc_core::ObservedClipboardRepresentation) -> bool {
        if let Some(mime) = &rep.mime {
            let mime_str = mime.as_str();
            if mime_str.starts_with("text/")
                || mime_str.starts_with("image/")
                || mime_str.eq_ignore_ascii_case("public.utf8-plain-text")
                || mime_str.eq_ignore_ascii_case("file/uri-list")
                || mime_str.eq_ignore_ascii_case("text/uri-list")
            {
                return true;
            }
        }

        rep.format_id.eq_ignore_ascii_case("text")
            || rep.format_id.eq_ignore_ascii_case("rtf")
            || rep.format_id.eq_ignore_ascii_case("html")
            || rep.format_id.eq_ignore_ascii_case("files")
            || rep.format_id.eq_ignore_ascii_case("image")
            || rep.format_id.eq_ignore_ascii_case("public.utf8-plain-text")
            || rep.format_id.eq_ignore_ascii_case("public.text")
            || rep.format_id.eq_ignore_ascii_case("NSStringPboardType")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uc_core::clipboard::PolicyError;
    use uc_core::ids::EntryId;
    use uc_core::ports::clipboard::{RepresentationCachePort, SpoolQueuePort, SpoolRequest};
    use uc_core::ports::{
        ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
        ClipboardRepresentationNormalizerPort, DeviceIdentityPort, SelectRepresentationPolicyPort,
    };
    use uc_core::{ClipboardChangeOrigin, ClipboardSelectionDecision, DeviceId};

    struct MockEntryRepository {
        save_calls: Arc<AtomicUsize>,
    }

    struct MockEventWriter {
        insert_calls: Arc<AtomicUsize>,
    }

    struct MockRepresentationPolicy {
        select_calls: Arc<AtomicUsize>,
    }

    struct MockNormalizer {
        normalize_calls: Arc<AtomicUsize>,
    }

    struct MockDeviceIdentity;

    struct MockRepresentationCache {
        put_calls: Arc<AtomicUsize>,
    }

    struct MockSpoolQueue {
        enqueue_calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepository {
        async fn save_entry_and_selection(
            &self,
            _entry: &uc_core::ClipboardEntry,
            _selection: &ClipboardSelectionDecision,
        ) -> Result<()> {
            self.save_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn get_entry(&self, _entry_id: &EntryId) -> Result<Option<uc_core::ClipboardEntry>> {
            Ok(None)
        }

        async fn list_entries(
            &self,
            _limit: usize,
            _offset: usize,
        ) -> Result<Vec<uc_core::ClipboardEntry>> {
            Ok(vec![])
        }

        async fn delete_entry(&self, _entry_id: &EntryId) -> Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _event: &uc_core::ClipboardEvent,
            _representations: &Vec<uc_core::PersistedClipboardRepresentation>,
        ) -> Result<()> {
            self.insert_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn delete_event_and_representations(
            &self,
            _event_id: &uc_core::ids::EventId,
        ) -> Result<()> {
            Ok(())
        }
    }

    impl SelectRepresentationPolicyPort for MockRepresentationPolicy {
        fn select(
            &self,
            _snapshot: &SystemClipboardSnapshot,
        ) -> std::result::Result<uc_core::clipboard::ClipboardSelection, PolicyError> {
            self.select_calls.fetch_add(1, Ordering::SeqCst);
            Err(PolicyError::NoUsableRepresentation)
        }
    }

    #[async_trait]
    impl ClipboardRepresentationNormalizerPort for MockNormalizer {
        async fn normalize(
            &self,
            _observed: &uc_core::clipboard::ObservedClipboardRepresentation,
        ) -> Result<uc_core::PersistedClipboardRepresentation> {
            self.normalize_calls.fetch_add(1, Ordering::SeqCst);
            Err(anyhow::anyhow!("normalize should not be called"))
        }
    }

    impl DeviceIdentityPort for MockDeviceIdentity {
        fn current_device_id(&self) -> DeviceId {
            DeviceId::new("device-test")
        }
    }

    #[async_trait]
    impl RepresentationCachePort for MockRepresentationCache {
        async fn put(&self, _rep_id: &uc_core::ids::RepresentationId, _bytes: Vec<u8>) {
            self.put_calls.fetch_add(1, Ordering::SeqCst);
        }

        async fn get(&self, _rep_id: &uc_core::ids::RepresentationId) -> Option<Vec<u8>> {
            None
        }

        async fn mark_completed(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn mark_spooling(&self, _rep_id: &uc_core::ids::RepresentationId) {}

        async fn remove(&self, _rep_id: &uc_core::ids::RepresentationId) {}
    }

    #[async_trait]
    impl SpoolQueuePort for MockSpoolQueue {
        async fn enqueue(&self, _request: SpoolRequest) -> anyhow::Result<()> {
            self.enqueue_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    #[tokio::test]
    async fn capture_skips_local_restore() {
        let save_calls = Arc::new(AtomicUsize::new(0));
        let insert_calls = Arc::new(AtomicUsize::new(0));
        let select_calls = Arc::new(AtomicUsize::new(0));
        let normalize_calls = Arc::new(AtomicUsize::new(0));
        let cache_put_calls = Arc::new(AtomicUsize::new(0));
        let enqueue_calls = Arc::new(AtomicUsize::new(0));

        let use_case = CaptureClipboardUseCase::new(
            Arc::new(MockEntryRepository {
                save_calls: save_calls.clone(),
            }),
            Arc::new(MockEventWriter {
                insert_calls: insert_calls.clone(),
            }),
            Arc::new(MockRepresentationPolicy {
                select_calls: select_calls.clone(),
            }),
            Arc::new(MockNormalizer {
                normalize_calls: normalize_calls.clone(),
            }),
            Arc::new(MockDeviceIdentity),
            Arc::new(MockRepresentationCache {
                put_calls: cache_put_calls.clone(),
            }),
            Arc::new(MockSpoolQueue {
                enqueue_calls: enqueue_calls.clone(),
            }),
        );

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![],
        };

        let _ = use_case
            .execute_with_origin(snapshot, ClipboardChangeOrigin::LocalRestore)
            .await
            .expect("expected ok result");

        assert_eq!(save_calls.load(Ordering::SeqCst), 0);
        assert_eq!(insert_calls.load(Ordering::SeqCst), 0);
        assert_eq!(select_calls.load(Ordering::SeqCst), 0);
        assert_eq!(normalize_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cache_put_calls.load(Ordering::SeqCst), 0);
        assert_eq!(enqueue_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn capture_skips_unsupported_snapshot_without_persisting() {
        let save_calls = Arc::new(AtomicUsize::new(0));
        let insert_calls = Arc::new(AtomicUsize::new(0));
        let select_calls = Arc::new(AtomicUsize::new(0));
        let normalize_calls = Arc::new(AtomicUsize::new(0));
        let cache_put_calls = Arc::new(AtomicUsize::new(0));
        let enqueue_calls = Arc::new(AtomicUsize::new(0));

        let use_case = CaptureClipboardUseCase::new(
            Arc::new(MockEntryRepository {
                save_calls: save_calls.clone(),
            }),
            Arc::new(MockEventWriter {
                insert_calls: insert_calls.clone(),
            }),
            Arc::new(MockRepresentationPolicy {
                select_calls: select_calls.clone(),
            }),
            Arc::new(MockNormalizer {
                normalize_calls: normalize_calls.clone(),
            }),
            Arc::new(MockDeviceIdentity),
            Arc::new(MockRepresentationCache {
                put_calls: cache_put_calls.clone(),
            }),
            Arc::new(MockSpoolQueue {
                enqueue_calls: enqueue_calls.clone(),
            }),
        );

        let snapshot = SystemClipboardSnapshot {
            ts_ms: 0,
            representations: vec![uc_core::ObservedClipboardRepresentation::new(
                uc_core::ids::RepresentationId::new(),
                uc_core::ids::FormatId::from("UnknownPrivateFormat"),
                None,
                vec![1],
            )],
        };

        let result = use_case
            .execute_with_origin(snapshot, ClipboardChangeOrigin::LocalCapture)
            .await
            .expect("expected ok result");

        assert!(result.is_none());
        assert_eq!(save_calls.load(Ordering::SeqCst), 0);
        assert_eq!(insert_calls.load(Ordering::SeqCst), 0);
        assert_eq!(select_calls.load(Ordering::SeqCst), 0);
        assert_eq!(normalize_calls.load(Ordering::SeqCst), 0);
        assert_eq!(cache_put_calls.load(Ordering::SeqCst), 0);
        assert_eq!(enqueue_calls.load(Ordering::SeqCst), 0);
    }
}
