//! 搜索重建协调器。拥有重建状态、原因码、启动检查和进度事件。

use std::sync::Arc;

use std::collections::HashSet;
use tokio::sync::{broadcast, mpsc, Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, instrument, warn, Instrument};

use uc_core::clipboard::ClipboardEntry;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    ClipboardEventRepositoryPort, GetClipboardEntryPort, ListClipboardEntriesPort,
    ListRepresentationsForEventPort,
};
use uc_core::ports::search::{SearchIndexMaintenancePort, SearchPipelinePort};
use uc_core::ports::{ClipboardSelectionRepositoryPort, SearchIndexPort, SearchKeyDerivationPort};
use uc_core::search::{
    RebuildProgress, RebuildStage, SearchError, SearchResult, SearchResultsPage,
};
use uc_infra::search::constants::CURRENT_INDEX_VERSION;
use uc_infra::search::text_extractor::SearchPipelineInput;

use crate::facade::search::{SearchProjectionBuilder, SearchStatusView};

pub const REASON_INITIAL_BACKFILL: &str = "initial_backfill";
pub const REASON_VERSION_MISMATCH: &str = "version_mismatch";
pub const REASON_MANUAL_REBUILD: &str = "manual_rebuild";
pub const REASON_REBUILD_FAILED_WAITING: &str = "rebuild_failed_waiting_for_retry";
/// A prior rebuild set the persisted blocked flag but never cleared it — the
/// process exited (or the rebuild failed) between marking blocked and finalizing.
/// Startup resumes by rebuilding rather than leaving the index permanently
/// unavailable, since the blocked flag has no retry driver of its own.
pub const REASON_INTERRUPTED_REBUILD: &str = "interrupted_rebuild";

pub const STATUS_READY: &str = "ready";
pub const STATUS_REBUILDING: &str = "rebuilding";
pub const STATUS_UNAVAILABLE: &str = "unavailable";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatusSnapshot {
    /// Index availability: "ready" / "rebuilding" / "unavailable". Named `state`
    /// (not `status`) to match `SearchStatusData.state`, so the WS `search` topic
    /// carries the index status under one key for both the on-subscribe snapshot
    /// and incremental coordinator updates (a single wire shape, not two).
    pub state: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualRebuildResult {
    Accepted,
    AlreadyInProgress,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchRebuildProgressView {
    pub stage: String,
    pub indexed: u32,
    pub total: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "camelCase")]
pub enum SearchCoordinatorEvent {
    Status(SearchStatusSnapshot),
    RebuildProgress(SearchRebuildProgressView),
}

pub struct SearchCoordinatorDeps {
    pub search_index: Arc<dyn SearchIndexPort>,
    /// One-shot storage maintenance (plaintext-residue purge after the encrypting
    /// rebuild). Separate from `search_index` because it is a background-only,
    /// storage-level concern.
    pub search_maintenance: Arc<dyn SearchIndexMaintenancePort>,
    pub search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    pub search_pipeline: Arc<dyn SearchPipelinePort>,
    pub clipboard_entry_repo: Arc<dyn ListClipboardEntriesPort>,
    /// Loads a single entry by id for the corrupted-row re-projection repair.
    pub get_entry: Arc<dyn GetClipboardEntryPort>,
    pub representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    /// Resolves the originating device of each entry's event for the
    /// `source_device` render column (same lookup as the live index path).
    pub event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    pub current_index_version: String,
}

impl SearchCoordinatorDeps {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        search_maintenance: Arc<dyn SearchIndexMaintenancePort>,
        search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
        search_pipeline: Arc<dyn SearchPipelinePort>,
        clipboard_entry_repo: Arc<dyn ListClipboardEntriesPort>,
        get_entry: Arc<dyn GetClipboardEntryPort>,
        representation_repo: Arc<dyn ListRepresentationsForEventPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
        event_repo: Arc<dyn ClipboardEventRepositoryPort>,
    ) -> Self {
        Self {
            search_index,
            search_maintenance,
            search_key_derivation,
            search_pipeline,
            clipboard_entry_repo,
            get_entry,
            representation_repo,
            selection_repo,
            event_repo,
            current_index_version: CURRENT_INDEX_VERSION.to_string(),
        }
    }
}

struct CoordinatorState {
    status: String,
    reason: Option<String>,
}

impl Default for CoordinatorState {
    fn default() -> Self {
        Self {
            status: STATUS_UNAVAILABLE.to_string(),
            reason: None,
        }
    }
}

pub struct SearchCoordinator {
    deps: Arc<SearchCoordinatorDeps>,
    event_tx: broadcast::Sender<SearchCoordinatorEvent>,
    rebuild_lock: Arc<Mutex<()>>,
    state: Arc<Mutex<CoordinatorState>>,
    /// Entry ids with an in-flight re-projection repair, so repeated corruption
    /// reports for the same entry across multiple queries coalesce into one repair.
    repair_in_flight: Arc<Mutex<HashSet<String>>>,
    /// Bounds how many re-projection repairs run at once, so a whole page (or
    /// table) failing to decode cannot fan out into hundreds of concurrent
    /// repair tasks hammering the shared database.
    repair_semaphore: Arc<Semaphore>,
}

