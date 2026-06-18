//! Spool janitor for cleaning up expired entries.
//! 用于清理过期缓存条目的巡检器。

use std::sync::Arc;

use anyhow::Result;
use tokio::fs;
use tracing::{debug, warn};
use uc_core::clipboard::PayloadAvailability;
use uc_core::ports::clipboard::ProcessingUpdateOutcome;
use uc_core::ports::{ClipboardRepresentationStore, ClockPort};

use crate::clipboard::SpoolManager;

/// Spool cleanup task for expired entries.
/// 过期缓存条目的清理任务。
pub struct SpoolJanitor {
    spool: Arc<SpoolManager>,
    repo: Arc<dyn ClipboardRepresentationStore>,
    clock: Arc<dyn ClockPort>,
    ttl_days: u64,
}

impl SpoolJanitor {
    pub fn new(
        spool: Arc<SpoolManager>,
        repo: Arc<dyn ClipboardRepresentationStore>,
        clock: Arc<dyn ClockPort>,
        ttl_days: u64,
    ) -> Self {
        Self {
            spool,
            repo,
            clock,
            ttl_days,
        }
    }

    pub async fn run_once(&self) -> Result<usize> {
        let expired = self
            .spool
            .list_expired(self.clock.now_ms(), self.ttl_days)
            .await?;
        let mut removed = 0usize;
        for entry in expired {
            // Only delete the spool file when the DB transition to Lost
            // actually applied. If the transition fails or hits a state
            // mismatch (rep already moved past Staged/Processing —
            // potentially BlobReady mid-cleanup), keeping the file lets
            // the next sweep retry. Previously the delete ran
            // unconditionally, which would turn a transient DB error
            // (StateMismatch / Err) into a permanent orphan: DB still
            // says Staged, spool gone, restore 500 forever.
            let updated = match self
                .repo
                .update_processing_result(
                    &entry.representation_id,
                    &[PayloadAvailability::Staged, PayloadAvailability::Processing],
                    None,
                    PayloadAvailability::Lost,
                    Some("spool ttl expired"),
                )
                .await
            {
                Ok(ProcessingUpdateOutcome::Updated(_)) => true,
                Ok(ProcessingUpdateOutcome::StateMismatch) => {
                    debug!(
                        representation_id = %entry.representation_id,
                        "Skipping spool file delete: state mismatch (rep moved past Staged/Processing)"
                    );
                    false
                }
                Ok(ProcessingUpdateOutcome::NotFound) => {
                    debug!(
                        representation_id = %entry.representation_id,
                        "Skipping spool file delete: representation missing from DB"
                    );
                    // The rep is gone from DB; the spool file is genuinely
                    // orphaned. Safe to delete.
                    true
                }
                Err(err) => {
                    warn!(
                        representation_id = %entry.representation_id,
                        error = %err,
                        "Failed to mark Lost during spool cleanup; leaving spool file for retry"
                    );
                    false
                }
            };

            if updated {
                if let Err(err) = fs::remove_file(&entry.file_path).await {
                    warn!(
                        representation_id = %entry.representation_id,
                        error = %err,
                        "Failed to delete expired spool file"
                    );
                } else {
                    removed += 1;
                }
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::testing::{ScriptedRepRepo, ScriptedReturn};
    use std::time::Duration;
    use tempfile::TempDir;
    use uc_core::clipboard::{MimeType, PersistedClipboardRepresentation};
    use uc_core::ids::{FormatId, RepresentationId};
    use uc_core::BlobId;

    struct FixedClock(i64);
    impl uc_core::ports::ClockPort for FixedClock {
        fn now_ms(&self) -> i64 {
            self.0
        }
    }

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

    async fn make_spool_with(reps: &[&str]) -> (Arc<SpoolManager>, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let spool = Arc::new(SpoolManager::new(dir.path(), 1024).expect("spool"));
        for id in reps {
            spool
                .write(&RepresentationId::from(*id), b"data")
                .await
                .expect("write");
        }
        // 让 mtime 与 now 拉开距离, 配合 ttl_days=0 让所有文件命中 list_expired
        tokio::time::sleep(Duration::from_millis(20)).await;
        (spool, dir)
    }

    fn make_janitor(spool: Arc<SpoolManager>, repo: Arc<ScriptedRepRepo>) -> SpoolJanitor {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        SpoolJanitor::new(spool, repo, Arc::new(FixedClock(now_ms)), 0)
    }

    #[tokio::test]
    async fn run_once_returns_zero_when_nothing_expired() {
        let (spool, _dir) = make_spool_with(&[]).await;
        let repo = Arc::new(ScriptedRepRepo::new());

        assert_eq!(
            make_janitor(spool, repo.clone()).run_once().await.unwrap(),
            0
        );
        assert!(repo.update_processing_calls().is_empty());
    }

    #[tokio::test]
    async fn run_once_deletes_file_after_successful_demote() {
        let (spool, _dir) = make_spool_with(&["rep-ok"]).await;
        let repo = Arc::new(ScriptedRepRepo::new());
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            dummy_rep("rep-ok"),
        )));

        assert_eq!(
            make_janitor(spool.clone(), repo.clone())
                .run_once()
                .await
                .unwrap(),
            1
        );
        let calls = repo.update_processing_calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].rep_id, RepresentationId::from("rep-ok"));
        assert_eq!(calls[0].new_state, PayloadAvailability::Lost);
        assert_eq!(calls[0].last_error.as_deref(), Some("spool ttl expired"));
        assert!(!spool
            .exists(&RepresentationId::from("rep-ok"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn run_once_keeps_file_on_state_mismatch() {
        let (spool, _dir) = make_spool_with(&["rep-busy"]).await;
        let repo = Arc::new(ScriptedRepRepo::new());
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::StateMismatch));

        assert_eq!(
            make_janitor(spool.clone(), repo).run_once().await.unwrap(),
            0
        );
        // 保留 spool 文件让下次 sweep 重试, 而不是把 DB row 永久 orphan
        assert!(spool
            .exists(&RepresentationId::from("rep-busy"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn run_once_deletes_file_when_db_row_not_found() {
        let (spool, _dir) = make_spool_with(&["rep-ghost"]).await;
        let repo = Arc::new(ScriptedRepRepo::new());
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::NotFound));

        assert_eq!(
            make_janitor(spool.clone(), repo).run_once().await.unwrap(),
            1
        );
        assert!(!spool
            .exists(&RepresentationId::from("rep-ghost"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn run_once_keeps_file_on_repo_error() {
        let (spool, _dir) = make_spool_with(&["rep-err"]).await;
        let repo = Arc::new(ScriptedRepRepo::new());
        repo.push_update_outcome(ScriptedReturn::Err("transient db error".into()));

        assert_eq!(
            make_janitor(spool.clone(), repo).run_once().await.unwrap(),
            0
        );
        assert!(spool
            .exists(&RepresentationId::from("rep-err"))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn run_once_processes_each_expired_entry_independently() {
        let (spool, _dir) = make_spool_with(&["a", "b", "c"]).await;
        let repo = Arc::new(ScriptedRepRepo::new());
        // 顺序按 list_entries_by_mtime 稳定排序 (mtime 后按 id 字典序): a → b → c
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::Updated(
            dummy_rep("a"),
        )));
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::StateMismatch));
        repo.push_update_outcome(ScriptedReturn::Ok(ProcessingUpdateOutcome::NotFound));

        assert_eq!(
            make_janitor(spool.clone(), repo.clone())
                .run_once()
                .await
                .unwrap(),
            2
        );
        assert_eq!(repo.update_processing_calls().len(), 3);
        assert!(!spool.exists(&RepresentationId::from("a")).await.unwrap());
        assert!(spool.exists(&RepresentationId::from("b")).await.unwrap());
        assert!(!spool.exists(&RepresentationId::from("c")).await.unwrap());
    }
}
