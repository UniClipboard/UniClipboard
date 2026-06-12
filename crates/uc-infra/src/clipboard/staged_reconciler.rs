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
use tracing::{debug, info, warn};

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
                    debug!(
                        representation_id = %rep_id,
                        "Staged reconciler: skipped demotion due to state mismatch"
                    );
                }
                Ok(ProcessingUpdateOutcome::NotFound) => {
                    debug!(
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::testing::{ScriptedRepRepo, ScriptedReturn};
    use tempfile::TempDir;
    use uc_core::clipboard::{MimeType, PersistedClipboardRepresentation};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::BlobId;

    fn dummy_rep(id: &str) -> PersistedClipboardRepresentation {
        PersistedClipboardRepresentation::new(
            RepresentationId::from(id),
            FormatId::from("public.utf8-plain-text"),
            Some(MimeType("text/plain".to_string())),
            3,
            None,
            Some(BlobId::from("blob-x")),
        )
    }

    async fn make_spool() -> (Arc<SpoolManager>, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let spool = Arc::new(SpoolManager::new(dir.path(), 1024).expect("spool"));
        (spool, dir)
    }

    #[tokio::test]
    async fn run_once_returns_zero_when_no_candidates() {
        let (spool, _dir) = make_spool().await;
        let repo = Arc::new(ScriptedRepRepo::new());
        // staged_ids 默认空 ⇒ list_ids_by_payload_state 返回空 ⇒ 提前 return 0
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        assert_eq!(reconciler.run_once().await.unwrap(), 0);
        assert!(repo.update_processing_calls().is_empty());
    }

    #[tokio::test]
    async fn run_once_does_not_demote_when_spool_file_exists() {
        let (spool, _dir) = make_spool().await;
        let healthy_id = RepresentationId::from("rep-healthy");
        spool.write(&healthy_id, b"bytes").await.unwrap();

        let repo = Arc::new(ScriptedRepRepo::new());
        repo.set_staged_ids(vec![healthy_id.clone()]);
        // 不脚本化 update_outcomes ⇒ 若调用会 panic
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        assert_eq!(reconciler.run_once().await.unwrap(), 0);
        assert!(repo.update_processing_calls().is_empty());
    }

    #[tokio::test]
    async fn run_once_demotes_orphan_when_spool_missing() {
        let (spool, _dir) = make_spool().await;
        let orphan_id = RepresentationId::from("rep-orphan");

        let repo = Arc::new(ScriptedRepRepo::new());
        repo.set_staged_ids(vec![orphan_id.clone()]);
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            dummy_rep("rep-orphan"),
        )));
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        assert_eq!(reconciler.run_once().await.unwrap(), 1);
        let calls = repo.update_processing_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].rep_id, orphan_id);
        assert_eq!(calls[0].new_state, PayloadAvailability::Lost);
        assert_eq!(
            calls[0].last_error.as_deref(),
            Some("orphaned at startup: spool file missing")
        );
        // CAS 期望状态: Staged + Processing 两态
        assert_eq!(
            calls[0].expected_states,
            vec![PayloadAvailability::Staged, PayloadAvailability::Processing]
        );
    }

    #[tokio::test]
    async fn run_once_skips_when_state_mismatches_during_demote() {
        let (spool, _dir) = make_spool().await;
        let id = RepresentationId::from("rep-raced");

        let repo = Arc::new(ScriptedRepRepo::new());
        repo.set_staged_ids(vec![id.clone()]);
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::StateMismatch));
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        // StateMismatch 不计入 demoted, 但仍消耗一次 update 调用
        assert_eq!(reconciler.run_once().await.unwrap(), 0);
        assert_eq!(repo.update_processing_calls().len(), 1);
    }

    #[tokio::test]
    async fn run_once_skips_when_row_vanished_mid_sweep() {
        let (spool, _dir) = make_spool().await;
        let id = RepresentationId::from("rep-vanished");

        let repo = Arc::new(ScriptedRepRepo::new());
        repo.set_staged_ids(vec![id]);
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::NotFound));
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        assert_eq!(reconciler.run_once().await.unwrap(), 0);
        assert_eq!(repo.update_processing_calls().len(), 1);
    }

    #[tokio::test]
    async fn run_once_skips_when_repo_returns_error() {
        let (spool, _dir) = make_spool().await;
        let id = RepresentationId::from("rep-err");

        let repo = Arc::new(ScriptedRepRepo::new());
        repo.set_staged_ids(vec![id]);
        repo.push_update_outcome(ScriptedReturn::Err("transient db".into()));
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        // repo 错误只 warn, 不让整个 sweep 失败
        assert_eq!(reconciler.run_once().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn run_once_mixes_healthy_and_orphan_in_one_sweep() {
        let (spool, _dir) = make_spool().await;
        let healthy = RepresentationId::from("h");
        let orphan = RepresentationId::from("o");
        spool.write(&healthy, b"alive").await.unwrap();

        let repo = Arc::new(ScriptedRepRepo::new());
        // 注意顺序: list_ids_by_payload_state 不保证顺序, 我们只要保证
        // 健康那条不会触发 update_processing_result, orphan 那条触发一次。
        repo.set_staged_ids(vec![healthy.clone(), orphan.clone()]);
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            dummy_rep("o"),
        )));
        let reconciler = StagedReconciler::new(repo.clone(), spool);

        assert_eq!(reconciler.run_once().await.unwrap(), 1);
        let calls = repo.update_processing_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].rep_id, orphan);
    }
}
