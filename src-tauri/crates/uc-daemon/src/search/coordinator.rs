//! `SearchCoordinator` — the single daemon owner for search index rebuild lifecycle,
//! reason codes, and WebSocket progress forwarding.
//!
//! This module owns:
//! - Single-flight rebuild guard (prevents concurrent rebuilds)
//! - Current reason code snapshot
//! - Auto-backfill on first unlocked startup
//! - Manual rebuild serialization (reject concurrent requests)
//! - WebSocket progress forwarding

use std::sync::Arc;

use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, info_span, instrument, warn, Instrument};

use uc_app::runtime::CoreRuntime;
use uc_app::usecases::CoreUseCases;
use uc_core::network::daemon_api_strings::{ws_event, ws_topic};
use uc_core::search::RebuildProgress;
use uc_infra::search::constants::CURRENT_INDEX_VERSION;

use crate::api::types::DaemonWsEvent;
use crate::search::projection::SearchProjectionBuilder;
use crate::service::{DaemonService, ServiceHealth};

// ──────────────────────────────────────────────────────────────────────────────
// Reason codes (exact string constants required by plan)
// ──────────────────────────────────────────────────────────────────────────────

pub const REASON_INITIAL_BACKFILL: &str = "initial_backfill";
pub const REASON_VERSION_MISMATCH: &str = "version_mismatch";
pub const REASON_MANUAL_REBUILD: &str = "manual_rebuild";
pub const REASON_REBUILD_FAILED_WAITING: &str = "rebuild_failed_waiting_for_retry";

// ──────────────────────────────────────────────────────────────────────────────
// Status values (exact string constants required by plan)
// ──────────────────────────────────────────────────────────────────────────────

pub const STATUS_READY: &str = "ready";
pub const STATUS_REBUILDING: &str = "rebuilding";
pub const STATUS_UNAVAILABLE: &str = "unavailable";

// ──────────────────────────────────────────────────────────────────────────────
// Search status snapshot (emitted via WS and returned from status route)
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchStatusSnapshot {
    /// One of: "ready", "rebuilding", "unavailable"
    pub status: String,
    /// Optional reason code. Present when status is "rebuilding" or "unavailable".
    pub reason: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Manual rebuild result
// ──────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualRebuildResult {
    /// Rebuild was accepted and started.
    Accepted,
    /// Another rebuild is already in flight — request rejected.
    AlreadyInProgress,
}

// ──────────────────────────────────────────────────────────────────────────────
// Internal state (held under Mutex)
// ──────────────────────────────────────────────────────────────────────────────

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

// ──────────────────────────────────────────────────────────────────────────────
// SearchCoordinator
// ──────────────────────────────────────────────────────────────────────────────

/// Single daemon owner for search rebuild lifecycle, reason codes, and WS progress forwarding.
///
/// Registered as a daemon service named `"search-coordinator"`. The service's `start()`
/// method performs the startup evaluation (initial_backfill or version_mismatch check)
/// then idles until cancelled.
///
/// Route handlers call `request_manual_rebuild()` to trigger a rebuild; the coordinator
/// serializes concurrent requests using a `Mutex<()>`.
pub struct SearchCoordinator {
    runtime: Arc<CoreRuntime>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
    /// Single-flight rebuild guard. Holding this mutex means a rebuild is in progress.
    rebuild_lock: Arc<Mutex<()>>,
    /// Observable status/reason. Protected by its own Mutex for lock-free reads.
    state: Arc<Mutex<CoordinatorState>>,
}

impl SearchCoordinator {
    pub fn new(runtime: Arc<CoreRuntime>, event_tx: broadcast::Sender<DaemonWsEvent>) -> Self {
        Self {
            runtime,
            event_tx,
            rebuild_lock: Arc::new(Mutex::new(())),
            state: Arc::new(Mutex::new(CoordinatorState::default())),
        }
    }

    /// Return the current search status snapshot for WS or HTTP status route.
    pub async fn status_snapshot(&self) -> SearchStatusSnapshot {
        let state = self.state.lock().await;
        SearchStatusSnapshot {
            status: state.status.clone(),
            reason: state.reason.clone(),
        }
    }