/// Maximum re-projection repairs running concurrently. Corruption is an
/// exceptional path; a small cap keeps the shared DB from being swamped while
/// still draining a backlog over successive queries.
const MAX_CONCURRENT_REPAIRS: usize = 4;

impl SearchCoordinator {
    pub fn new(deps: SearchCoordinatorDeps) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            deps: Arc::new(deps),
            event_tx,
            rebuild_lock: Arc::new(Mutex::new(())),
            state: Arc::new(Mutex::new(CoordinatorState::default())),
            repair_in_flight: Arc::new(Mutex::new(HashSet::new())),
            repair_semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_REPAIRS)),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SearchCoordinatorEvent> {
        self.event_tx.subscribe()
    }

    pub async fn status_snapshot(&self) -> SearchStatusSnapshot {
        let state = self.state.lock().await;
        SearchStatusSnapshot {
            state: state.status.clone(),
            reason: state.reason.clone(),
        }
    }

    pub async fn status_view(&self) -> Result<SearchStatusView, SearchError> {
        let snapshot = self.status_snapshot().await;
        let meta = self.deps.search_index.get_index_meta().await?;
        Ok(SearchStatusView {
            state: snapshot.state,
            reason: snapshot.reason,
            last_rebuild_started_at_ms: meta.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: meta.last_rebuild_completed_at_ms,
        })
    }

    /// Project a page of entries directly from the main store, bypassing the
    /// search index — the §4.7 degraded browse fallback.
    ///
    /// When the index is not ready (blocked / version mismatch / rebuilding) a
    /// filter-less browse is still served by re-deriving the same projection the
    /// index would hold, so cards render identically (content_type, tags, render
    /// metadata) — just sourced live instead of from the index. `total`/`has_more`
    /// are entry-page bounds: `has_more` reflects whether more entries follow this
    /// window; `total` is a lower bound (offset + entries on this page) because the
    /// main store exposes no count, which is acceptable for the transient degraded
    /// view.
    pub async fn browse_projection(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<SearchResultsPage, SearchError> {
        project_browse_page(
            self.deps.clipboard_entry_repo.as_ref(),
            self.deps.representation_repo.as_ref(),
            self.deps.selection_repo.as_ref(),
            self.deps.event_repo.as_ref(),
            limit,
            offset,
        )
        .await
    }

    #[instrument(name = "search.request_manual_rebuild", level = "info", skip(self))]
    pub async fn request_manual_rebuild(&self) -> ManualRebuildResult {
        match self.rebuild_lock.clone().try_lock_owned() {
            Ok(guard) => {
                let deps = Arc::clone(&self.deps);
                let event_tx = self.event_tx.clone();
                let state = Arc::clone(&self.state);

                let span = info_span!("search.rebuild", reason = REASON_MANUAL_REBUILD);
                tokio::spawn(
                    async move {
                        let _guard = guard;
                        Self::run_rebuild(deps, event_tx, state, REASON_MANUAL_REBUILD).await;
                    }
                    .instrument(span),
                );

                info!(reason = REASON_MANUAL_REBUILD, "search rebuild accepted");
                ManualRebuildResult::Accepted
            }
            Err(_) => {
                info!("search rebuild rejected: already in progress");
                ManualRebuildResult::AlreadyInProgress
            }
        }
    }

    /// Run a manual rebuild on the caller task.
    ///
    /// CLI uses this path because there is no daemon process to own a
    /// detached background rebuild after the CLI exits.
    #[instrument(name = "search.run_manual_rebuild_now", level = "info", skip(self))]
    pub async fn run_manual_rebuild_now(&self) -> ManualRebuildResult {
        match self.rebuild_lock.clone().try_lock_owned() {
            Ok(guard) => {
                let deps = Arc::clone(&self.deps);
                let event_tx = self.event_tx.clone();
                let state = Arc::clone(&self.state);
                let _guard = guard;
                Self::run_rebuild(deps, event_tx, state, REASON_MANUAL_REBUILD).await;
                ManualRebuildResult::Accepted
            }
            Err(_) => ManualRebuildResult::AlreadyInProgress,
        }
    }

    #[instrument(name = "search.startup_evaluation", level = "info", skip(self))]
    async fn startup_evaluation(&self) {
        let meta = match self.deps.search_index.get_index_meta().await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "search coordinator: failed to get index meta at startup");
                self.set_state(STATUS_UNAVAILABLE, Some(REASON_REBUILD_FAILED_WAITING))
                    .await;
                return;
            }
        };

        if meta.index_version != self.deps.current_index_version {
            info!(
                current = %meta.index_version,
                expected = %self.deps.current_index_version,
                "search coordinator: index version mismatch, triggering rebuild"
            );
            self.trigger_rebuild_locked(REASON_VERSION_MISMATCH).await;
            return;
        }

        if meta.last_rebuild_completed_at_ms.is_none() {
            let has_entries = match self.deps.clipboard_entry_repo.list_entries(1, 0).await {
                Ok(entries) => !entries.is_empty(),
                Err(e) => {
                    warn!(error = %e, "search coordinator: failed to list entries at startup");
                    false
                }
            };

            if has_entries {
                info!("search coordinator: no completed rebuild found and entries exist, triggering initial_backfill");
                self.trigger_rebuild_locked(REASON_INITIAL_BACKFILL).await;
                return;
            }
        }

        if meta.search_blocked {
            // The blocked flag is sticky: a rebuild sets it before doing work and
            // only clears it on successful finalize, so a rebuild interrupted
            // mid-flight (process killed, crash, or hard failure) leaves it true
            // forever. Nothing else drives a retry, so resume by rebuilding here
            // instead of dead-ending at `unavailable` until a manual rebuild.
            warn!(
                "search coordinator: index left blocked by an interrupted rebuild, resuming rebuild"
            );
            self.trigger_rebuild_locked(REASON_INTERRUPTED_REBUILD)
                .await;
            return;
        }

        // Index is on the current version and not blocked. If the one-shot
        // plaintext-residue purge (owed after the encrypting rebuild) has not yet
        // run, schedule it now — it survives a locked/killed startup by staying
        // owed (`plaintext_purge_done_ms IS NULL`) until it completes.
        if meta.plaintext_purge_done_ms.is_none() {
            self.spawn_purge_if_needed();
        }

        info!("search coordinator: index is ready");
        self.set_state(STATUS_READY, None).await;
    }

    /// Spawn the one-shot plaintext-residue purge in the background. Idempotent:
    /// the task re-reads meta and no-ops if the index is not yet on the current
    /// version or the purge already completed.
    fn spawn_purge_if_needed(&self) {
        let deps = Arc::clone(&self.deps);
        tokio::spawn(
            async move {
                purge_plaintext_residue_if_needed(&deps).await;
            }
            .instrument(info_span!("search.purge_plaintext_residue")),
        );
    }

    /// Re-run the startup decision once the encryption session becomes ready.
    ///
    /// A locked cold start cannot derive keys, so the v10 rebuild and the residue
    /// purge cannot run and would otherwise stay owed until the next process
    /// restart. This drives them the moment the session unlocks. Guarded so a
    /// rebuild already in progress is not duplicated.
    pub async fn on_session_ready(&self) {
        // If a rebuild is already running it holds the rebuild lock; it will run
        // the purge itself on completion, so there is nothing to do here.
        match self.rebuild_lock.try_lock() {
            Ok(_guard) => {
                // Guard dropped at end of scope; startup_evaluation re-acquires as
                // needed. Re-reading meta inside it is the idempotency re-check.
                info!("search coordinator: session ready, re-evaluating index state");
            }
            Err(_) => {
                debug!("search coordinator: session ready while rebuild in progress, skipping");
                return;
            }
        }
        self.startup_evaluation().await;
    }

    /// Schedule a re-projection repair for entries whose stored render payload
    /// could not be decoded. Repeated reports for the same entry across queries
    /// coalesce into a single repair via the in-flight set. Best-effort and
    /// rate-limited by construction (one task per fresh id); a repair that fails
    /// is logged and not retried in a storm.
    pub fn schedule_repair(&self, entry_ids: Vec<EntryId>) {
        for entry_id in entry_ids {
            // Bound the fan-out: if all repair permits are taken, skip this report
            // rather than queueing an unbounded backlog — a later query re-reports
            // the still-corrupted row. This caps concurrency AND memory when a
            // whole page fails to decode at once.
            let permit = match Arc::clone(&self.repair_semaphore).try_acquire_owned() {
                Ok(p) => p,
                Err(_) => continue,
            };
            let key = entry_id.to_string();
            {
                let mut in_flight = match self.repair_in_flight.try_lock() {
                    Ok(g) => g,
                    Err(_) => {
                        // Contended; skip this report — a later query re-reports it.
                        // (`permit` drops here, releasing the slot.)
                        continue;
                    }
                };
                if !in_flight.insert(key.clone()) {
                    // Already being repaired. (`permit` drops here.)
                    continue;
                }
            }
            let deps = Arc::clone(&self.deps);
            let in_flight = Arc::clone(&self.repair_in_flight);
            tokio::spawn(
                async move {
                    // Hold the permit for the whole repair so concurrency stays
                    // capped at `MAX_CONCURRENT_REPAIRS`.
                    let _permit = permit;
                    repair_entry(&deps, &entry_id).await;
                    in_flight.lock().await.remove(&entry_id.to_string());
                }
                .instrument(info_span!("search.repair_entry")),
            );
        }
    }

    async fn trigger_rebuild_locked(&self, reason: &'static str) {
        let guard = self.rebuild_lock.clone().lock_owned().await;
        let deps = Arc::clone(&self.deps);
        let event_tx = self.event_tx.clone();
        let state = Arc::clone(&self.state);
        let span = info_span!("search.rebuild", reason);
        tokio::spawn(
            async move {
                let _guard = guard;
                Self::run_rebuild(deps, event_tx, state, reason).await;
            }
            .instrument(span),
        );
    }

    async fn run_rebuild(
        deps: Arc<SearchCoordinatorDeps>,
        event_tx: broadcast::Sender<SearchCoordinatorEvent>,
        state: Arc<Mutex<CoordinatorState>>,
        reason: &str,
    ) {
        info!(reason, "search coordinator: starting rebuild");
        {
            let mut s = state.lock().await;
            s.status = STATUS_REBUILDING.to_string();
            s.reason = Some(reason.to_string());
        }
        emit_status_snapshot(&event_tx, STATUS_REBUILDING, Some(reason));

        const BATCH_SIZE: usize = 200;
        let mut all_entries = Vec::new();
        let mut offset = 0usize;

        let search_key = match deps.search_key_derivation.derive_search_key().await {
            Ok(k) => k,
            Err(e) => {
                warn!(error = %e, reason, "search coordinator: key derivation failed during rebuild");
                set_failed_state(&event_tx, &state).await;
                return;
            }
        };

        loop {
            let batch = match deps
                .clipboard_entry_repo
                .list_entries(BATCH_SIZE, offset)
                .await
            {
                Ok(b) => b,
                Err(e) => {
                    warn!(error = %e, reason, "search coordinator: failed to list entries during rebuild");
                    break;
                }
            };

            let batch_len = batch.len();

            for entry in &batch {
                let pipeline_input = match project_persisted_entry(
                    deps.representation_repo.as_ref(),
                    deps.selection_repo.as_ref(),
                    deps.event_repo.as_ref(),
                    entry,
                )
                .await
                {
                    Some(input) => input,
                    None => continue,
                };

                match deps.search_pipeline.build(&pipeline_input, &search_key) {
                    Ok((doc, postings)) => {
                        // Index every entry, including no-posting ones (e.g. an
                        // image with no searchable text), so browse and the
                        // content-type filter see the full set.
                        all_entries.push((doc, postings));
                    }
                    Err(e) => {
                        warn!(
                            error = %e,
                            entry_id = %entry.entry_id,
                            "search coordinator: pipeline build failed for entry, skipping"
                        );
                    }
                }
            }

            if batch_len < BATCH_SIZE {
                break;
            }
            offset += BATCH_SIZE;
        }

        let (progress_tx, mut progress_rx) = mpsc::channel::<RebuildProgress>(64);
        let event_tx_clone = event_tx.clone();
        tokio::spawn(
            async move {
                while let Some(progress) = progress_rx.recv().await {
                    emit_progress(&event_tx_clone, progress);
                }
            }
            .in_current_span(),
        );

        match deps.search_index.rebuild(all_entries, progress_tx).await {
            Ok(()) => {
                info!(reason, "search coordinator: rebuild completed successfully");
                {
                    let mut s = state.lock().await;
                    s.status = STATUS_READY.to_string();
                    s.reason = None;
                }
                emit_status_snapshot(&event_tx, STATUS_READY, None);
                // The rebuild rewrote every row through the encrypting projection,
                // so the dropped plaintext columns' on-disk residue can now be
                // reclaimed. Owed-once, tracked in meta.
                purge_plaintext_residue_if_needed(&deps).await;
            }
            Err(e) => {
                warn!(error = %e, reason, "search coordinator: rebuild failed");
                set_failed_state(&event_tx, &state).await;
            }
        }
    }

    async fn set_state(&self, status: &str, reason: Option<&str>) {
        let mut s = self.state.lock().await;
        s.status = status.to_string();
        s.reason = reason.map(|r| r.to_string());
        emit_status_snapshot(&self.event_tx, status, reason);
    }

    pub async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("search coordinator starting");
        self.startup_evaluation().await;
        info!("search coordinator startup evaluation complete");
        cancel.cancelled().await;
        info!("search coordinator cancelled");
        Ok(())
    }
}

