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
//!
//! `execute` runs two passes:
//!   1. the file-cache TTL sweep described above (copied files only), and
//!   2. a total-size quota over disk-backed entries — the only pass that
//!      bounds the content-addressed image-blob store, which the TTL sweep
//!      never sees (issue #957).
//!
//! The quota pass is **quota-only** (no age rule for image blobs) and
//! **grandfathers** everything that predates the persisted quota baseline, so
//! it never retroactively reclaims a user's pre-upgrade history — the failure
//! mode that got the earlier age-retention variant reverted (#957). See
//! `CleanupExpiredFilesUseCase::run_quota`.

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
    CacheFsPort, ClipboardEntryRepositoryPort, ClipboardEventWriterPort,
    ClipboardRepresentationRepositoryPort, ClipboardSelectionRepositoryPort, SettingsPort,
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

/// Hidden marker file (in the app-data root) holding the cache-quota baseline
/// timestamp in milliseconds. See `CleanupExpiredFilesUseCase::quota_baseline_path`.
const QUOTA_BASELINE_FILE: &str = ".cache-quota-baseline";

pub(crate) struct CleanupExpiredFilesUseCase {
    settings: Arc<dyn SettingsPort>,
    file_cache_dir: PathBuf,
    entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
    selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    event_writer: Arc<dyn ClipboardEventWriterPort>,
    representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    blob_transfer: Option<Arc<dyn BlobTransferPort>>,
    search_index: Option<Arc<dyn SearchIndexPort>>,
    /// Filesystem port — every on-disk access this use case makes (the TTL
    /// sweep, orphan/empty-dir removal, and the quota baseline marker) routes
    /// through it rather than `std::fs`, keeping the use case infra-agnostic.
    cache_fs: Arc<dyn CacheFsPort>,
}

impl CleanupExpiredFilesUseCase {
    pub(crate) fn new(
        settings: Arc<dyn SettingsPort>,
        file_cache_dir: PathBuf,
        entry_repo: Arc<dyn ClipboardEntryRepositoryPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_writer: Arc<dyn ClipboardEventWriterPort>,
        representation_repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        cache_fs: Arc<dyn CacheFsPort>,
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
            cache_fs,
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
        // device, so a total cap is the only practical interpretation.
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
        // Non-fatal: a file-cache sweep failure must not block the quota pass
        // below, which is the only one that bounds the image-blob store.
        if let Err(e) = self
            .run_file_cache_ttl(retention_hours, &delete_uc, &mut result)
            .await
        {
            warn!(error = %e, "File-cache TTL sweep failed; continuing to quota pass");
            result.errors += 1;
        }

        // Pass 2: total-size quota over disk-backed entries (blob-backed images
        // and file-cache files alike). This is the ONLY pass that bounds the
        // image-blob store — pass 1 walks `file-cache/` and never sees the
        // iroh-blobs store, so without this an image-only workload grows the
        // blob store without bound (issue #957).
        //
        // Quota-only by design (NO age rule for image blobs) and grandfathering
        // pre-baseline data: the earlier age-retention variant retroactively
        // deleted image-heavy users' history on first upgrade and was reverted
        // (#957); this pass only ever touches data created after the quota
        // baseline was established. See `run_quota`.
        self.run_quota(quota_bytes, &delete_uc, &mut result).await;

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
    /// This sweep only ever touches `file-cache/` (copied files referenced by
    /// `text/uri-list` reps); it never sees the image-blob store.
    async fn run_file_cache_ttl(
        &self,
        retention_hours: u32,
        delete_uc: &DeleteClipboardEntryUseCase,
        result: &mut CleanupResult,
    ) -> Result<()> {
        let retention_secs = retention_hours as u64 * 3600;
        let now_ms = now_millis();
        let cache_fs = self.cache_fs.as_ref();

        if !cache_fs.exists(&self.file_cache_dir).await {
            info!(
                path = %self.file_cache_dir.display(),
                "File cache directory does not exist, skipping TTL sweep"
            );
            return Ok(());
        }

        let expired_files =
            collect_expired_files(cache_fs, &self.file_cache_dir, now_ms, retention_secs).await?;
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
                None => {
                    // Vanished between the directory walk and here (e.g. a
                    // sibling sweep): nothing to free, count it as gone rather
                    // than an error. The port's `remove_file` doesn't expose
                    // `NotFound`, so probe existence first.
                    if !cache_fs.exists(path).await {
                        result.orphans_removed += 1;
                        result.files_removed += 1;
                    } else {
                        match cache_fs.remove_file(path).await {
                            Ok(()) => {
                                info!(
                                    path = %path.display(),
                                    "Removed orphan cache file (no owning entry in DB)"
                                );
                                result.orphans_removed += 1;
                                result.files_removed += 1;
                                result.bytes_reclaimed += size;
                            }
                            Err(e) => {
                                warn!(
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to remove orphan cache file"
                                );
                                result.errors += 1;
                            }
                        }
                    }
                }
            }
        }

