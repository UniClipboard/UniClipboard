use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ports::blob::BlobTransferPort;
use uc_core::ports::clipboard::{
    DeleteClipboardEntryPort, GetClipboardEntryPort, ListClipboardEntriesPort,
    ListRepresentationsForEventPort,
};
use uc_core::ports::{ClipboardEventWriterPort, ClipboardSelectionRepositoryPort, SearchIndexPort};

use super::delete_entry::DeleteClipboardEntryUseCase;

#[derive(Debug, Clone)]
pub(crate) struct ClearHistoryResult {
    pub(crate) deleted_count: u64,
    pub(crate) failed_entries: Vec<(String, String)>,
}

/// Use case for clearing all clipboard history entries via paginated listing
/// and per-entry deletion.
pub(crate) struct ClearClipboardHistoryUseCase {
    list_entries: Arc<dyn ListClipboardEntriesPort>,
    get_entry: Arc<dyn GetClipboardEntryPort>,
    delete_entry: Arc<dyn DeleteClipboardEntryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    file_cache_dir: Option<PathBuf>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
    blob_transfer: Option<Arc<dyn BlobTransferPort>>,
}

const BATCH_SIZE: usize = 1000;

impl ClearClipboardHistoryUseCase {
    pub(crate) fn from_ports(
        list_entries: Arc<dyn ListClipboardEntriesPort>,
        get_entry: Arc<dyn GetClipboardEntryPort>,
        delete_entry: Arc<dyn DeleteClipboardEntryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    ) -> Self {
        Self {
            list_entries,
            get_entry,
            delete_entry,
            selection_repo,
            event_writer,
            representation_repo,
            file_cache_dir: None,
            search_index: None,
            blob_transfer: None,
        }
    }

    pub(crate) fn with_file_cache_dir(mut self, dir: PathBuf) -> Self {
        self.file_cache_dir = Some(dir);
        self
    }

    pub(crate) fn with_search_index(mut self, search_index: Arc<dyn SearchIndexPort>) -> Self {
        self.search_index = Some(search_index);
        self
    }

    pub(crate) fn with_blob_transfer(mut self, blob_transfer: Arc<dyn BlobTransferPort>) -> Self {
        self.blob_transfer = Some(blob_transfer);
        self
    }

    #[tracing::instrument(name = "usecase.clear_clipboard_history.execute", skip(self))]
    pub(crate) async fn execute(&self) -> Result<ClearHistoryResult> {
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

        let mut deleted_count = 0u64;
        let mut failed_entries: Vec<(String, String)> = Vec::new();

        let mut delete_uc = DeleteClipboardEntryUseCase::from_ports(
            self.get_entry.clone(),
            self.delete_entry.clone(),
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
        if let Some(blob_transfer) = self.blob_transfer.clone() {
            delete_uc = delete_uc.with_blob_transfer(blob_transfer);
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

    async fn collect_all_entries(&self) -> Result<Vec<uc_core::clipboard::ClipboardEntry>> {
        let mut entries = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self
                .list_entries
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