async fn set_failed_state(
    event_tx: &broadcast::Sender<SearchCoordinatorEvent>,
    state: &Mutex<CoordinatorState>,
) {
    let mut s = state.lock().await;
    s.status = STATUS_UNAVAILABLE.to_string();
    s.reason = Some(REASON_REBUILD_FAILED_WAITING.to_string());
    emit_status_snapshot(
        event_tx,
        STATUS_UNAVAILABLE,
        Some(REASON_REBUILD_FAILED_WAITING),
    );
}

fn emit_status_snapshot(
    event_tx: &broadcast::Sender<SearchCoordinatorEvent>,
    status: &str,
    reason: Option<&str>,
) {
    let snapshot = SearchStatusSnapshot {
        state: status.to_string(),
        reason: reason.map(|r| r.to_string()),
    };
    let _ = event_tx.send(SearchCoordinatorEvent::Status(snapshot));
}

fn emit_progress(event_tx: &broadcast::Sender<SearchCoordinatorEvent>, progress: RebuildProgress) {
    let _ = event_tx.send(SearchCoordinatorEvent::RebuildProgress(
        rebuild_progress_to_view(progress),
    ));
}

fn rebuild_progress_to_view(progress: RebuildProgress) -> SearchRebuildProgressView {
    SearchRebuildProgressView {
        stage: rebuild_stage_to_string(progress.stage),
        indexed: progress.indexed,
        total: progress.total,
    }
}