        cleanup_empty_dirs(cache_fs, &self.file_cache_dir).await;
        Ok(())
    }

    /// Pass 2: enforce a total-size quota over disk-backed entries (blob-backed
    /// images and file-cache files alike), evicting oldest-first. This is the
    /// only pass that reclaims clipboard image blobs.
    ///
    /// Two safety rules distinguish this from the reverted age-retention
    /// variant (#957) that deleted users' history on first upgrade:
    ///
    ///   - **Quota-only, no age rule.** Entries are evicted purely to bring the
    ///     managed total under `quota_bytes`; an entry is never deleted merely
    ///     for being old.
    ///   - **Grandfathering.** A baseline timestamp is persisted the first time
    ///     this pass runs. Only entries created at/after the baseline are
    ///     quota-managed; everything that predates the quota feature is exempt
    ///     (never evicted, never counted toward the budget). This bounds *new*
    ///     growth without retroactively reclaiming pre-upgrade payloads.
    ///
    /// Additional guards: the single most-recent managed entry is never evicted
    /// (so a freshly copied over-quota payload doesn't vanish from history), and
    /// every failure path is fail-safe — if the baseline can't be read or
    /// written, the pass evicts nothing rather than risk deleting un-baselined
    /// data. Eviction routes through the entry-aware delete path, which untags
    /// blobs so iroh-blobs GC reclaims the bytes on its next sweep.
    ///
    /// Pinned/favorited entries are a future exemption: the schema does not yet
    /// persist a favorite flag (see `ToggleFavoriteClipboardEntryUseCase`), so
    /// there is nothing to exempt today. When `is_favorited` lands, filter such
    /// entries out of the managed set before calling the eviction policy.
    async fn run_quota(
        &self,
        quota_bytes: u64,
        delete_uc: &DeleteClipboardEntryUseCase,
        result: &mut CleanupResult,
    ) {
        if quota_bytes == 0 {
            info!("Cache quota disabled (quota = 0); skipping quota enforcement");
            return;
        }

        // Baseline persistence runs through the cache-fs port.
        let cache_fs = self.cache_fs.as_ref();

        let Some(baseline_path) = self.quota_baseline_path() else {
            warn!(
                file_cache_dir = %self.file_cache_dir.display(),
                "Cannot locate app-data root for the quota baseline; skipping quota enforcement (fail-safe)"
            );
            return;
        };

        let baseline_ms = match read_quota_baseline(cache_fs, &baseline_path).await {
            Ok(Some(ms)) => ms,
            Ok(None) => {
                // First run of the quota feature: record the baseline and
                // grandfather everything that already exists. Evict nothing
                // this pass — pre-baseline data must never be reclaimed.
                let now_ms = now_millis();
                match write_quota_baseline(cache_fs, &baseline_path, now_ms).await {
                    Ok(()) => info!(
                        baseline_ms = now_ms,
                        path = %baseline_path.display(),
                        "Established cache-quota baseline; existing payloads grandfathered (exempt from quota)"
                    ),
                    Err(e) => warn!(
                        error = %e,
                        path = %baseline_path.display(),
                        "Failed to persist cache-quota baseline; skipping quota enforcement (fail-safe)"
                    ),
                }
                return;
            }
            Err(e) => {
                warn!(
                    error = %e,
                    path = %baseline_path.display(),
                    "Cache-quota baseline unreadable; skipping quota enforcement (fail-safe, baseline left intact)"
                );
                return;
            }
        };

        let entries = match self.collect_disk_backed_entries().await {
            Ok(e) => e,
            Err(e) => {
                warn!(error = %e, "Failed to enumerate disk-backed entries for quota; skipping");
                return;
            }
        };

        // Only post-baseline entries are quota-managed; pre-baseline payloads
        // are grandfathered (exempt from both the budget total and eviction).
        let grandfathered = entries
            .iter()
            .filter(|e| e.created_at_ms < baseline_ms)
            .count();
        let managed: Vec<DiskBackedEntry> = entries
            .into_iter()
            .filter(|e| e.created_at_ms >= baseline_ms)
            .collect();
        if managed.is_empty() {
            return;
        }

        let managed_total: u64 = managed.iter().map(|e| e.disk_bytes).sum();
        let victims = select_entries_to_evict_for_quota(managed, quota_bytes);

        if victims.is_empty() {
            info!(
                managed_total_mb = managed_total / (1024 * 1024),
                quota_mb = quota_bytes / (1024 * 1024),
                grandfathered,
                "Cache quota: managed payloads within budget, nothing to evict"
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
                        "Quota delete failed for disk-backed entry"
                    );
                    result.errors += 1;
                }
            }
        }

        info!(
            candidates,
            managed_total_mb = managed_total / (1024 * 1024),
            quota_mb = quota_bytes / (1024 * 1024),
            grandfathered,
            baseline_ms,
            "Cache quota enforcement complete (oldest-first; pre-baseline data grandfathered; disk reclaimed by iroh-blobs GC on its next sweep)"
        );
    }

    /// Baseline marker path: a hidden file in the app-data root (the parent of
    /// the file cache, e.g. `<app_data>/file-cache` → `<app_data>`), alongside
    /// the other app-data markers. Deliberately NOT inside `file_cache_dir`, so
    /// the TTL sweep never treats it as an orphan and removes it. `None` when
    /// the cache dir has no parent (a layout that should never occur in
    /// practice), in which case the quota pass fails safe and evicts nothing.
    fn quota_baseline_path(&self) -> Option<PathBuf> {
        self.file_cache_dir
            .parent()
            .map(|root| root.join(QUOTA_BASELINE_FILE))
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
                .map_err(|e| anyhow::anyhow!("list entries for quota: {e}"))?;

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
                            "Failed to load representations for quota — skipping entry"
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

/// A clipboard entry whose payload occupies disk, with the inputs the quota
/// policy needs.
#[derive(Debug, Clone)]
struct DiskBackedEntry {
    entry_id: EntryId,
    created_at_ms: i64,
    disk_bytes: u64,
}

/// Decide which disk-backed entries to evict, oldest-first, to bring the total
/// down to `quota_bytes`. Pure and deterministic so the policy can be
/// unit-tested without any I/O.
///
/// Quota-only: there is no age rule here — an entry is evicted solely to free
/// budget. Entries are processed oldest-first by `created_at_ms`; eviction
/// stops as soon as the projected remaining total is `<= quota_bytes`. The
/// single newest entry is never evicted, so a freshly copied payload that on
/// its own exceeds the quota is kept rather than instantly reclaimed.
/// `quota_bytes == 0` disables the quota and evicts nothing.
fn select_entries_to_evict_for_quota(
    mut entries: Vec<DiskBackedEntry>,
    quota_bytes: u64,
) -> Vec<EntryId> {
    if quota_bytes == 0 || entries.is_empty() {
        return Vec::new();
    }
    entries.sort_by_key(|e| e.created_at_ms);

    let total: u64 = entries.iter().map(|e| e.disk_bytes).sum();
    let last_idx = entries.len() - 1;

    let mut freed: u64 = 0;
    let mut victims = Vec::new();
    for (idx, entry) in entries.into_iter().enumerate() {
        if total.saturating_sub(freed) <= quota_bytes {
            break;
        }
        // Never evict the single most-recent managed entry: a just-captured
        // payload that alone exceeds the quota should stay in history rather
        // than disappear the instant it is copied.
        if idx == last_idx {
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

/// Parse a quota-baseline marker's raw contents into epoch milliseconds.
/// Kept separate from I/O so the parse/validation stays unit-testable.
fn parse_quota_baseline(contents: &[u8]) -> Result<i64> {
    let text = std::str::from_utf8(contents)
        .map_err(|e| anyhow::anyhow!("quota baseline is not valid UTF-8: {e}"))?;
    text.trim()
        .parse::<i64>()
        .map_err(|e| anyhow::anyhow!("parse quota baseline {:?}: {e}", text.trim()))
}

/// Read the persisted quota baseline (epoch milliseconds) through the cache-fs
/// port.
///
/// `Ok(None)` means the marker file does not exist yet (first run); `Ok(Some)`
/// is a valid baseline; `Err` means the file exists but could not be read or
/// parsed. Callers treat `Err` as fail-safe (skip eviction) and must NOT
/// overwrite the file, so a transient read glitch can't silently reset the
/// baseline and un-grandfather existing data.
async fn read_quota_baseline(cache_fs: &dyn CacheFsPort, path: &Path) -> Result<Option<i64>> {
    match cache_fs.read_file(path).await? {
        Some(bytes) => Ok(Some(parse_quota_baseline(&bytes)?)),
        None => Ok(None),
    }
}

async fn write_quota_baseline(cache_fs: &dyn CacheFsPort, path: &Path, ms: i64) -> Result<()> {
    cache_fs.write_file(path, ms.to_string().as_bytes()).await
}

async fn collect_expired_files(
    cache_fs: &dyn CacheFsPort,
    cache_dir: &Path,
    now_ms: i64,
    retention_secs: u64,
) -> Result<Vec<(PathBuf, u64)>> {
    let mut expired = Vec::new();
    collect_expired_recursive(cache_fs, cache_dir, now_ms, retention_secs, &mut expired).await?;
    Ok(expired)
}

async fn collect_expired_recursive(
    cache_fs: &dyn CacheFsPort,
    dir: &Path,
    now_ms: i64,
    retention_secs: u64,
    out: &mut Vec<(PathBuf, u64)>,
) -> Result<()> {
    let entries = match cache_fs.read_dir(dir).await {
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

    let retention_ms = (retention_secs as i64).saturating_mul(1000);

    for entry in entries {
        if entry.is_dir {
            // Recursive async call must be boxed.
            Box::pin(collect_expired_recursive(
                cache_fs,
                &entry.path,
                now_ms,
                retention_secs,
                out,
            ))
            .await?;
            continue;
        }

        let meta = match cache_fs.metadata(&entry.path).await {
            Ok(Some(m)) => m,
            // Vanished between the listing and the metadata read — nothing to do.
            Ok(None) => continue,
            Err(e) => {
                warn!(
                    path = %entry.path.display(),
                    error = %e,
                    "Failed to read file metadata"
                );
                continue;
            }
        };

        if meta.is_dir {
            continue;
        }
        // `None` modified time → treat as age 0 (not expired), matching the
        // prior `modified().unwrap_or(now)` behavior.
        let age_ms = meta.modified_unix_ms.map(|m| now_ms - m).unwrap_or(0);
        if age_ms >= retention_ms {
            out.push((entry.path, meta.size_bytes));
        }
    }

    Ok(())
}

async fn cleanup_empty_dirs(cache_fs: &dyn CacheFsPort, cache_dir: &Path) {
    let entries = match cache_fs.read_dir(cache_dir).await {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries {
        if !entry.is_dir {
            continue;
        }
        match cache_fs.read_dir(&entry.path).await {
            Ok(contents) if contents.is_empty() => {
                if let Err(e) = cache_fs.remove_dir(&entry.path).await {
                    warn!(
                        path = %entry.path.display(),
                        error = %e,
                        "Failed to remove empty cache directory"
                    );
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    // --- select_entries_to_evict_for_quota: pure quota policy --------------

    #[test]
    fn quota_disabled_evicts_nothing() {
        let entries = vec![entry("a", 1, 5_000), entry("b", 2, 5_000)];
        assert!(select_entries_to_evict_for_quota(entries, 0).is_empty());
    }

    #[test]
    fn quota_keeps_everything_when_already_under_budget() {
        let entries = vec![entry("a", 1, 40), entry("b", 2, 40)];
        assert!(select_entries_to_evict_for_quota(entries, 100).is_empty());
    }

    #[test]
    fn quota_evicts_oldest_until_under_budget() {
        // total = 180; quota = 100. Oldest-first until <= 100.
        let entries = vec![entry("a", 1, 60), entry("b", 2, 60), entry("c", 3, 60)];
        let victims = select_entries_to_evict_for_quota(entries, 100);
        // drop a (180→120) and b (120→60); c kept.
        assert_eq!(ids(&victims), vec!["a", "b"]);
    }

    #[test]
    fn quota_processes_oldest_first_regardless_of_input_order() {
        let entries = vec![
            entry("newest", 300, 60),
            entry("oldest", 100, 60),
            entry("middle", 200, 60),
        ];
        // quota 100, total 180 → evict the two oldest by created_at.
        let victims = select_entries_to_evict_for_quota(entries, 100);
        assert_eq!(ids(&victims), vec!["oldest", "middle"]);
    }

    #[test]
    fn quota_never_evicts_the_single_newest_entry() {
        // The newest entry alone (200) exceeds the quota (100). Everything
        // older is evicted, but the newest is kept rather than reclaimed the
        // instant it was copied — so the total stays above quota by design.
        let entries = vec![
            entry("old1", 1, 50),
            entry("old2", 2, 50),
            entry("newest", 3, 200),
        ];
        let victims = select_entries_to_evict_for_quota(entries, 100);
        assert_eq!(ids(&victims), vec!["old1", "old2"]);
    }

    #[test]
    fn quota_single_entry_is_never_evicted() {
        let entries = vec![entry("only", 1, 10_000)];
        assert!(select_entries_to_evict_for_quota(entries, 100).is_empty());
    }

    // --- quota baseline parsing (pure) ------------------------------------

    #[test]
    fn parse_baseline_accepts_trimmed_integer() {
        assert_eq!(
            parse_quota_baseline(b"  1700000000000\n").unwrap(),
            1_700_000_000_000
        );
    }

    #[test]
    fn parse_baseline_rejects_garbage() {
        // A garbage marker must surface as Err so the caller fails safe and
        // leaves the file intact, rather than parsing it as a fresh baseline
        // and un-grandfathering existing data.
        assert!(parse_quota_baseline(b"not-a-number").is_err());
    }

    // --- quota baseline persistence (via cache-fs port) -------------------

    /// In-memory `CacheFsPort` exercising only the file read/write methods the
    /// baseline helpers call; everything else is unreachable here.
    #[derive(Default)]
    struct InMemoryCacheFs {
        files: std::sync::Mutex<HashMap<PathBuf, Vec<u8>>>,
    }

    #[async_trait::async_trait]
    impl CacheFsPort for InMemoryCacheFs {
        async fn read_file(&self, path: &Path) -> Result<Option<Vec<u8>>> {
            Ok(self.files.lock().unwrap().get(path).cloned())
        }
        async fn write_file(&self, path: &Path, contents: &[u8]) -> Result<()> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), contents.to_vec());
            Ok(())
        }
        async fn exists(&self, path: &Path) -> bool {
            self.files.lock().unwrap().contains_key(path)
        }
        async fn read_dir(&self, _path: &Path) -> Result<Vec<uc_core::ports::cache_fs::DirEntry>> {
            unreachable!("not exercised by baseline tests")
        }
        async fn remove_dir_all(&self, _path: &Path) -> Result<()> {
            unreachable!("not exercised by baseline tests")
        }
        async fn remove_file(&self, _path: &Path) -> Result<()> {
            unreachable!("not exercised by baseline tests")
        }
        async fn dir_size(&self, _path: &Path) -> Result<u64> {
            unreachable!("not exercised by baseline tests")
        }
        async fn metadata(
            &self,
            _path: &Path,
        ) -> Result<Option<uc_core::ports::cache_fs::FileMetadata>> {
            unreachable!("not exercised by baseline tests")
        }
        async fn remove_dir(&self, _path: &Path) -> Result<()> {
            unreachable!("not exercised by baseline tests")
        }
    }

    #[tokio::test]
    async fn baseline_absent_reads_as_none() {
        let fs = InMemoryCacheFs::default();
        let path = PathBuf::from("/app-data").join(QUOTA_BASELINE_FILE);
        assert_eq!(read_quota_baseline(&fs, &path).await.unwrap(), None);
    }

    #[tokio::test]
    async fn baseline_round_trips() {
        let fs = InMemoryCacheFs::default();
        let path = PathBuf::from("/app-data").join(QUOTA_BASELINE_FILE);
        write_quota_baseline(&fs, &path, 1_700_000_000_000)
            .await
            .unwrap();
        assert_eq!(
            read_quota_baseline(&fs, &path).await.unwrap(),
            Some(1_700_000_000_000)
        );
    }

    #[tokio::test]
    async fn baseline_corrupt_content_is_an_error_not_a_silent_reset() {
        let fs = InMemoryCacheFs::default();
        let path = PathBuf::from("/app-data").join(QUOTA_BASELINE_FILE);
        fs.write_file(&path, b"not-a-number").await.unwrap();
        assert!(read_quota_baseline(&fs, &path).await.is_err());
    }

    // --- TTL sweep over the cache-fs port (real adapter + tempdir) --------

    #[tokio::test]
    async fn collect_expired_files_walks_recursively_and_respects_retention() {
        let fs = uc_infra::fs::TokioCacheFsAdapter::new();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::write(root.join("a.bin"), vec![0u8; 10])
            .await
            .unwrap();
        tokio::fs::create_dir(root.join("sub")).await.unwrap();
        tokio::fs::write(root.join("sub").join("b.bin"), vec![0u8; 20])
            .await
            .unwrap();

        let now_ms = now_millis();

        // retention 0 → every file is "expired"; the recursive walk finds both
        // (one at the root, one nested) with their real sizes.
        let mut all = collect_expired_files(&fs, root, now_ms, 0).await.unwrap();
        all.sort_by_key(|(_, size)| *size);
        assert_eq!(
            all.iter().map(|(_, s)| *s).collect::<Vec<_>>(),
            vec![10, 20]
        );

        // A huge retention window → nothing is old enough to sweep yet.
        let none = collect_expired_files(&fs, root, now_ms, 10_000_000_000)
            .await
            .unwrap();
        assert!(none.is_empty());
    }

    #[tokio::test]
    async fn cleanup_empty_dirs_removes_only_empty_subdirs() {
        let fs = uc_infra::fs::TokioCacheFsAdapter::new();
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        tokio::fs::create_dir(root.join("empty")).await.unwrap();
        tokio::fs::create_dir(root.join("full")).await.unwrap();
        tokio::fs::write(root.join("full").join("f.bin"), b"x")
            .await
            .unwrap();

        cleanup_empty_dirs(&fs, root).await;

        assert!(
            !fs.exists(&root.join("empty")).await,
            "empty subdir should be removed"
        );
        assert!(
            fs.exists(&root.join("full")).await,
            "non-empty subdir must be kept"
        );
    }
}
