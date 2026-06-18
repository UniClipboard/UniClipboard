use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, info_span, warn, Instrument};
use uc_core::ids::EntryId;
use uc_core::ports::blob::{BlobTransferPort, TagReason};
use uc_core::ports::clipboard::{
    DeleteClipboardEntryPort, GetClipboardEntryPort, ListRepresentationsForEventPort,
};
use uc_core::ports::{ClipboardEventWriterPort, ClipboardSelectionRepositoryPort, SearchIndexPort};

/// Use case for deleting clipboard entries with all associated data.
pub(crate) struct DeleteClipboardEntryUseCase {
    get_entry: Arc<dyn GetClipboardEntryPort>,
    delete_entry: Arc<dyn DeleteClipboardEntryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    file_cache_dir: Option<PathBuf>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
    blob_transfer: Option<Arc<dyn BlobTransferPort>>,
}

impl DeleteClipboardEntryUseCase {
    pub(crate) fn from_ports(
        get_entry: Arc<dyn GetClipboardEntryPort>,
        delete_entry: Arc<dyn DeleteClipboardEntryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    ) -> Self {
        Self {
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

    /// Deletes a clipboard entry and its associated selection, event, and snapshot
    /// representations in the required order. For file entries (text/uri-list),
    /// also deletes the cache files from disk when they live inside the managed
    /// file-cache directory.
    #[tracing::instrument(
        name = "usecase.delete_clipboard_entry.execute",
        skip(self),
        fields(entry_id = %entry_id)
    )]
    pub(crate) async fn execute(&self, entry_id: &EntryId) -> Result<()> {
        let entry = async {
            self.get_entry
                .get_entry(entry_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("Clipboard entry not found: {}", entry_id))
        }
        .instrument(info_span!(
            "fetch_entry",
            entry_id = %entry_id
        ))
        .await?;
        let event_id = entry.event_id.clone();

        if let Some(blob_transfer) = self.blob_transfer.as_ref() {
            async {
                if let Err(e) = blob_transfer
                    .untag(TagReason::ClipboardEntry(entry_id.clone()))
                    .await
                {
                    warn!(
                        entry_id = %entry_id,
                        error = %e,
                        "blob untag failed during entry delete; iroh-blobs GC will reclaim metadata on its next sweep"
                    );
                }
            }
            .instrument(info_span!("release_blob_tag", entry_id = %entry_id))
            .await;
        }

        async {
            let Some(ref cache_dir) = self.file_cache_dir else {
                return;
            };

            if let Ok(representations) = self
                .representation_repo
                .get_representations_for_event(&event_id)
                .await
            {
                for rep in &representations {
                    let mime = rep.mime_type.as_ref().map(|m| m.as_str()).unwrap_or("");
                    if mime.contains("uri-list") {
                        if let Some(ref inline) = rep.inline_data {
                            let uri_text = String::from_utf8_lossy(inline);
                            for line in uri_text.lines() {
                                let line = line.trim();
                                if line.is_empty() || line.starts_with('#') {
                                    continue;
                                }
                                let path = if line.starts_with("file://") {
                                    url::Url::parse(line)
                                        .ok()
                                        .and_then(|u| u.to_file_path().ok())
                                } else {
                                    Some(std::path::PathBuf::from(line))
                                };

                                let Some(path) = path else {
                                    continue;
                                };

                                if !path.starts_with(cache_dir) {
                                    info!(
                                        path = %path.display(),
                                        cache_dir = %cache_dir.display(),
                                        "Skipping file deletion — path is outside the managed file-cache directory (user-owned file)"
                                    );
                                    continue;
                                }

                                if let Err(e) = tokio::fs::remove_file(&path).await {
                                    warn!(
                                        path = %path.display(),
                                        error = %e,
                                        "Failed to delete cache file during entry cleanup"
                                    );
                                } else {
                                    info!(
                                        path = %path.display(),
                                        "Deleted cache file during entry cleanup"
                                    );
                                    if let Some(parent) = path.parent() {
                                        if parent != cache_dir.as_path()
                                            && parent.starts_with(cache_dir)
                                        {
                                            let _ = tokio::fs::remove_dir(parent).await;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        .instrument(info_span!("cleanup_cache_files", event_id = %event_id))
        .await;

        if let Some(search_index) = self.search_index.as_ref() {
            async {
                if let Err(e) = search_index.remove_entry(entry_id).await {
                    warn!(
                        error = %e,
                        entry_id = %entry_id,
                        "search index cleanup failed, continuing delete"
                    );
                }
            }
            .instrument(info_span!("cleanup_search_index", entry_id = %entry_id))
            .await;
        }

        self.selection_repo
            .delete_selection(entry_id)
            .instrument(info_span!(
                "delete_selection",
                entry_id = %entry_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete selection: {}", e))?;

        self.delete_entry
            .delete_entry(entry_id)
            .instrument(info_span!(
                "delete_entry",
                entry_id = %entry_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete entry: {}", e))?;

        self.event_writer
            .delete_event_and_representations(&event_id)
            .instrument(info_span!(
                "delete_event",
                event_id = %event_id
            ))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to delete event: {}", e))?;

        info!(
            entry_id = %entry_id,
            event_id = %event_id,
            "Deleted clipboard entry successfully"
        );
        Ok(())
    }
}
