//! Use case for clearing all clipboard history.
//! 清除所有剪贴板历史的用例。

use anyhow::Result;
use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardSelectionRepositoryPort,
};

/// Result of a bulk clipboard history clear operation.
/// 批量清除剪贴板历史操作的结果。
#[derive(Debug, Clone)]
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
}

const BATCH_SIZE: usize = 1000;

impl ClearClipboardHistory {
    /// Constructs a `ClearClipboardHistory` use case from the required port implementations.
    pub fn from_ports(
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
    ) -> Self {
        Self {
            entry_repo,
            selection_repo,
            event_writer,
        }
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

        let delete_uc = super::super::DeleteClipboardEntry::from_ports(
            self.entry_repo.clone(),
            self.selection_repo.clone(),
            self.event_writer.clone(),
        );

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
