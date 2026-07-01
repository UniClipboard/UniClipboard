//! Background clipboard history cleanup worker for the daemon.
//!
//! Runs the daemon's maintenance sweeps on a single shared cadence: once at
//! startup, then every [`CLEANUP_INTERVAL`] until cancelled. Extracted from
//! the ad-hoc `run_file_cache_hygiene` detached task (previously spawned
//! outside the `DaemonService`/`JoinSet` lifecycle) so cleanup is a proper
//! managed service and shuts down cooperatively with the rest of the daemon.
//!
//! Jobs run in a fixed order every tick:
//!
//! 1. **Reconcile** (`reconcile_missing_files`): drop DB entries whose
//!    cache-managed file no longer exists on disk. Must run first — an
//!    entry pointing at a vanished file is exactly the precondition for the
//!    iroh-blobs "poisoned storage" panic once any later step observes the
//!    hash. If reconcile itself fails, the rest of the tick is skipped
//!    entirely rather than risk running the later steps against unreconciled
//!    state.
//! 2. **File-cache cleanup** (`cleanup_expired_files`): the existing
//!    `file_sync.*`-driven TTL sweep + disk quota enforcement.
//! 3. **Retention policy** (`enforce_retention_policy`): the user-configured
//!    `retention_policy` age/count rules, applied across the full clipboard
//!    history (not just disk-backed entries).

use std::sync::Arc;

use async_trait::async_trait;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use uc_application::facade::ClipboardHistoryFacade;

use crate::daemon::service::{DaemonService, ServiceHealth};

/// Interval between cleanup sweeps after the startup pass. Cleanup is
/// idempotent and cheap when there is nothing to do, so a single shared
/// cadence for all three jobs keeps this simple rather than giving each job
/// its own timer.
const CLEANUP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

pub struct CleanupWorker {
    history_facade: Arc<ClipboardHistoryFacade>,
}

impl CleanupWorker {
    pub fn new(history_facade: Arc<ClipboardHistoryFacade>) -> Self {
        Self { history_facade }
    }

    async fn run_jobs(&self) {
        // reconcile must actually succeed before the delete-capable passes
        // run — a failed reconcile means a dangling `file://` entry may still
        // be sitting in the DB, which is exactly the precondition for the
        // iroh-blobs "poisoned storage" panic once cleanup/retention observes
        // its hash. Skip the rest of this tick rather than risk it.
        if !self.reconcile().await {
            return;
        }
        self.cleanup_file_cache().await;
        self.enforce_retention().await;
    }

    /// Returns `true` on success (including a no-op success), `false` if the
    /// reconcile pass itself failed and the caller must not proceed.
    async fn reconcile(&self) -> bool {
        match self.history_facade.reconcile_missing_files().await {
            Ok(result) => {
                if result.entries_deleted > 0 || result.errors > 0 {
                    info!(
                        entries_scanned = result.entries_scanned,
                        entries_deleted = result.entries_deleted,
                        errors = result.errors,
                        "Reconcile dropped stale entries with missing cache files"
                    );
                }
                true
            }
            Err(e) => {
                warn!(error = %e, "Reconcile failed; skipping the rest of this cleanup tick");
                false
            }
        }
    }

    async fn cleanup_file_cache(&self) {
        match self.history_facade.cleanup_expired_files().await {
            Ok(result) => {
                if result.files_removed > 0 {
                    info!(
                        files_removed = result.files_removed,
                        entries_deleted = result.entries_deleted,
                        orphans_removed = result.orphans_removed,
                        bytes_reclaimed = result.bytes_reclaimed,
                        errors = result.errors,
                        "File cache cleanup completed"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "File cache cleanup failed (non-fatal)");
            }
        }
    }

    async fn enforce_retention(&self) {
        match self.history_facade.enforce_retention_policy().await {
            Ok(result) => {
                if result.entries_deleted > 0 || result.errors > 0 {
                    info!(
                        entries_deleted = result.entries_deleted,
                        errors = result.errors,
                        "Retention policy enforcement completed"
                    );
                }
            }
            Err(e) => {
                warn!(error = %e, "Retention policy enforcement failed (non-fatal)");
            }
        }
    }
}

#[async_trait]
impl DaemonService for CleanupWorker {
    fn name(&self) -> &str {
        "cleanup"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("cleanup worker starting");

        self.run_jobs().await;

        let mut ticker = tokio::time::interval(CLEANUP_INTERVAL);
        // Collapse missed ticks into one sweep on resume instead of bursting
        // catch-up runs (e.g. after the host sleeps for hours).
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // The first tick is immediate; consume it since the startup pass just ran.
        ticker.tick().await;

        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = ticker.tick() => self.run_jobs().await,
            }
        }

        info!("cleanup worker cancelled");
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!("cleanup worker stopped");
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}
