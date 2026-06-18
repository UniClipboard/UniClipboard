//! 搜索重建协调器。拥有重建状态、原因码、启动检查和进度事件。

use std::sync::Arc;

use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, instrument, warn, Instrument};
use uc_core::ports::clipboard::{ListClipboardEntriesPort, ListRepresentationsForEventPort};
use uc_core::ports::search::SearchPipelinePort;
use uc_core::ports::{ClipboardSelectionRepositoryPort, SearchIndexPort, SearchKeyDerivationPort};
use uc_core::search::{RebuildProgress, RebuildStage, SearchError};
use uc_infra::search::constants::CURRENT_INDEX_VERSION;

use crate::facade::search::{SearchProjectionBuilder, SearchStatusView};

pub const REASON_INITIAL_BACKFILL: &str = "initial_backfill";
pub const REASON_VERSION_MISMATCH: &str = "version_mismatch";
pub const REASON_MANUAL_REBUILD: &str = "manual_rebuild";
pub const REASON_REBUILD_FAILED_WAITING: &str = "rebuild_failed_waiting_for_retry";

pub const STATUS_READY: &str = "ready";
pub const STATUS_REBUILDING: &str = "rebuilding";
pub const STATUS_UNAVAILABLE: &str = "unavailable";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatusSnapshot {
    pub status: String,
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
    pub search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    pub search_pipeline: Arc<dyn SearchPipelinePort>,
    pub clipboard_entry_repo: Arc<dyn ListClipboardEntriesPort>,
    pub representation_repo: Arc<dyn ListRepresentationsForEventPort>,
    pub selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    pub current_index_version: String,
}

impl SearchCoordinatorDeps {
    pub fn new(
        search_index: Arc<dyn SearchIndexPort>,
        search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
        search_pipeline: Arc<dyn SearchPipelinePort>,
        clipboard_entry_repo: Arc<dyn ListClipboardEntriesPort>,
        representation_repo: Arc<dyn ListRepresentationsForEventPort>,
        selection_repo: Arc<dyn ClipboardSelectionRepositoryPort>,
    ) -> Self {
        Self {
            search_index,
            search_key_derivation,
            search_pipeline,
            clipboard_entry_repo,
            representation_repo,
            selection_repo,
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
}

impl SearchCoordinator {
    pub fn new(deps: SearchCoordinatorDeps) -> Self {
        let (event_tx, _) = broadcast::channel(64);
        Self {
            deps: Arc::new(deps),
            event_tx,
            rebuild_lock: Arc::new(Mutex::new(())),
            state: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<SearchCoordinatorEvent> {
        self.event_tx.subscribe()
    }

    pub async fn status_snapshot(&self) -> SearchStatusSnapshot {
        let state = self.state.lock().await;
        SearchStatusSnapshot {
            status: state.status.clone(),
            reason: state.reason.clone(),
        }
    }

    pub async fn status_view(&self) -> Result<SearchStatusView, SearchError> {
        let snapshot = self.status_snapshot().await;
        let meta = self.deps.search_index.get_index_meta().await?;
        Ok(SearchStatusView {
            state: snapshot.status,
            reason: snapshot.reason,
            last_rebuild_started_at_ms: meta.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: meta.last_rebuild_completed_at_ms,
        })
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
            warn!("search coordinator: index is blocked at startup, marking unavailable");
            self.set_state(STATUS_UNAVAILABLE, Some(REASON_REBUILD_FAILED_WAITING))
                .await;
            return;
        }

        info!("search coordinator: index is ready");
        self.set_state(STATUS_READY, None).await;
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
                let reps = match deps
                    .representation_repo
                    .get_representations_for_event(&entry.event_id)
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        debug!(
                            error = %e,
                            entry_id = %entry.entry_id,
                            "search coordinator: failed to load reps for entry, skipping"
                        );
                        continue;
                    }
                };

                let selection = match deps.selection_repo.get_selection(&entry.entry_id).await {
                    Ok(Some(sel)) => sel,
                    Ok(None) => {
                        debug!(
                            entry_id = %entry.entry_id,
                            "search coordinator: no selection for entry, skipping"
                        );
                        continue;
                    }
                    Err(e) => {
                        debug!(
                            error = %e,
                            entry_id = %entry.entry_id,
                            "search coordinator: failed to get selection for entry, skipping"
                        );
                        continue;
                    }
                };

                let pipeline_input =
                    match SearchProjectionBuilder::build_from_persisted(entry, &selection, &reps) {
                        Some(input) => input,
                        None => {
                            debug!(
                                entry_id = %entry.entry_id,
                                "search coordinator: no searchable content for entry, skipping"
                            );
                            continue;
                        }
                    };

                match deps.search_pipeline.build(&pipeline_input, &search_key) {
                    Ok((doc, postings)) => {
                        if !postings.is_empty() {
                            all_entries.push((doc, postings));
                        }
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
                let mut s = state.lock().await;
                s.status = STATUS_READY.to_string();
                s.reason = None;
                emit_status_snapshot(&event_tx, STATUS_READY, None);
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
        status: status.to_string(),
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
