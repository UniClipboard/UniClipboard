//! Spool janitor for cleaning up expired entries.
//! 用于清理过期缓存条目的巡检器。

use std::sync::Arc;

use anyhow::Result;
use tokio::fs;
use tracing::warn;
use uc_core::clipboard::PayloadAvailability;
use uc_core::ports::clipboard::ProcessingUpdateOutcome;
use uc_core::ports::{ClipboardRepresentationRepositoryPort, ClockPort};

use crate::clipboard::SpoolManager;

/// Spool cleanup task for expired entries.
/// 过期缓存条目的清理任务。
pub struct SpoolJanitor {
    spool: Arc<SpoolManager>,
    repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
    clock: Arc<dyn ClockPort>,
    ttl_days: u64,
}

impl SpoolJanitor {
    pub fn new(
        spool: Arc<SpoolManager>,
        repo: Arc<dyn ClipboardRepresentationRepositoryPort>,
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
                    warn!(
                        representation_id = %entry.representation_id,
                        "Skipping spool file delete: state mismatch (rep moved past Staged/Processing)"
                    );
                    false
                }
                Ok(ProcessingUpdateOutcome::NotFound) => {
                    warn!(
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