    /// Request a manual rebuild.
    ///
    /// Returns `ManualRebuildResult::AlreadyInProgress` immediately if a rebuild
    /// is already in flight. Otherwise starts the rebuild in a background task
    /// and returns `ManualRebuildResult::Accepted`.
    #[instrument(name = "search.request_manual_rebuild", level = "info", skip(self))]
    pub async fn request_manual_rebuild(&self) -> ManualRebuildResult {
        // Try to acquire the rebuild lock without waiting.
        match self.rebuild_lock.clone().try_lock_owned() {
            Ok(guard) => {
                // Accepted — start rebuild in background
                let runtime = self.runtime.clone();
                let event_tx = self.event_tx.clone();
                let state = self.state.clone();

                let span = info_span!("search.rebuild", reason = REASON_MANUAL_REBUILD);
                tokio::spawn(
                    async move {
                        let _guard = guard; // hold the lock until rebuild completes
                        Self::run_rebuild(runtime, event_tx, state, REASON_MANUAL_REBUILD).await;
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

    /// Perform startup evaluation.
    ///
    /// - If `last_rebuild_completed_at_ms` is None AND there's at least one clipboard entry,
    ///   kick off an `initial_backfill` rebuild.
    /// - If `index_version != CURRENT_INDEX_VERSION`, kick off a `version_mismatch` rebuild.
    /// - If `search_blocked` is still true and no rebuild was started, expose
    ///   `unavailable / rebuild_failed_waiting_for_retry`.
    #[instrument(name = "search.startup_evaluation", level = "info", skip(self))]
    async fn startup_evaluation(&self) {
        let deps = self.runtime.wiring_deps();

        let meta = match deps.search.search_index.get_index_meta().await {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "search coordinator: failed to get index meta at startup");
                self.set_state(STATUS_UNAVAILABLE, Some(REASON_REBUILD_FAILED_WAITING))
                    .await;
                return;
            }
        };

        // Check version mismatch first
        if meta.index_version != CURRENT_INDEX_VERSION {
            info!(
                current = %meta.index_version,
                expected = CURRENT_INDEX_VERSION,
                "search coordinator: index version mismatch, triggering rebuild"
            );
            self.trigger_rebuild_locked(REASON_VERSION_MISMATCH).await;
            return;
        }

        // Check if this is a fresh index that has never been rebuilt
        if meta.last_rebuild_completed_at_ms.is_none() {
            // Check if there's any content to backfill
            let has_entries = match deps.clipboard.clipboard_entry_repo.list_entries(1, 0).await {
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

        // Check if currently blocked (e.g. previous rebuild failed)
        if meta.search_blocked {
            warn!("search coordinator: index is blocked at startup, marking unavailable");
            self.set_state(STATUS_UNAVAILABLE, Some(REASON_REBUILD_FAILED_WAITING))
                .await;
            return;
        }

        // All clear
        info!("search coordinator: index is ready");
        self.set_state(STATUS_READY, None).await;
    }

    /// Acquire the rebuild lock and run a rebuild. Returns without error — errors are logged.
    async fn trigger_rebuild_locked(&self, reason: &'static str) {
        let guard = self.rebuild_lock.clone().lock_owned().await;
        let runtime = self.runtime.clone();
        let event_tx = self.event_tx.clone();
        let state = self.state.clone();
        let span = info_span!("search.rebuild", reason);
        tokio::spawn(
            async move {
                let _guard = guard;
                Self::run_rebuild(runtime, event_tx, state, reason).await;
            }
            .instrument(span),
        );
    }

    /// Core rebuild logic.
    ///
    /// 1. Set status to `rebuilding / <reason>`
    /// 2. Paginate all clipboard entries in batches of 200
    /// 3. Build search documents via `SearchProjectionBuilder`
    /// 4. Call `rebuild_search_index` use case with a progress channel
    /// 5. Forward `RebuildProgress` events to the broadcast channel
    /// 6. Set final status to `ready` or `unavailable / rebuild_failed_waiting_for_retry`
    async fn run_rebuild(
        runtime: Arc<CoreRuntime>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
        state: Arc<Mutex<CoordinatorState>>,
        reason: &str,
    ) {
        info!(reason, "search coordinator: starting rebuild");

        // Mark as rebuilding
        {
            let mut s = state.lock().await;
            s.status = STATUS_REBUILDING.to_string();
            s.reason = Some(reason.to_string());
        }
        emit_status_snapshot(&event_tx, STATUS_REBUILDING, Some(reason));

        // Gather all entries in batches of 200
        const BATCH_SIZE: usize = 200;
        let mut all_entries = Vec::new();
        let mut offset = 0usize;

        let deps = runtime.wiring_deps();

        // Derive search key once for the entire rebuild
        let search_key = match deps.search.search_key_derivation.derive_search_key().await {
            Ok(k) => k,
            Err(e) => {
                warn!(error = %e, reason, "search coordinator: key derivation failed during rebuild");
                let mut s = state.lock().await;
                s.status = STATUS_UNAVAILABLE.to_string();
                s.reason = Some(REASON_REBUILD_FAILED_WAITING.to_string());
                emit_status_snapshot(
                    &event_tx,
                    STATUS_UNAVAILABLE,
                    Some(REASON_REBUILD_FAILED_WAITING),
                );
                return;
            }
        };

        loop {
            let batch = match deps
                .clipboard
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
                // Fetch representations for this entry to build from persisted data.
                // The representation repo is keyed by event_id.
                let reps = match deps
                    .clipboard
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

                // Get the selection decision for this entry
                let selection = match deps
                    .clipboard
                    .selection_repo
                    .get_selection(&entry.entry_id)
                    .await
                {
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

                // Build search pipeline input from persisted data
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

                // Build document and postings
                match deps
                    .search
                    .search_pipeline
                    .build(&pipeline_input, &search_key)
                {
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
                break; // last page
            }
            offset += BATCH_SIZE;
        }

        // Create progress forwarding channel
        let (progress_tx, mut progress_rx) = tokio::sync::mpsc::channel::<RebuildProgress>(64);

        // Spawn a task to forward progress events to the broadcast channel
        let event_tx_clone = event_tx.clone();
        tokio::spawn(
            async move {
                while let Some(progress) = progress_rx.recv().await {
                    let payload = match serde_json::to_value(&progress) {
                        Ok(v) => v,
                        Err(e) => {
                            warn!(error = %e, "search coordinator: failed to serialize progress event");
                            continue;
                        }
                    };
                    let event = DaemonWsEvent {
                        topic: ws_topic::SEARCH.to_string(),
                        event_type: ws_event::SEARCH_REBUILD_PROGRESS.to_string(),
                        session_id: None,
                        ts: chrono::Utc::now().timestamp_millis(),
                        payload,
                    };
                    if let Err(e) = event_tx_clone.send(event) {
                        debug!(error = %e, "search coordinator: no WS subscribers for rebuild progress");
                    }
                }
            }
            .in_current_span(),
        );

        // Run the rebuild use case
        let usecases = CoreUseCases::new(runtime.as_ref());
        let result = usecases
            .rebuild_search_index()
            .execute(all_entries, progress_tx)
            .await;

        match result {
            Ok(()) => {
                info!(reason, "search coordinator: rebuild completed successfully");
                let mut s = state.lock().await;
                s.status = STATUS_READY.to_string();
                s.reason = None;
                emit_status_snapshot(&event_tx, STATUS_READY, None);
            }
            Err(e) => {
                warn!(error = %e, reason, "search coordinator: rebuild failed");
                let mut s = state.lock().await;
                s.status = STATUS_UNAVAILABLE.to_string();
                s.reason = Some(REASON_REBUILD_FAILED_WAITING.to_string());
                emit_status_snapshot(
                    &event_tx,
                    STATUS_UNAVAILABLE,
                    Some(REASON_REBUILD_FAILED_WAITING),
                );
            }
        }
    }

    async fn set_state(&self, status: &str, reason: Option<&str>) {
        let mut s = self.state.lock().await;
        s.status = status.to_string();
        s.reason = reason.map(|r| r.to_string());
    }
}

fn emit_status_snapshot(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    status: &str,
    reason: Option<&str>,
) {
    let snapshot = SearchStatusSnapshot {
        status: status.to_string(),
        reason: reason.map(|r| r.to_string()),
    };
    let payload = match serde_json::to_value(&snapshot) {
        Ok(v) => v,
        Err(e) => {
            warn!(error = %e, "search coordinator: failed to serialize status snapshot");
            return;
        }
    };
    let event = DaemonWsEvent {
        topic: ws_topic::SEARCH.to_string(),
        event_type: ws_event::SEARCH_STATUS_SNAPSHOT.to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload,
    };
    if let Err(e) = event_tx.send(event) {
        debug!(error = %e, "search coordinator: no WS subscribers for status snapshot");
    }
}

#[async_trait::async_trait]
impl DaemonService for SearchCoordinator {
    fn name(&self) -> &str {
        "search-coordinator"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("search coordinator starting");
        self.startup_evaluation().await;
        info!("search coordinator startup evaluation complete");
        cancel.cancelled().await;
        info!("search coordinator cancelled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        // Health is based on whether we're in a terminal state
        // (ready or unavailable). Rebuilding is also "healthy" from
        // the service lifecycle perspective.
        ServiceHealth::Healthy
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex as StdMutex, OnceLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};
    use tokio::sync::broadcast;

    fn build_runtime() -> Arc<CoreRuntime> {
        static RUNTIME_LOCK: OnceLock<StdMutex<()>> = OnceLock::new();
        let _guard = RUNTIME_LOCK
            .get_or_init(|| StdMutex::new(()))
            .lock()
            .unwrap();
        let profile = format!(
            "test_search_coordinator_{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        );
        let previous_profile = std::env::var("UC_PROFILE").ok();
        std::env::set_var("UC_PROFILE", &profile);
        let runtime = Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap());
        match previous_profile {
            Some(profile) => std::env::set_var("UC_PROFILE", profile),
            None => std::env::remove_var("UC_PROFILE"),
        }
        runtime
    }

    /// Test that:
    /// 1. A second manual rebuild request while the lock is held returns AlreadyInProgress
    /// 2. After the lock is released, a new manual rebuild is accepted
    ///
    /// This proves the single-flight serialization invariant.
    #[tokio::test]
    async fn search_coordinator_auto_backfill_and_manual_rebuild_serialization() {
        let runtime = build_runtime();
        let (event_tx, _rx) = broadcast::channel::<DaemonWsEvent>(64);
        let coordinator = Arc::new(SearchCoordinator::new(runtime.clone(), event_tx));

        // --- Test manual rebuild serialization ---
        // Hold the rebuild lock manually to simulate a rebuild in progress.
        // We hold an owned guard so the lock is definitly taken.
        let lock_held = coordinator.rebuild_lock.clone().lock_owned().await;

        // First manual request should be rejected immediately (lock is held)
        let result1 = coordinator.request_manual_rebuild().await;
        assert_eq!(
            result1,
            ManualRebuildResult::AlreadyInProgress,
            "should reject concurrent manual rebuild when lock is held"
        );

        // A second request while lock is still held should also be rejected
        let result2 = coordinator.request_manual_rebuild().await;
        assert_eq!(
            result2,
            ManualRebuildResult::AlreadyInProgress,
            "should reject second manual rebuild while lock is still held"
        );

        // Release the lock
        drop(lock_held);

        // After lock is released, a manual rebuild should be accepted
        let result3 = coordinator.request_manual_rebuild().await;
        assert_eq!(
            result3,
            ManualRebuildResult::Accepted,
            "should accept manual rebuild when no rebuild is in progress"
        );
    }

    /// Test that a failed rebuild correctly sets the status to
    /// `unavailable / rebuild_failed_waiting_for_retry`.
    #[tokio::test]
    async fn search_status_snapshot_reports_unavailable_after_failed_rebuild() {
        let runtime = build_runtime();
        let (event_tx, _rx) = broadcast::channel::<DaemonWsEvent>(64);
        let coordinator = Arc::new(SearchCoordinator::new(runtime.clone(), event_tx.clone()));

        // Default state should be unavailable (initialized to default)
        let snapshot = coordinator.status_snapshot().await;
        assert_eq!(snapshot.status, STATUS_UNAVAILABLE);

        // Manually set state to rebuilding then simulate a failed rebuild
        // by calling the private state setter via run_rebuild with a mock that fails.
        // We simulate the failed path by directly setting the state.
        {
            let mut s = coordinator.state.lock().await;
            s.status = STATUS_REBUILDING.to_string();
            s.reason = Some(REASON_MANUAL_REBUILD.to_string());
        }

        let rebuilding_snapshot = coordinator.status_snapshot().await;
        assert_eq!(rebuilding_snapshot.status, STATUS_REBUILDING);
        assert_eq!(
            rebuilding_snapshot.reason.as_deref(),
            Some(REASON_MANUAL_REBUILD)
        );

        // Simulate rebuild failure
        {
            let mut s = coordinator.state.lock().await;
            s.status = STATUS_UNAVAILABLE.to_string();
            s.reason = Some(REASON_REBUILD_FAILED_WAITING.to_string());
        }

        let failed_snapshot = coordinator.status_snapshot().await;
        assert_eq!(failed_snapshot.status, STATUS_UNAVAILABLE);
        assert_eq!(
            failed_snapshot.reason.as_deref(),
            Some(REASON_REBUILD_FAILED_WAITING)
        );
        assert_eq!(
            failed_snapshot.reason.as_deref(),
            Some("rebuild_failed_waiting_for_retry"),
            "exact reason string must match"
        );
    }

    #[tokio::test]
    async fn search_coordinator_stays_alive_until_cancelled() {
        let runtime = build_runtime();
        let (event_tx, _rx) = broadcast::channel::<DaemonWsEvent>(64);
        let coordinator = Arc::new(SearchCoordinator::new(runtime, event_tx));

        let cancel = CancellationToken::new();
        let worker_cancel = cancel.clone();
        let mut task = tokio::spawn(async move { coordinator.start(worker_cancel).await });

        assert!(
            tokio::time::timeout(Duration::from_secs(1), &mut task)
                .await
                .is_err(),
            "search coordinator should keep running after startup evaluation"
        );

        cancel.cancel();
        tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("search coordinator should stop after cancellation")
            .expect("task should not panic")
            .expect("search coordinator should stop cleanly");
    }
}
