//! Startup reconciler for orphaned `Staged` / `Processing` representations.
//!
//! Complements `SpoolScanner`, which iterates the spool directory and acts on
//! each file. The reconciler iterates *the database* — finding every
//! representation in `Staged` or `Processing` and verifying that bytes are
//! actually recoverable. Any representation whose bytes are gone from both
//! cache and spool is demoted to `Lost` so subsequent restore attempts
//! return a stable `payload_unavailable` response instead of repeatedly
//! failing through the orphaned-Staged path.
//!
//! ## Why this exists (root cause for UNICLIPBOARD-RUST-5/6)
//!
//! Pre-fix, capture spawned spool writes as detached tasks. If the process
//! exited before the spool write completed, the representation was left
//! `Staged` in DB with no bytes anywhere. The resolver bailed, the worker
//! refused to retry on cache+spool double-miss, and every restore attempt
//! produced 500 + a Sentry event on the same row forever. P1-4 (synchronous
//! spool durability) prevents *new* orphans; this reconciler cleans up the
//! *existing* ones on alpha users' machines.
//!
//! ## Behaviour
//!
//! - Lists every `Staged` / `Processing` rep via the repository port.
//! - For each, checks whether the spool file is still on disk. The in-memory
//!   cache is empty at startup, so spool presence is the sole liveness signal.
//! - On miss, calls `update_processing_result(... → Lost ...)` with a CAS on
//!   the original state. State mismatch / NotFound outcomes are logged and
//!   skipped (race with another worker is acceptable).
//! - Returns the count of demoted reps for observability. Failures during the
//!   sweep are logged but do not abort startup.

use std::sync::Arc;

use anyhow::Result;
use tracing::{info, warn};

use uc_core::clipboard::PayloadAvailability;
use uc_core::ports::clipboard::ProcessingUpdateOutcome;
use uc_core::ports::ClipboardRepresentationRepositoryPort;

use crate::clipboard::SpoolManager;

/// Reconciles representations stuck in `Staged` / `Processing` whose bytes
/// can no longer be recovered. Run once at startup, before any worker that
/// might consume these reps.
pub struct StagedReconciler {
    repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    spool: Arc<SpoolManager>,
}

impl StagedReconciler {
    pub fn new(
        repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
        spool: Arc<SpoolManager>,
    ) -> Self {
        Self { repo, spool }
    }

    /// Sweep the DB once and demote orphaned representations to `Lost`.
    /// Returns the number of representations demoted.
    pub async fn run_once(&self) -> Result<usize> {
        let candidates = self
            .repo
            .list_ids_by_payload_state(&[
                PayloadAvailability::Staged,
                PayloadAvailability::Processing,
            ])
            .await?;

        if candidates.is_empty() {
            info!("Staged reconciler: no Staged/Processing representations to check");
            return Ok(0);
        }

        let total = candidates.len();
        let mut demoted = 0usize;
        let mut healthy = 0usize;

        for rep_id in candidates {
            match self.spool.exists(&rep_id).await {
                Ok(true) => {
                    healthy += 1;
                    continue;
                }
                Ok(false) => {
                    // Fall through to demotion.
                }
                Err(err) => {
                    warn!(
                        representation_id = %rep_id,
                        error = %err,
                        "Staged reconciler: failed to stat spool file; skipping"
                    );
                    continue;
                }
            }

            match self
                .repo
                .update_processing_result(
                    &rep_id,
                    &[PayloadAvailability::Staged, PayloadAvailability::Processing],
                    None,
                    PayloadAvailability::Lost,
                    Some("orphaned at startup: spool file missing"),
                )
                .await
            {
                Ok(ProcessingUpdateOutcome::Updated(_)) => {
                    demoted += 1;
                    info!(
                        representation_id = %rep_id,
                        "Staged reconciler: demoted orphaned representation to Lost"
                    );
                }
                Ok(ProcessingUpdateOutcome::StateMismatch) => {
                    warn!(
                        representation_id = %rep_id,
                        "Staged reconciler: skipped demotion due to state mismatch"
                    );
                }
                Ok(ProcessingUpdateOutcome::NotFound) => {
                    warn!(
                        representation_id = %rep_id,
                        "Staged reconciler: representation vanished mid-sweep"
                    );
                }
                Err(err) => {
                    warn!(
                        representation_id = %rep_id,
                        error = %err,
                        "Staged reconciler: failed to demote representation"
                    );
                }
            }
        }

        info!(total, healthy, demoted, "Staged reconciler completed");
        Ok(demoted)
    }
}
