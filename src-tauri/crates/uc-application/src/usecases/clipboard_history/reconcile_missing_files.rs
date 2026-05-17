//! Startup reconciliation: drop DB entries whose backing cache files have
//! vanished.
//!
//! Companion to [`super::cleanup::CleanupExpiredFilesUseCase`]. Cleanup
//! walks the file-cache directory to find files past their retention TTL
//! and matches them back to entries. Reconcile walks the entry list and
//! drops any entry whose cache-managed `file://` paths no longer exist on
//! disk. The two passes together close both directions of cache↔DB drift.
//!
//! Why this exists: under iroh-blobs 0.100.0 a `Complete{External(path)}`
//! entry whose path has been removed externally triggers
//! `BaoFileStorage::open -> Err(_) -> Poisoned`, and a subsequent
//! `observe(hash)` panics on `bao_file.rs:410` with
//! "poisoned storage should not be used". Old uniclipboard releases ran a
//! raw `tokio::fs::remove_file` cleanup that left exactly this drift behind;
//! reconcile cleans up that historical mess on startup so the upstream
//! crate never sees a stale External path.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use tracing::{info, info_span, warn, Instrument};

use uc_core::ids::{EntryId, EventId};
use uc_core::ports::blob::BlobTransferPort;
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::{
    CacheFsPort, ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
    ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort,
};

use super::delete_entry::DeleteClipboardEntryUseCase;

/// Result of a reconcile pass.
#[derive(Debug, Default, Clone)]
pub struct ReconcileResult {
    /// Number of entries scanned across every batch.
    pub entries_scanned: u32,
    /// Number of entries dropped because at least one of their cache files
    /// was missing on disk.
    pub entries_deleted: u32,
    /// Number of entries that we *wanted* to drop but `delete_entry`
    /// failed on. Surfaced separately so callers can decide whether to
    /// alert vs. silently log.
    pub errors: u32,
}

const ENTRY_LIST_BATCH_SIZE: usize = 1000;

pub(crate) struct ReconcileMissingFilesUseCase {
    file_cache_dir: PathBuf,
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    cache_fs: Arc<dyn CacheFsPort>,
    blob_transfer: Option<Arc<dyn BlobTransferPort>>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
}