fn rebuild_stage_to_string(stage: RebuildStage) -> String {
    match stage {
        RebuildStage::Started => "started",
        RebuildStage::Indexing => "indexing",
        RebuildStage::Complete => "complete",
        RebuildStage::Failed => "failed",
    }
    .to_string()
}

/// Re-derive one persisted entry's `SearchPipelineInput` from the main store,
/// the shared core of both the rebuild loop and the §4.7 degraded browse read.
/// Returns `None` (skip the entry) when its representations or selection are
/// missing or it carries no searchable content; a source-device lookup failure
/// degrades to `None` source rather than skipping the entry, matching the live
/// index path.
async fn project_persisted_entry(
    representation_repo: &dyn ListRepresentationsForEventPort,
    selection_repo: &dyn ClipboardSelectionRepositoryPort,
    event_repo: &dyn ClipboardEventRepositoryPort,
    entry: &ClipboardEntry,
) -> Option<SearchPipelineInput> {
    let reps = match representation_repo
        .get_representations_for_event(&entry.event_id)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            debug!(
                error = %e,
                entry_id = %entry.entry_id,
                "search projection: failed to load reps for entry, skipping"
            );
            return None;
        }
    };

    let selection = match selection_repo.get_selection(&entry.entry_id).await {
        Ok(Some(sel)) => sel,
        Ok(None) => {
            debug!(
                entry_id = %entry.entry_id,
                "search projection: no selection for entry, skipping"
            );
            return None;
        }
        Err(e) => {
            debug!(
                error = %e,
                entry_id = %entry.entry_id,
                "search projection: failed to get selection for entry, skipping"
            );
            return None;
        }
    };

    // Resolve the originating device from the event store — the same lookup the
    // live index path uses, so the two stay in parity.
    let source_device = match event_repo.get_source_device(&entry.event_id).await {
        Ok(device) => device.map(|d| d.to_string()),
        Err(e) => {
            debug!(
                error = %e,
                entry_id = %entry.entry_id,
                "search projection: failed to resolve source device, projecting without it"
            );
            None
        }
    };

    SearchProjectionBuilder::build_from_persisted(entry, &selection, &reps, source_device)
}

