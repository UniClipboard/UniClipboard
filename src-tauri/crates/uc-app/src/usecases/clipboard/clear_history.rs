//! Use case for clearing all clipboard history.
//! 清除所有剪贴板历史的用例。

use anyhow::Result;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, SearchIndexPort,
};

/// Result of a bulk clipboard history clear operation.
/// 批量清除剪贴板历史操作的结果。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearHistoryResult {
    /// Number of entries successfully deleted.
    pub deleted_count: u64,
    /// Entries that failed to delete: (entry_id, error_message).
    pub failed_entries: Vec<(String, String)>,
}

/// Use case for clearing all clipboard history entries via paginated listing and per-entry deletion.
/// 通过分页列出和逐条删除来清除所有剪贴板历史条目的用例。
pub struct ClearClipboardHistory {
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    file_cache_dir: Option<PathBuf>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
}

const BATCH_SIZE: usize = 1000;

impl ClearClipboardHistory {
    /// Constructs a `ClearClipboardHistory` use case from the required port implementations.
    pub fn from_ports(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            event_writer,
            representation_repo,
            file_cache_dir: None,
            search_index: None,
        }
    }

    /// Sets the managed file-cache directory used during per-entry cleanup.
    pub fn with_file_cache_dir(mut self, dir: PathBuf) -> Self {
        self.file_cache_dir = Some(dir);
        self
    }

    /// Injects the search index so bulk clear also removes indexed rows.
    pub fn with_search_index(mut self, search_index: Arc<dyn SearchIndexPort>) -> Self {
        self.search_index = Some(search_index);
        self
    }

    /// Clears all clipboard history by paginating through all entries and deleting each one.
    ///
    /// Returns a `ClearHistoryResult` containing the number of successfully deleted entries
    /// and a list of entries that failed to delete. Returns an error only if the initial
    /// listing fails or if ALL deletions fail.
    #[tracing::instrument(name = "usecase.clear_clipboard_history.execute", skip(self))]
    pub async fn execute(&self) -> Result<ClearHistoryResult> {
        // 1. Collect all entry IDs via pagination
        let entries = self.collect_all_entries().await?;

        let total = entries.len() as u64;
        info!(
            total_entries = total,
            "Starting bulk clipboard history deletion"
        );

        if total == 0 {
            return Ok(ClearHistoryResult {
                deleted_count: 0,
                failed_entries: Vec::new(),
            });
        }

        // 2. Delete each entry, tracking successes and failures
        let mut deleted_count = 0u64;
        let mut failed_entries: Vec<(String, String)> = Vec::new();

        let mut delete_uc = super::super::DeleteClipboardEntry::from_ports(
            self.entry_repo.clone(),
            self.selection_repo.clone(),
            self.event_writer.clone(),
            self.representation_repo.clone(),
        );
        if let Some(file_cache_dir) = self.file_cache_dir.clone() {
            delete_uc = delete_uc.with_file_cache_dir(file_cache_dir);
        }
        if let Some(search_index) = self.search_index.clone() {
            delete_uc = delete_uc.with_search_index(search_index);
        }

        for entry in &entries {
            let entry_id_str = entry.entry_id.inner().to_string();
            match delete_uc.execute(&entry.entry_id).await {
                Ok(()) => deleted_count += 1,
                Err(e) => {
                    warn!(
                        entry_id = %entry.entry_id,
                        error = %e,
                        "Failed to delete entry during bulk clear"
                    );
                    failed_entries.push((entry_id_str, e.to_string()));
                }
            }
        }

        info!(
            deleted = deleted_count,
            failed = failed_entries.len(),
            total = total,
            "Clipboard history cleared"
        );

        // If ALL deletions failed, return an error
        if deleted_count == 0 && !failed_entries.is_empty() {
            return Err(anyhow::anyhow!(
                "All {} entries failed to delete",
                failed_entries.len()
            ));
        }

        Ok(ClearHistoryResult {
            deleted_count,
            failed_entries,
        })
    }

    /// Collects all clipboard entries by paginating through the repository.
    async fn collect_all_entries(&self) -> Result<Vec<uc_core::clipboard::ClipboardEntry>> {
        let mut entries = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self
                .entry_repo
                .list_entries(BATCH_SIZE, offset)
                .instrument(info_span!(
                    "list_entries_batch",
                    batch_size = BATCH_SIZE,
                    offset = offset
                ))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to list entries for bulk delete: {}", e))?;

            if batch.is_empty() {
                break;
            }

            let batch_len = batch.len();
            entries.extend(batch);
            offset += batch_len;

            if batch_len < BATCH_SIZE {
                break;
            }
        }

        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Mutex;
    use uc_core::clipboard::{ClipboardEntry, ClipboardEvent, PersistedClipboardRepresentation};
    use uc_core::ids::{EntryId, EventId, RepresentationId};
    use uc_core::ports::ProcessingUpdateOutcome;
    use uc_core::search::{
        RebuildProgress, SearchDocument, SearchError, SearchIndexMeta, SearchPosting, SearchQuery,
        SearchResultsPage,
    };

    struct MockEntryRepo {
        entries: Mutex<HashMap<String, ClipboardEntry>>,
    }

    impl MockEntryRepo {
        fn new(entries: Vec<ClipboardEntry>) -> Self {
            Self {
                entries: Mutex::new(
                    entries
                        .into_iter()
                        .map(|entry| (entry.entry_id.to_string(), entry))
                        .collect(),
                ),
            }
        }
    }

    #[async_trait]
    impl ClipboardEntryRepositoryPort for MockEntryRepo {
        async fn save_entry_and_selection(
            &self,
            _entry: &ClipboardEntry,
            _selection: &uc_core::ClipboardSelectionDecision,
        ) -> Result<()> {
            Ok(())
        }

        async fn get_entry(&self, entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
            Ok(self.entries.lock().await.get(entry_id.as_str()).cloned())
        }

        async fn list_entries(&self, _limit: usize, _offset: usize) -> Result<Vec<ClipboardEntry>> {
            Ok(self.entries.lock().await.values().cloned().collect())
        }

        async fn delete_entry(&self, entry_id: &EntryId) -> Result<()> {
            self.entries.lock().await.remove(entry_id.as_str());
            Ok(())
        }
    }

    struct MockSelectionRepo;

    #[async_trait]
    impl ClipboardSelectionRepositoryPort for MockSelectionRepo {
        async fn get_selection(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<uc_core::ClipboardSelectionDecision>> {
            Ok(None)
        }

        async fn delete_selection(&self, _entry_id: &EntryId) -> Result<()> {
            Ok(())
        }
    }

    struct MockEventWriter {
        deleted_events: AtomicUsize,
    }

    #[async_trait]
    impl ClipboardEventWriterPort for MockEventWriter {
        async fn insert_event(
            &self,
            _event: &ClipboardEvent,
            _representations: &Vec<PersistedClipboardRepresentation>,
        ) -> Result<()> {
            Ok(())
        }

        async fn delete_event_and_representations(&self, _event_id: &EventId) -> Result<()> {
            self.deleted_events.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    struct MockRepresentationRepo;

    #[async_trait]
    impl ClipboardRepresentationRepositoryPort for MockRepresentationRepo {
        async fn get_representation(
            &self,
            _event_id: &EventId,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_id(
            &self,
            _representation_id: &RepresentationId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn get_representation_by_blob_id(
            &self,
            _blob_id: &uc_core::BlobId,
        ) -> Result<Option<PersistedClipboardRepresentation>> {
            Ok(None)
        }

        async fn update_blob_id(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<()> {
            Ok(())
        }

        async fn update_blob_id_if_none(
            &self,
            _representation_id: &RepresentationId,
            _blob_id: &uc_core::BlobId,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn update_processing_result(
            &self,
            _rep_id: &RepresentationId,
            _expected_states: &[uc_core::clipboard::PayloadAvailability],
            _blob_id: Option<&uc_core::BlobId>,
            _new_state: uc_core::clipboard::PayloadAvailability,
            _last_error: Option<&str>,
        ) -> Result<ProcessingUpdateOutcome> {
            Ok(ProcessingUpdateOutcome::NotFound)
        }
    }

    struct MockSearchIndex {
        removed_entry_ids: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl SearchIndexPort for MockSearchIndex {
        async fn index_entry(
            &self,
            _document: SearchDocument,
            _postings: Vec<SearchPosting>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError> {
            self.removed_entry_ids
                .lock()
                .await
                .push(entry_id.to_string());
            Ok(())
        }

        async fn search(&self, _query: SearchQuery) -> Result<SearchResultsPage, SearchError> {
            Ok(SearchResultsPage {
                items: vec![],
                total: 0,
                has_more: false,
            })
        }

        async fn rebuild(
            &self,
            _entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
            _progress_tx: tokio::sync::mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            Ok(SearchIndexMeta {
                index_version: "search-v2".into(),
                search_blocked: false,
                last_rebuild_started_at_ms: None,
                last_rebuild_completed_at_ms: None,
            })
        }
    }

    #[tokio::test]
    async fn execute_removes_search_index_entries_during_bulk_clear() {
        let entry = ClipboardEntry::new(
            EntryId::from("bulk-clear-entry-1"),
            EventId::from("bulk-clear-event-1"),
            1_777_000_000_000,
            Some("bulk clear".into()),
            42,
        );
        let entry_repo = Arc::new(MockEntryRepo::new(vec![entry.clone()]));
        let event_writer = Arc::new(MockEventWriter {
            deleted_events: AtomicUsize::new(0),
        });
        let search_index = Arc::new(MockSearchIndex {
            removed_entry_ids: Mutex::new(Vec::new()),
        });

        let result = ClearClipboardHistory::from_ports(
            entry_repo.clone(),
            Arc::new(MockSelectionRepo),
            event_writer.clone(),
            Arc::new(MockRepresentationRepo),
        )
        .with_search_index(search_index.clone())
        .execute()
        .await
        .expect("bulk clear should succeed");

        assert_eq!(result.deleted_count, 1);
        assert!(result.failed_entries.is_empty());
        assert!(entry_repo
            .get_entry(&entry.entry_id)
            .await
            .expect("get_entry should succeed")
            .is_none());
        assert_eq!(
            *search_index.removed_entry_ids.lock().await,
            vec![entry.entry_id.to_string()]
        );
        assert_eq!(event_writer.deleted_events.load(Ordering::SeqCst), 1);
    }
}