impl ReconcileMissingFilesUseCase {
    pub(crate) fn new(
        file_cache_dir: PathBuf,
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        cache_fs: Arc<dyn CacheFsPort>,
    ) -> Self {
        Self {
            file_cache_dir,
            entry_repo,
            selection_repo,
            event_writer,
            representation_repo,
            cache_fs,
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

    #[tracing::instrument(name = "usecase.reconcile_missing_files.execute", skip(self))]
    pub(crate) async fn execute(&self) -> Result<ReconcileResult> {
        if !self.cache_fs.exists(&self.file_cache_dir).await {
            info!(
                path = %self.file_cache_dir.display(),
                "File cache directory does not exist, nothing to reconcile"
            );
            return Ok(ReconcileResult::default());
        }

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

        let mut result = ReconcileResult::default();
        let mut offset = 0usize;
        let mut handled: HashSet<EntryId> = HashSet::new();
        let mut to_delete: Vec<EntryId> = Vec::new();

        // Two-phase: scan first, delete second. Deleting during the scan
        // shrinks the underlying table, which silently shifts later
        // offset-based pages and skips entries adjacent to deletions.
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
                .map_err(|e| anyhow::anyhow!("list entries for reconcile: {e}"))?;

            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();
            result.entries_scanned += batch_len as u32;

            for entry in &batch {
                if !handled.insert(entry.entry_id.clone()) {
                    continue;
                }
                let missing = match self.entry_has_missing_cache_file(&entry.event_id).await {
                    Ok(missing) => missing,
                    Err(e) => {
                        warn!(
                            entry_id = %entry.entry_id,
                            error = %e,
                            "Failed to inspect representations while reconciling — skipping entry"
                        );
                        continue;
                    }
                };
                if missing {
                    to_delete.push(entry.entry_id.clone());
                }
            }

            offset += batch_len;
            if batch_len < ENTRY_LIST_BATCH_SIZE {
                break;
            }
        }

        for entry_id in &to_delete {
            match delete_uc.execute(entry_id).await {
                Ok(()) => {
                    result.entries_deleted += 1;
                    info!(
                        entry_id = %entry_id,
                        "Reconcile: dropped entry whose cache file no longer exists"
                    );
                }
                Err(e) => {
                    warn!(
                        entry_id = %entry_id,
                        error = %e,
                        "Reconcile: delete_entry failed for stale entry"
                    );
                    result.errors += 1;
                }
            }
        }

        info!(
            entries_scanned = result.entries_scanned,
            entries_deleted = result.entries_deleted,
            errors = result.errors,
            "Reconcile pass complete"
        );
        Ok(result)
    }

    /// Return `Ok(true)` when any cache-managed `file://` URI in the
    /// entry's representations points at a path that doesn't exist on
    /// disk. Paths outside `file_cache_dir` are ignored (user-owned
    /// files we never managed).
    async fn entry_has_missing_cache_file(&self, event_id: &EventId) -> Result<bool> {
        let representations = self
            .representation_repo
            .get_representations_for_event(event_id)
            .await
            .map_err(|e| anyhow::anyhow!("get representations: {e}"))?;

        for rep in &representations {
            let mime = rep.mime_type.as_ref().map(|m| m.as_str()).unwrap_or("");
            if !mime.contains("uri-list") {
                continue;
            }
            let Some(inline) = rep.inline_data.as_ref() else {
                continue;
            };
            for path in extract_cache_paths(inline, &self.file_cache_dir) {
                if !self.cache_fs.exists(&path).await {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }
}

/// Decode a `text/uri-list` payload and return only those `file://` paths
/// that fall under `cache_dir`. Lines that fail to parse, comments, and
/// paths outside `cache_dir` are silently skipped — they are not the
/// reconcile target. This mirrors the extraction logic in
/// [`super::cleanup`] and [`super::delete_entry`], lifted into a pure
/// helper so it can be unit-tested without a full use-case fixture.
fn extract_cache_paths(uri_list: &[u8], cache_dir: &Path) -> Vec<PathBuf> {
    let text = String::from_utf8_lossy(uri_list);
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parsed = if line.starts_with("file://") {
            url::Url::parse(line)
                .ok()
                .and_then(|u| u.to_file_path().ok())
        } else {
            Some(PathBuf::from(line))
        };
        let Some(path) = parsed else { continue };
        if path.starts_with(cache_dir) {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache_dir() -> PathBuf {
        PathBuf::from("/var/cache/uniclipboard")
    }

    #[test]
    fn extracts_file_uri_inside_cache_dir() {
        let uri = b"file:///var/cache/uniclipboard/abc/IMG.JPG\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        assert_eq!(
            paths,
            vec![PathBuf::from("/var/cache/uniclipboard/abc/IMG.JPG")]
        );
    }

    #[test]
    fn skips_paths_outside_cache_dir() {
        // user dropped a file from their Downloads folder — we never
        // managed it, so it must not show up in the reconcile target set.
        let uri = b"file:///Users/alice/Downloads/IMG.JPG\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        assert!(paths.is_empty(), "user-owned path should be filtered out");
    }

    #[test]
    fn skips_blank_lines_and_comments() {
        let uri = b"# this is a comment\n\nfile:///var/cache/uniclipboard/a/b.txt\n\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        assert_eq!(
            paths,
            vec![PathBuf::from("/var/cache/uniclipboard/a/b.txt")]
        );
    }

    #[test]
    fn accepts_bare_path_lines() {
        // Some legacy representations carry plain paths instead of
        // file:// URIs. The reverse-index loop in the original cleanup
        // also handled this; keep parity.
        let uri = b"/var/cache/uniclipboard/abc/file.bin\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        assert_eq!(
            paths,
            vec![PathBuf::from("/var/cache/uniclipboard/abc/file.bin")]
        );
    }

    #[test]
    fn ignores_garbage_lines() {
        let uri = b"not a url\nfile://?garbage\nfile:///var/cache/uniclipboard/ok.txt\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        // The bare-path branch (`Some(PathBuf::from(line))`) means
        // unparseable lines that aren't `file://`-prefixed still become
        // a PathBuf and are filtered by the cache-dir prefix check.
        // Only the legitimate cache path should survive.
        assert_eq!(paths, vec![PathBuf::from("/var/cache/uniclipboard/ok.txt")]);
    }

    #[test]
    fn returns_each_path_in_multi_file_payload() {
        let uri =
            b"file:///var/cache/uniclipboard/a/1.txt\nfile:///var/cache/uniclipboard/b/2.txt\n";
        let paths = extract_cache_paths(uri, &cache_dir());
        assert_eq!(
            paths,
            vec![
                PathBuf::from("/var/cache/uniclipboard/a/1.txt"),
                PathBuf::from("/var/cache/uniclipboard/b/2.txt"),
            ]
        );
    }
}
