//! Entry-level cleanup of expired file-cache entries.
//!
//! Replaces the historical mtime-only `tokio::fs::remove_file` sweep that
//! ran in `file_sync::cleanup`. The old behaviour deleted cache files
//! without telling iroh-blobs, leaving `Complete{External([path], _)}`
//! metadata pointing at vanished files — the precondition for the
//! `Poisoned` panic at `bao_file.rs:410` once any code path tried to
//! re-open the blob.
//!
//! The new flow walks the cache dir, builds an in-memory
//! `path → entry_id` index from `text/uri-list` representations, and
//! routes each expired file through the entry-aware delete path
//! (`DeleteClipboardEntryUseCase`). Files with no owning entry are
//! orphans and are removed directly — they would otherwise sit in the
//! cache forever.
//!
//! The reverse index is built once per execution and lives only in
//! memory; we deliberately avoid introducing a `path → entry_id` SQLite
//! index because cleanup runs at most once per startup and the cost of
//! decrypting representations on the order of a few thousand entries is
//! fine. If cleanup frequency ever grows (e.g. per-hour sweep), revisit
//! this trade-off.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, info_span, warn, Instrument};

use uc_core::ids::EntryId;
use uc_core::ports::blob::BlobTransferPort;
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::{
    ClipboardEntryRepositoryPort, ClipboardEventWriterPort, ClipboardRepresentationRepositoryPort,
    ClipboardSelectionRepositoryPort, SettingsPort,
};

use super::delete_entry::DeleteClipboardEntryUseCase;

/// Result of a cleanup pass.
#[derive(Debug, Default, Clone)]
pub struct CleanupResult {
    /// Number of cache files reclaimed (entries deleted + orphans removed).
    pub files_removed: u32,
    /// Bytes reclaimed across all files removed.
    pub bytes_reclaimed: u64,
    /// Number of entries that were deleted via `delete_entry`.
    pub entries_deleted: u32,
    /// Number of orphan files removed without a matching entry.
    pub orphans_removed: u32,
    /// Number of failures (delete_entry failure or orphan remove_file failure).
    pub errors: u32,
}

const ENTRY_LIST_BATCH_SIZE: usize = 1000;

pub(crate) struct CleanupExpiredFilesUseCase {
    settings: Arc<dyn SettingsPort>,
    file_cache_dir: PathBuf,
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_transfer: Option<Arc<dyn BlobTransferPort>>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
}

impl CleanupExpiredFilesUseCase {
    pub(crate) fn new(
        settings: Arc<dyn SettingsPort>,
        file_cache_dir: PathBuf,
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    ) -> Self {
        Self {
            settings,
            file_cache_dir,
            entry_repo,
            selection_repo,
            event_writer,
            representation_repo,
            blob_transfer: None,
            search_index: None,
        }
    }

    pub(crate) fn with_blob_transfer(mut self, blob_transfer: Arc<dyn BlobTransferPort>) -> Self {
        self.blob_transfer = Some(blob_transfer);
        self
    }

    pub(crate) fn with_search_index(mut self, search_index: Arc<dyn SearchIndexPort>) -> Self {
        self.search_index = Some(search_index);
        self
    }