/// Build a degraded browse page (§4.7) directly from the main store. Extracted
/// as a free function (over the read ports it needs) so it can be unit-tested
/// without standing up a full `SearchCoordinator`.
async fn project_browse_page(
    entry_repo: &dyn ListClipboardEntriesPort,
    representation_repo: &dyn ListRepresentationsForEventPort,
    selection_repo: &dyn ClipboardSelectionRepositoryPort,
    event_repo: &dyn ClipboardEventRepositoryPort,
    limit: usize,
    offset: usize,
) -> Result<SearchResultsPage, SearchError> {
    let entries = entry_repo
        .list_entries(limit, offset)
        .await
        .map_err(|e| SearchError::Internal(format!("degraded browse: list entries failed: {e}")))?;
    // `has_more` tracks entry-page bounds (entries skipped for missing content
    // still consumed an offset slot, so paginate on entries, not projections).
    let has_more = entries.len() == limit;
    let total = (offset + entries.len()) as u32;

    let mut items = Vec::with_capacity(entries.len());
    for entry in &entries {
        if let Some(input) =
            project_persisted_entry(representation_repo, selection_repo, event_repo, entry).await
        {
            items.push(pipeline_input_to_search_result(input));
        }
    }

    Ok(SearchResultsPage {
        items,
        total,
        has_more,
        // Degraded browse reads the decrypted main store directly — there is no
        // encrypted render payload to fail decoding, so nothing is ever corrupted.
        corrupted_entry_ids: Vec::new(),
    })
}

/// Run the one-shot plaintext-residue purge if it is owed. Idempotent: no-ops
/// unless the index is on the current version and the purge has not yet run.
async fn purge_plaintext_residue_if_needed(deps: &SearchCoordinatorDeps) {
    let meta = match deps.search_index.get_index_meta().await {
        Ok(m) => m,
        Err(e) => {
            warn!(error = %e, "search coordinator: purge check failed to read index meta");
            return;
        }
    };
    if meta.index_version != deps.current_index_version {
        // Not on the target version yet — a rebuild will run the purge on success.
        return;
    }
    if meta.plaintext_purge_done_ms.is_some() {
        return;
    }
    info!("search coordinator: running one-shot plaintext-residue purge");
    if let Err(e) = deps.search_maintenance.purge_plaintext_residue().await {
        warn!(error = %e, "search coordinator: plaintext-residue purge failed; will retry on next startup");
        return;
    }
    let ts = chrono::Utc::now().timestamp_millis();
    if let Err(e) = deps.search_maintenance.mark_plaintext_purge_done(ts).await {
        warn!(error = %e, "search coordinator: purge ran but recording completion failed; will re-run next startup");
    } else {
        info!("search coordinator: plaintext-residue purge complete");
    }
}

/// Re-project a single entry whose stored render payload could not be decoded and
/// overwrite its index row (encrypting anew). Best-effort; a failure is logged
/// and not retried in a storm.
async fn repair_entry(deps: &SearchCoordinatorDeps, entry_id: &EntryId) {
    let entry = match deps.get_entry.get_entry(entry_id).await {
        Ok(Some(e)) => e,
        Ok(None) => {
            debug!(entry_id = %entry_id, "search repair: entry no longer exists, skipping");
            return;
        }
        Err(e) => {
            warn!(entry_id = %entry_id, error = %e, "search repair: failed to load entry");
            return;
        }
    };

    let input = match project_persisted_entry(
        deps.representation_repo.as_ref(),
        deps.selection_repo.as_ref(),
        deps.event_repo.as_ref(),
        &entry,
    )
    .await
    {
        Some(i) => i,
        None => {
            debug!(entry_id = %entry_id, "search repair: entry not projectable, skipping");
            return;
        }
    };

    let search_key = match deps.search_key_derivation.derive_search_key().await {
        Ok(k) => k,
        Err(e) => {
            warn!(entry_id = %entry_id, error = %e, "search repair: key derivation failed");
            return;
        }
    };

    let (doc, postings) = match deps.search_pipeline.build(&input, &search_key) {
        Ok(v) => v,
        Err(e) => {
            warn!(entry_id = %entry_id, error = %e, "search repair: pipeline build failed");
            return;
        }
    };

    match deps.search_index.index_entry(doc, postings).await {
        Ok(()) => {
            info!(entry_id = %entry_id, "search repair: re-projected corrupted render payload")
        }
        Err(e) => {
            warn!(entry_id = %entry_id, error = %e, "search repair: index_entry failed; not retrying")
        }
    }
}

