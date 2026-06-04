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

use uc_core::clipboard::PayloadAvailability;
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

        let retention_hours = settings.file_sync.file_retention_hours;
        // `file_cache_quota_per_device` is enforced here as a *total* on-disk
        // budget for cached payloads. The per-device layout it was named for
        // never materialized: clipboard image blobs land in a single
        // content-addressed iroh-blobs store that is not partitioned by source
        // device, so a total cap is both the only practical interpretation and
        // strictly more protective than the unenforced original.
        let quota_bytes = settings.file_sync.file_cache_quota_per_device;

        let mut result = CleanupResult::default();

        // One entry-aware delete path, shared by both passes. For blob-backed
        // entries this untags the blob; iroh-blobs GC reclaims the bytes on its
        // next sweep (see DeleteClipboardEntryUseCase).
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

        // Pass 1: TTL sweep of the on-disk file cache (copied files only).
        // Non-fatal: a file-cache sweep failure must not block the blob
        // retention/quota pass below, which is the only one that bounds the
        // image-blob store.
        if let Err(e) = self
            .run_file_cache_ttl(retention_hours, &delete_uc, &mut result)
            .await
        {
            warn!(error = %e, "File-cache TTL sweep failed; continuing to retention/quota pass");
            result.errors += 1;
        }

        // Pass 2: entry-level age retention + total-size quota over every
        // disk-backed entry. This is the ONLY pass that reclaims clipboard
        // image blobs — pass 1 walks `file-cache/` and never sees the
        // iroh-blobs store, so without this an image-only workload grows the
        // blob store without bound (issue #957).
        self.run_blob_retention_and_quota(retention_hours, quota_bytes, &delete_uc, &mut result)
            .await;

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

    /// Pass 1: delete file-cache entries whose on-disk files have aged past
    /// `retention_hours`. Routes each expired file through the entry-aware
    /// delete path (or removes it as an orphan when no owning entry exists).
    async fn run_file_cache_ttl(
        &self,
        retention_hours: u32,
        delete_uc: &DeleteClipboardEntryUseCase,
        result: &mut CleanupResult,
    ) -> Result<()> {
        let retention_secs = retention_hours as u64 * 3600;
        let now = std::time::SystemTime::now();

        if !self.file_cache_dir.exists() {
            info!(
                path = %self.file_cache_dir.display(),
                "File cache directory does not exist, skipping TTL sweep"
            );
            return Ok(());
        }

        let expired_files = collect_expired_files(&self.file_cache_dir, now, retention_secs)?;
        if expired_files.is_empty() {
            info!("No expired cache files to clean up");
            return Ok(());
        }

        let path_to_entry = self.build_reverse_index().await?;
        info!(
            expired_files = expired_files.len(),
            indexed_paths = path_to_entry.len(),
            "Reverse index built; routing expired files to entry-level delete or orphan removal"
        );

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
        Ok(())
    }

    /// Pass 2: enforce age retention + a total-size quota over disk-backed
    /// entries (blob-backed images and file-cache files alike). Disk-backed
    /// entries are enumerated from the DB; eviction deletes them oldest-first
    /// via the entry-aware delete path, which untags blobs so iroh-blobs GC can
    /// reclaim them. Failures are logged and never propagate — a best-effort
    /// hygiene pass must not abort startup.
    async fn run_blob_retention_and_quota(
        &self,
        retention_hours: u32,
        quota_bytes: u64,
        delete_uc: &DeleteClipboardEntryUseCase,
        result: &mut CleanupResult,
    ) {
        let entries = match self.collect_disk_backed_entries().await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to enumerate disk-backed entries for retention/quota");
                return;
            }
        };
        if entries.is_empty() {
            return;
        }

        let total_bytes: u64 = entries.iter().map(|e| e.disk_bytes).sum();
        let now_ms = now_millis();
        let victims = select_entries_to_evict(entries, now_ms, retention_hours, quota_bytes);

        if victims.is_empty() {
            info!(
                total_mb = total_bytes / (1024 * 1024),
                quota_mb = quota_bytes / (1024 * 1024),
                "Blob retention/quota: within limits, nothing to evict"
            );
            return;
        }

        let candidates = victims.len();
        for entry_id in &victims {
            match delete_uc.execute(entry_id).await {
                Ok(()) => {
                    result.entries_deleted += 1;
                }
                Err(e) => {
                    warn!(
                        entry_id = %entry_id,
                        error = %e,
                        "Retention/quota delete failed for disk-backed entry"
                    );
                    result.errors += 1;
                }
            }
        }

        info!(
            candidates,
            total_mb = total_bytes / (1024 * 1024),
            quota_mb = quota_bytes / (1024 * 1024),
            retention_hours,
            "Blob retention + quota enforcement complete (disk reclaimed by iroh-blobs GC on its next sweep)"
        );
    }

    /// Enumerate every entry whose payload occupies disk (blob store or file
    /// cache), paired with its creation time and on-disk byte estimate. An
    /// entry counts as disk-backed when any representation is `BlobReady`,
    /// `Staged`, or `Processing` (i.e. its bytes live outside the DB);
    /// `Inline` reps live in the DB and `Lost`/`Failed` reps hold no bytes.
    async fn collect_disk_backed_entries(&self) -> Result<Vec<DiskBackedEntry>> {
        let mut out = Vec::new();
        let mut offset = 0usize;

        loop {
            let batch = self
                .entry_repo
                .list_entries(ENTRY_LIST_BATCH_SIZE, offset)
                .await
                .map_err(|e| anyhow::anyhow!("list entries for retention/quota: {e}"))?;

            if batch.is_empty() {
                break;
            }
            let batch_len = batch.len();

            for entry in &batch {
                let reps = match self
                    .representation_repo
                    .get_representations_for_event(&entry.event_id)
                    .await
                {
                    Ok(reps) => reps,
                    Err(e) => {
                        warn!(
                            event_id = %entry.event_id,
                            error = %e,
                            "Failed to load representations for retention/quota — skipping entry"
                        );
                        continue;
                    }
                };

                let disk_bytes: u64 = reps
                    .iter()
                    .filter(|r| {
                        matches!(
                            r.payload_state,
                            PayloadAvailability::BlobReady
                                | PayloadAvailability::Staged
                                | PayloadAvailability::Processing
                        )
                    })
                    .map(|r| r.size_bytes.max(0) as u64)
                    .sum();

                if disk_bytes > 0 {
                    out.push(DiskBackedEntry {
                        entry_id: entry.entry_id.clone(),
                        created_at_ms: entry.created_at_ms,
                        disk_bytes,
                    });
                }
            }

            offset += batch_len;
            if batch_len < ENTRY_LIST_BATCH_SIZE {
                break;
            }
        }

        Ok(out)
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

/// A clipboard entry whose payload occupies disk, with the inputs the
/// retention/quota policy needs.
#[derive(Debug, Clone)]
struct DiskBackedEntry {
    entry_id: EntryId,
    created_at_ms: i64,
    disk_bytes: u64,
}

/// Decide which disk-backed entries to evict, oldest-first, to satisfy both:
///   (a) age retention — drop entries created before `retention_hours` ago
///       (`retention_hours == 0` disables the age rule), and
///   (b) total-size quota — keep deleting the oldest remaining entries until
///       the projected total disk-backed size is `<= quota_bytes`
///       (`quota_bytes == 0` disables the quota rule).
///
/// Pure and deterministic so the policy can be unit-tested without any I/O.
/// Because entries are processed oldest-first and `freed` only grows, once an
/// entry is neither expired nor needed for the quota every newer entry is also
/// safe, so the scan can stop.
fn select_entries_to_evict(
    mut entries: Vec<DiskBackedEntry>,
    now_ms: i64,
    retention_hours: u32,
    quota_bytes: u64,
) -> Vec<EntryId> {
    entries.sort_by_key(|e| e.created_at_ms);

    let total: u64 = entries.iter().map(|e| e.disk_bytes).sum();
    let age_cutoff_ms = if retention_hours > 0 {
        Some(now_ms - (retention_hours as i64) * 3_600_000)
    } else {
        None
    };

    let mut freed: u64 = 0;
    let mut victims = Vec::new();
    for entry in entries {
        let expired = age_cutoff_ms.is_some_and(|cutoff| entry.created_at_ms < cutoff);
        let over_quota = quota_bytes > 0 && total.saturating_sub(freed) > quota_bytes;
        if !expired && !over_quota {
            break;
        }
        freed += entry.disk_bytes;
        victims.push(entry.entry_id);
    }
    victims
}

fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = 3_600_000;
    const NOW_MS: i64 = 1_000_000_000_000;

    fn entry(id: &str, created_at_ms: i64, disk_bytes: u64) -> DiskBackedEntry {
        DiskBackedEntry {
            entry_id: EntryId::from(id),
            created_at_ms,
            disk_bytes,
        }
    }

    fn ids(v: &[EntryId]) -> Vec<String> {
        v.iter().map(|e| e.to_string()).collect()
    }

    #[test]
    fn evicts_nothing_when_both_rules_disabled() {
        let entries = vec![
            entry("a", NOW_MS - 100 * HOUR_MS, 5_000),
            entry("b", NOW_MS, 5_000),
        ];
        // retention_hours = 0 and quota_bytes = 0 → both rules off.
        assert!(select_entries_to_evict(entries, NOW_MS, 0, 0).is_empty());
    }

    #[test]
    fn age_rule_evicts_only_entries_older_than_retention() {
        let entries = vec![
            entry("old", NOW_MS - 25 * HOUR_MS, 1_000), // older than 24h
            entry("fresh", NOW_MS - 1 * HOUR_MS, 1_000), // within 24h
        ];
        // 24h retention, quota disabled.
        let victims = select_entries_to_evict(entries, NOW_MS, 24, 0);
        assert_eq!(ids(&victims), vec!["old"]);
    }

    #[test]
    fn quota_rule_evicts_oldest_until_under_budget() {
        // total = 180; quota = 100; age disabled. Oldest-first until <= 100.
        let entries = vec![entry("a", 1, 60), entry("b", 2, 60), entry("c", 3, 60)];
        let victims = select_entries_to_evict(entries, NOW_MS, 0, 100);
        // drop a (180→120) and b (120→60); c kept.
        assert_eq!(ids(&victims), vec!["a", "b"]);
    }

    #[test]
    fn quota_rule_keeps_everything_when_already_under_budget() {
        let entries = vec![entry("a", 1, 40), entry("b", 2, 40)];
        assert!(select_entries_to_evict(entries, NOW_MS, 0, 100).is_empty());
    }

    #[test]
    fn age_and_quota_combine_oldest_first() {
        // total = 110, quota = 50, 24h retention.
        let entries = vec![
            entry("old", NOW_MS - 30 * HOUR_MS, 30), // expired by age
            entry("mid", NOW_MS - 1 * HOUR_MS, 40),  // fresh, but needed for quota
            entry("new", NOW_MS, 40),                // fresh, kept
        ];
        let victims = select_entries_to_evict(entries, NOW_MS, 24, 50);
        // old (age) → freed 30; still 80 > 50 so mid (quota) → freed 70; 40 <= 50 stop.
        assert_eq!(ids(&victims), vec!["old", "mid"]);
    }

    #[test]
    fn unsorted_input_is_processed_oldest_first() {
        let entries = vec![
            entry("newest", 300, 60),
            entry("oldest", 100, 60),
            entry("middle", 200, 60),
        ];
        // quota 100, total 180 → evict two oldest by created_at: oldest, middle.
        let victims = select_entries_to_evict(entries, NOW_MS, 0, 100);
        assert_eq!(ids(&victims), vec!["oldest", "middle"]);
    }
}