    #[tracing::instrument(name = "usecase.cleanup_expired_files.execute", skip(self))]
    pub(crate) async fn execute(&self) -> Result<CleanupResult> {
        let settings = self.settings.load().await?;

        if !settings.file_sync.file_auto_cleanup {
            info!("File auto-cleanup disabled, skipping");
            return Ok(CleanupResult::default());
        }

        let retention_secs = settings.file_sync.file_retention_hours as u64 * 3600;
        let now = std::time::SystemTime::now();

        if !self.file_cache_dir.exists() {
            info!(
                path = %self.file_cache_dir.display(),
                "File cache directory does not exist, nothing to clean"
            );
            return Ok(CleanupResult::default());
        }

        let expired_files = collect_expired_files(&self.file_cache_dir, now, retention_secs)?;
        if expired_files.is_empty() {
            info!("No expired cache files to clean up");
            return Ok(CleanupResult::default());
        }

        let path_to_entry = self.build_reverse_index().await?;
        info!(
            expired_files = expired_files.len(),
            indexed_paths = path_to_entry.len(),
            "Reverse index built; routing expired files to entry-level delete or orphan removal"
        );

        let mut delete_uc = DeleteClipboardEntryUseCase::from_ports(
            self.entry_repo.clone(),
            self.selection_repo.clone(),
            self.event_writer.clone(),
            self.representation_repo.clone(),
        )
        .with_file_cache_dir(self.file_cache_dir.clone());
        if let Some(idx) = self.search_index.clone() {
            delete_uc = delete_uc.with_search_index(idx);
        }
        if let Some(bt) = self.blob_transfer.clone() {
            delete_uc = delete_uc.with_blob_transfer(bt);
        }

        let mut result = CleanupResult::default();
        // Multiple cache paths can map to the same entry_id (an entry with
        // several files); only invoke delete_entry once per entry.
        let mut handled_entries: HashSet<EntryId> = HashSet::new();

        for (path, size) in &expired_files {
            match path_to_entry.get(path) {
                Some(entry_id) => {
                    if !handled_entries.insert(entry_id.clone()) {
                        // already deleted via a sibling expired file in this pass;
                        // delete_entry already removed every cache file the entry
                        // owned, so just account for the bytes we expected to free.
                        result.files_removed += 1;
                        result.bytes_reclaimed += size;
                        continue;
                    }
                    match delete_uc.execute(entry_id).await {
                        Ok(()) => {
                            result.entries_deleted += 1;
                            result.files_removed += 1;
                            result.bytes_reclaimed += size;
                        }
                        Err(e) => {
                            warn!(
                                entry_id = %entry_id,
                                path = %path.display(),
                                error = %e,
                                "delete_entry failed for expired cache file"
                            );
                            result.errors += 1;
                        }
                    }
                }
                None => match tokio::fs::remove_file(path).await {
                    Ok(()) => {
                        info!(
                            path = %path.display(),
                            "Removed orphan cache file (no owning entry in DB)"
                        );
                        result.orphans_removed += 1;
                        result.files_removed += 1;
                        result.bytes_reclaimed += size;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        result.orphans_removed += 1;
                        result.files_removed += 1;
                    }
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to remove orphan cache file"
                        );
                        result.errors += 1;
                    }
                },
            }
        }

        cleanup_empty_dirs(&self.file_cache_dir).await;

        info!(
            files_removed = result.files_removed,
            entries_deleted = result.entries_deleted,
            orphans_removed = result.orphans_removed,
            errors = result.errors,
            bytes_reclaimed_mb = result.bytes_reclaimed / (1024 * 1024),
            "File cache cleanup complete"
        );
        Ok(result)
    }

    /// Walk every entry in the DB and build a `cache_path → entry_id`
    /// index from `text/uri-list` representations. Plaintext URIs are
    /// returned by the decrypting representation port — callers do not
    /// need to think about encryption here.
    async fn build_reverse_index(&self) -> Result<HashMap<PathBuf, EntryId>> {
        let mut index: HashMap<PathBuf, EntryId> = HashMap::new();
        let mut offset = 0usize;

        loop {
            let batch = self
                .entry_repo
                .list_entries(ENTRY_LIST_BATCH_SIZE, offset)
                .instrument(info_span!(
                    "list_entries_batch",
                    batch_size = ENTRY_LIST_BATCH_SIZE,
                    offset = offset
                ))
                .await
                .map_err(|e| anyhow::anyhow!("list entries for cleanup index: {e}"))?;

            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();

            for entry in &batch {
                let representations = match self
                    .representation_repo
                    .get_representations_for_event(&entry.event_id)
                    .await
                {
                    Ok(reps) => reps,
                    Err(e) => {
                        warn!(
                            event_id = %entry.event_id,
                            error = %e,
                            "Failed to load representations while building reverse index — skipping entry"
                        );
                        continue;
                    }
                };

                for rep in &representations {
                    let mime = rep.mime_type.as_ref().map(|m| m.as_str()).unwrap_or("");
                    if !mime.contains("uri-list") {
                        continue;
                    }
                    let Some(inline) = rep.inline_data.as_ref() else {
                        continue;
                    };
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
                            Some(PathBuf::from(line))
                        };
                        let Some(path) = path else { continue };
                        if path.starts_with(&self.file_cache_dir) {
                            index.insert(path, entry.entry_id.clone());
                        }
                    }
                }
            }

            offset += batch_len;
            if batch_len < ENTRY_LIST_BATCH_SIZE {
                break;
            }
        }

        Ok(index)
    }
}

fn collect_expired_files(
    cache_dir: &Path,
    now: std::time::SystemTime,
    retention_secs: u64,
) -> Result<Vec<(PathBuf, u64)>> {
    let mut expired = Vec::new();
    collect_expired_recursive(cache_dir, now, retention_secs, &mut expired)?;
    Ok(expired)
}

fn collect_expired_recursive(
    dir: &Path,
    now: std::time::SystemTime,
    retention_secs: u64,
    out: &mut Vec<(PathBuf, u64)>,
) -> Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            warn!(
                path = %dir.display(),
                error = %e,
                "Failed to read cache directory"
            );
            return Ok(());
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to read directory entry");
                continue;
            }
        };

        let meta = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                warn!(
                    path = %entry.path().display(),
                    error = %e,
                    "Failed to read file metadata"
                );
                continue;
            }
        };

        if meta.is_dir() {
            collect_expired_recursive(&entry.path(), now, retention_secs, out)?;
        } else if meta.is_file() {
            let modified = meta.modified().unwrap_or(now);
            let age = now.duration_since(modified).unwrap_or_default();
            if age.as_secs() >= retention_secs {
                out.push((entry.path(), meta.len()));
            }
        }
    }

    Ok(())
}

async fn cleanup_empty_dirs(cache_dir: &Path) {
    let entries = match std::fs::read_dir(cache_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(mut contents) = std::fs::read_dir(&path) {
                if contents.next().is_none() {
                    if let Err(e) = tokio::fs::remove_dir(&path).await {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "Failed to remove empty cache directory"
                        );
                    }
                }
            }
        }
    }
}