/// Map a freshly re-derived `SearchPipelineInput` to a `SearchResult` so a
/// degraded browse row carries the same fields a search hit would.
fn pipeline_input_to_search_result(input: SearchPipelineInput) -> SearchResult {
    SearchResult {
        entry_id: input.entry_id,
        content_type: input.content_type,
        active_time_ms: input.active_time_ms,
        tags: input.tags,
        text_preview: input.text_preview,
        char_count: input.char_count,
        mime_type: input.mime_type,
        file_extensions: input.file_extensions,
        file_names: input.file_names,
        file_paths: input.file_paths,
        link_urls: input.link_urls,
        source_device: input.source_device,
        payload_state: input.payload_state,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;
    use uc_core::clipboard::{
        ClipboardSelection, ClipboardSelectionDecision, ObservedClipboardRepresentation,
        PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, RepresentationId};
    use uc_core::search::document::{ContentType, SearchDocument, SearchIndexMeta, SearchPosting};
    use uc_core::search::key::SearchKey;
    use uc_core::search::query::SearchQuery;
    use uc_core::search::tag::TagId;
    use uc_core::ClipboardEntryContentCategory;
    use uc_core::MimeType;

    /// Returns the supplied entries verbatim (already windowed by the test).
    struct FakeEntryRepo {
        entries: Vec<ClipboardEntry>,
    }

    #[async_trait::async_trait]
    impl ListClipboardEntriesPort for FakeEntryRepo {
        async fn list_entries(
            &self,
            limit: usize,
            offset: usize,
        ) -> Result<Vec<ClipboardEntry>, uc_core::clipboard::ClipboardRepositoryError> {
            let end = (offset + limit).min(self.entries.len());
            let start = offset.min(end);
            Ok(self.entries[start..end].to_vec())
        }
    }

    /// Returns one `text/plain` representation under a shared rep id for every
    /// event, so the shared selection's rep ids always resolve.
    struct FakeRepRepo {
        rep_id: RepresentationId,
    }

    #[async_trait::async_trait]
    impl ListRepresentationsForEventPort for FakeRepRepo {
        async fn get_representations_for_event(
            &self,
            _event_id: &EventId,
        ) -> Result<
            Vec<PersistedClipboardRepresentation>,
            uc_core::clipboard::ClipboardRepositoryError,
        > {
            Ok(vec![PersistedClipboardRepresentation::new(
                self.rep_id.clone(),
                FormatId::from("text"),
                Some(MimeType("text/plain".to_string())),
                5,
                Some(b"hello".to_vec()),
                None,
            )])
        }
    }

    /// Points preview/paste at the shared rep id for any entry.
    struct FakeSelectionRepo {
        rep_id: RepresentationId,
    }

    #[async_trait::async_trait]
    impl ClipboardSelectionRepositoryPort for FakeSelectionRepo {
        async fn get_selection(
            &self,
            entry_id: &EntryId,
        ) -> anyhow::Result<Option<ClipboardSelectionDecision>> {
            Ok(Some(ClipboardSelectionDecision::new(
                entry_id.clone(),
                ClipboardSelection {
                    primary_rep_id: self.rep_id.clone(),
                    secondary_rep_ids: Vec::new(),
                    preview_rep_id: self.rep_id.clone(),
                    paste_rep_id: self.rep_id.clone(),
                    policy_version: SelectionPolicyVersion::V1,
                },
            )))
        }

        async fn delete_selection(&self, _entry_id: &EntryId) -> anyhow::Result<()> {
            unimplemented!("not used by browse projection")
        }
    }

    /// Source device is unknown for the degraded fallback path under test.
    struct FakeEventRepo;

    #[async_trait::async_trait]
    impl ClipboardEventRepositoryPort for FakeEventRepo {
        async fn get_representation(
            &self,
            _id: &EventId,
            _representation_id: &str,
        ) -> anyhow::Result<ObservedClipboardRepresentation> {
            unimplemented!("not used by browse projection")
        }

        async fn get_source_device(&self, _event_id: &EventId) -> anyhow::Result<Option<DeviceId>> {
            Ok(None)
        }
    }

    fn entry(favorited: bool) -> ClipboardEntry {
        let e = ClipboardEntry::new(EntryId::new(), EventId::new(), 0, 0)
            .with_content_category(ClipboardEntryContentCategory::Text);
        if favorited {
            e.with_favorited(true)
        } else {
            e
        }
    }

    /// The degraded browse projection reads the main store, re-derives the same
    /// projection the index would hold (physical `content_type` + the favorited
    /// mirror tag), and reports entry-page bounds — so a filter-less browse keeps
    /// working while the index is not ready (§4.7).
    #[tokio::test]
    async fn browse_projection_reads_main_store_with_parity_fields() {
        let rep_id = RepresentationId::new();
        let entry_repo = FakeEntryRepo {
            entries: vec![entry(true), entry(false)],
        };
        let rep_repo = FakeRepRepo {
            rep_id: rep_id.clone(),
        };
        let sel_repo = FakeSelectionRepo {
            rep_id: rep_id.clone(),
        };
        let event_repo = FakeEventRepo;

        // limit 5 over 2 entries => no further page.
        let page = project_browse_page(&entry_repo, &rep_repo, &sel_repo, &event_repo, 5, 0)
            .await
            .expect("degraded browse projects without error");

        assert_eq!(page.items.len(), 2);
        assert!(!page.has_more, "2 of 2 entries fit in a page of 5");
        assert_eq!(page.total, 2);

        // Both rows are plain text re-derived through the single projection
        // authority.
        assert!(page
            .items
            .iter()
            .all(|r| r.content_type == ContentType::Text));
        // The first entry is favorited, so its mirror tag is present; the second
        // is not.
        assert!(page.items[0].tags.contains(&TagId::favorited()));
        assert!(!page.items[1].tags.contains(&TagId::favorited()));
    }

    /// A short page (fewer entries than the limit) reports `has_more = false`,
    /// while a full page reports `has_more = true` so the caller keeps paging.
    #[tokio::test]
    async fn browse_projection_reports_has_more_on_full_pages() {
        let rep_id = RepresentationId::new();
        let entry_repo = FakeEntryRepo {
            entries: vec![entry(false), entry(false), entry(false)],
        };
        let rep_repo = FakeRepRepo {
            rep_id: rep_id.clone(),
        };
        let sel_repo = FakeSelectionRepo {
            rep_id: rep_id.clone(),
        };
        let event_repo = FakeEventRepo;

        let page = project_browse_page(&entry_repo, &rep_repo, &sel_repo, &event_repo, 2, 0)
            .await
            .expect("degraded browse projects without error");

        assert_eq!(page.items.len(), 2);
        assert!(page.has_more, "page of 2 over 3 entries has a next page");
        assert_eq!(page.total, 2, "total is a lower bound of offset + page len");
    }

    // ── Startup resume of an interrupted rebuild ─────────────────────────────

    /// Records whether `rebuild` ran and serves a caller-configured meta row.
    struct FakeSearchIndex {
        meta: SearchIndexMeta,
        rebuild_called: Arc<AtomicBool>,
    }

    #[async_trait::async_trait]
    impl SearchIndexPort for FakeSearchIndex {
        async fn index_entry(
            &self,
            _document: SearchDocument,
            _postings: Vec<SearchPosting>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn remove_entry(&self, _entry_id: &EntryId) -> Result<(), SearchError> {
            Ok(())
        }

        async fn search(&self, _query: SearchQuery) -> Result<SearchResultsPage, SearchError> {
            Err(SearchError::IndexNotReady)
        }

        async fn rebuild(
            &self,
            _entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
            _progress_tx: mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            self.rebuild_called.store(true, Ordering::SeqCst);
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            Ok(self.meta.clone())
        }
    }

    struct FakeKeyDerivation;

    #[async_trait::async_trait]
    impl SearchKeyDerivationPort for FakeKeyDerivation {
        async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
            Ok(SearchKey([0u8; 32]))
        }
        async fn derive_render_key(&self) -> Result<uc_core::search::RenderKey, SearchError> {
            Ok(uc_core::search::RenderKey([0u8; 32]))
        }
    }

    /// No-op maintenance port for coordinator tests (purge is not exercised here).
    struct FakeMaintenance;

    #[async_trait::async_trait]
    impl SearchIndexMaintenancePort for FakeMaintenance {
        async fn purge_plaintext_residue(&self) -> Result<(), SearchError> {
            Ok(())
        }
        async fn mark_plaintext_purge_done(&self, _ts_ms: i64) -> Result<(), SearchError> {
            Ok(())
        }
    }

    /// Returns no entry — the repair path is not exercised in these tests.
    struct FakeGetEntry;

    #[async_trait::async_trait]
    impl GetClipboardEntryPort for FakeGetEntry {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, uc_core::clipboard::ClipboardRepositoryError> {
            Ok(None)
        }
    }

    /// Unused in the zero-entry resume test (no entries are projected), but
    /// required to satisfy the coordinator's port bound.
    struct FakePipeline;

    impl SearchPipelinePort for FakePipeline {
        fn build_document(&self, _input: &SearchPipelineInput) -> SearchDocument {
            unreachable!("no entries projected in this test")
        }

        fn build_postings(
            &self,
            _input: &SearchPipelineInput,
            _search_key: &SearchKey,
        ) -> anyhow::Result<Vec<SearchPosting>> {
            unreachable!("no entries projected in this test")
        }

        fn build(
            &self,
            _input: &SearchPipelineInput,
            _search_key: &SearchKey,
        ) -> anyhow::Result<(SearchDocument, Vec<SearchPosting>)> {
            unreachable!("no entries projected in this test")
        }
    }

    /// A sticky `search_blocked` flag left behind by an interrupted rebuild must
    /// not dead-end at `unavailable`: startup resumes by rebuilding so the index
    /// self-heals without a manual trigger. Regression for the "rebuilding banner
    /// never clears" stuck state (the blocked flag has no retry driver of its own).
    #[tokio::test]
    async fn startup_resumes_rebuild_when_index_left_blocked() {
        let rebuild_called = Arc::new(AtomicBool::new(false));
        let index = FakeSearchIndex {
            meta: SearchIndexMeta {
                index_version: CURRENT_INDEX_VERSION.to_string(),
                search_blocked: true,
                last_rebuild_started_at_ms: Some(2_000),
                // A prior rebuild completed, so the initial-backfill branch is
                // skipped and the blocked branch is the one under test.
                last_rebuild_completed_at_ms: Some(1_000),
                // Already purged, so the purge branch stays quiet.
                plaintext_purge_done_ms: Some(1_500),
            },
            rebuild_called: Arc::clone(&rebuild_called),
        };

        let rep_id = RepresentationId::new();
        let deps = SearchCoordinatorDeps::new(
            Arc::new(index),
            Arc::new(FakeMaintenance),
            Arc::new(FakeKeyDerivation),
            Arc::new(FakePipeline),
            // No entries: the resumed rebuild finalizes immediately to an empty,
            // ready index, so the pipeline build path is never reached.
            Arc::new(FakeEntryRepo { entries: vec![] }),
            Arc::new(FakeGetEntry),
            Arc::new(FakeRepRepo {
                rep_id: rep_id.clone(),
            }),
            Arc::new(FakeSelectionRepo { rep_id }),
            Arc::new(FakeEventRepo),
        );
        let coordinator = SearchCoordinator::new(deps);

        coordinator.startup_evaluation().await;

        // The rebuild runs on a spawned task; wait for it to flip state to ready.
        let mut ready = false;
        for _ in 0..200 {
            if coordinator.status_snapshot().await.state == STATUS_READY {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        assert!(
            rebuild_called.load(Ordering::SeqCst),
            "a blocked index at startup must trigger a rebuild, not dead-end at unavailable"
        );
        assert!(
            ready,
            "the resumed rebuild must drive the index back to ready"
        );
        let snapshot = coordinator.status_snapshot().await;
        assert_eq!(snapshot.state, STATUS_READY);
        assert_eq!(snapshot.reason, None);
    }

    /// Counts `get_entry` calls and holds each in-flight for `delay`, so a repair
    /// stays pending long enough for a second report of the same id to be
    /// coalesced. Returns `None` (entry gone), which ends `repair_entry` right
    /// after the load — so the call count equals the number of repairs run.
    struct CountingGetEntry {
        calls: Arc<AtomicUsize>,
        delay: Duration,
    }

    #[async_trait::async_trait]
    impl GetClipboardEntryPort for CountingGetEntry {
        async fn get_entry(
            &self,
            _entry_id: &EntryId,
        ) -> Result<Option<ClipboardEntry>, uc_core::clipboard::ClipboardRepositoryError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            tokio::time::sleep(self.delay).await;
            Ok(None)
        }
    }

    /// A corrupted entry reported across several queries (and duplicated within a
    /// batch) must coalesce into exactly one re-projection repair via the
    /// in-flight set — never one repair per report.
    #[tokio::test]
    async fn schedule_repair_coalesces_repeated_reports_for_same_entry() {
        let calls = Arc::new(AtomicUsize::new(0));
        let index = FakeSearchIndex {
            meta: SearchIndexMeta {
                index_version: CURRENT_INDEX_VERSION.to_string(),
                search_blocked: false,
                last_rebuild_started_at_ms: Some(2_000),
                last_rebuild_completed_at_ms: Some(1_000),
                plaintext_purge_done_ms: Some(1_500),
            },
            rebuild_called: Arc::new(AtomicBool::new(false)),
        };
        let rep_id = RepresentationId::new();
        let deps = SearchCoordinatorDeps::new(
            Arc::new(index),
            Arc::new(FakeMaintenance),
            Arc::new(FakeKeyDerivation),
            Arc::new(FakePipeline),
            Arc::new(FakeEntryRepo { entries: vec![] }),
            Arc::new(CountingGetEntry {
                calls: Arc::clone(&calls),
                // Long enough that the second report lands while the first repair
                // is still in flight, so dedup is exercised deterministically.
                delay: Duration::from_millis(100),
            }),
            Arc::new(FakeRepRepo {
                rep_id: rep_id.clone(),
            }),
            Arc::new(FakeSelectionRepo { rep_id }),
            Arc::new(FakeEventRepo),
        );
        let coordinator = SearchCoordinator::new(deps);

        let id = EntryId::from("corrupt-1");
        // First "query" reports the id twice in one batch; a second "query"
        // reports it again a moment later — three reports, one entry.
        coordinator.schedule_repair(vec![id.clone(), id.clone()]);
        coordinator.schedule_repair(vec![id.clone()]);

        // Wait past the in-flight window for the single repair to finish.
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "repeated corruption reports for one entry must coalesce into a single repair"
        );
    }
}
