//! Use case for tracking receiver-side file transfer lifecycle.
//!
//! Orchestrates state transitions through `FileTransferRepositoryPort`
//! without touching Diesel, Tauri, or filesystem implementation details.

use std::sync::Arc;

use anyhow::Result;
use tracing::{debug_span, info, info_span, warn, Instrument};
use uc_core::ports::file_transfer_repository::{
    EntryTransferSummary, ExpiredInflightTransfer, FileTransferRepositoryPort,
    PendingInboundTransfer,
};

/// Timeout budget: pending transfers fail after 60 seconds without first chunk.
pub const PENDING_TIMEOUT_MS: i64 = 60_000;

/// Timeout budget: transferring transfers fail after 5 minutes without new chunk activity.
pub const TRANSFERRING_TIMEOUT_MS: i64 = 300_000;

/// App-layer use case for tracking inbound file transfer state transitions.
///
/// Free of Diesel, Tauri, and filesystem implementation details.
pub struct TrackInboundTransfersUseCase {
    repo: Arc<dyn FileTransferRepositoryPort>,
}

impl TrackInboundTransfersUseCase {
    pub fn new(repo: Arc<dyn FileTransferRepositoryPort>) -> Self {
        Self { repo }
    }

    /// Seed pending transfer records from clipboard metadata.
    ///
    /// Called by `SyncInboundClipboardUseCase` after a file-backed entry is persisted.
    pub async fn record_pending_from_clipboard(
        &self,
        transfers: Vec<PendingInboundTransfer>,
    ) -> Result<()> {
        if transfers.is_empty() {
            return Ok(());
        }

        let count = transfers.len();
        async {
            self.repo.insert_pending_transfers(&transfers).await?;
            info!(
                count,
                "Seeded pending transfer records from clipboard metadata"
            );
            Ok(())
        }
        .instrument(info_span!("track_inbound.record_pending", count))
        .await
    }

    /// Promote a transfer to `transferring` on first data chunk.
    pub async fn mark_transferring(&self, transfer_id: &str, now_ms: i64) -> Result<bool> {
        self.repo
            .mark_transferring(transfer_id, now_ms)
            .instrument(info_span!("track_inbound.mark_transferring", transfer_id))
            .await
    }

    /// Refresh liveness timestamp on subsequent progress events.
    pub async fn refresh_activity(&self, transfer_id: &str, now_ms: i64) -> Result<()> {
        self.repo
            .refresh_activity(transfer_id, now_ms)
            .instrument(info_span!("track_inbound.refresh_activity", transfer_id))
            .await
    }

    /// Mark a transfer as completed.
    ///
    /// Returns `true` if a row was actually updated, `false` if the pending
    /// record hasn't been seeded yet (race condition).
    pub async fn mark_completed(
        &self,
        transfer_id: &str,
        content_hash: Option<&str>,
        now_ms: i64,
    ) -> Result<bool> {
        self.repo
            .mark_completed(transfer_id, content_hash, now_ms)
            .instrument(info_span!("track_inbound.mark_completed", transfer_id))
            .await
    }

    /// Mark a transfer as failed with a reason.
    pub async fn mark_failed(&self, transfer_id: &str, reason: &str, now_ms: i64) -> Result<()> {
        self.repo
            .mark_failed(transfer_id, reason, now_ms)
            .instrument(info_span!("track_inbound.mark_failed", transfer_id))
            .await
    }

    /// List expired in-flight transfers for timeout sweep.
    ///
    /// Uses the locked timeout budgets:
    /// - pending: `PENDING_TIMEOUT_MS` (60s)
    /// - transferring: `TRANSFERRING_TIMEOUT_MS` (5min)
    pub async fn list_expired_inflight(&self, now_ms: i64) -> Result<Vec<ExpiredInflightTransfer>> {
        let pending_cutoff = now_ms - PENDING_TIMEOUT_MS;
        let transferring_cutoff = now_ms - TRANSFERRING_TIMEOUT_MS;

        self.repo
            .list_expired_inflight(pending_cutoff, transferring_cutoff)
            .instrument(debug_span!("track_inbound.list_expired_inflight"))
            .await
    }

    /// Startup reconciliation: bulk-fail all in-flight transfers and return cleanup targets.
    ///
    /// Returns expired transfer records whose `cached_path` the platform layer
    /// can use to delete partial downloads.
    pub async fn reconcile_inflight_after_startup(
        &self,
        now_ms: i64,
    ) -> Result<Vec<ExpiredInflightTransfer>> {
        let reason = "orphaned: app restarted while transfer was in-flight";

        let cleanup_targets = self
            .repo
            .bulk_fail_inflight(reason, now_ms)
            .instrument(info_span!("track_inbound.reconcile_startup"))
            .await?;

        if !cleanup_targets.is_empty() {
            warn!(
                count = cleanup_targets.len(),
                "Reconciled in-flight transfers after startup — marked as failed"
            );
        }

        Ok(cleanup_targets)
    }

    /// Get aggregate transfer summary for an entry (for projections).
    pub async fn get_entry_summary(&self, entry_id: &str) -> Result<Option<EntryTransferSummary>> {
        self.repo.get_entry_transfer_summary(entry_id).await
    }

    /// Look up the entry_id for a given transfer_id.
    ///
    /// Used by platform wiring when only transfer_id is available (e.g., progress events).
    pub async fn get_entry_summary_by_transfer(&self, transfer_id: &str) -> Result<Option<String>> {
        self.repo.get_entry_id_for_transfer(transfer_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use uc_core::ports::file_transfer_repository::{
        TrackedFileTransfer, TrackedFileTransferStatus,
    };

    /// In-memory mock for FileTransferRepositoryPort.
    struct MockFileTransferRepo {
        transfers: Mutex<Vec<TrackedFileTransfer>>,
    }

    impl MockFileTransferRepo {
        fn new() -> Self {
            Self {
                transfers: Mutex::new(Vec::new()),
            }
        }

        fn find(&self, transfer_id: &str) -> Option<TrackedFileTransfer> {
            self.transfers
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.transfer_id == transfer_id)
                .cloned()
        }
    }

    #[async_trait::async_trait]
    impl FileTransferRepositoryPort for MockFileTransferRepo {
        async fn insert_pending_transfers(
            &self,
            transfers: &[PendingInboundTransfer],
        ) -> Result<()> {
            let mut store = self.transfers.lock().unwrap();
            for t in transfers {
                store.push(TrackedFileTransfer {
                    transfer_id: t.transfer_id.clone(),
                    entry_id: t.entry_id.clone(),
                    origin_device_id: t.origin_device_id.clone(),
                    filename: t.filename.clone(),
                    cached_path: t.cached_path.clone(),
                    status: TrackedFileTransferStatus::Pending,
                    failure_reason: None,
                    file_size: None,
                    content_hash: None,
                    updated_at_ms: t.created_at_ms,
                    created_at_ms: t.created_at_ms,
                });
            }
            Ok(())
        }

        async fn backfill_announce_metadata(
            &self,
            transfer_id: &str,
            file_size: i64,
            content_hash: &str,
        ) -> Result<()> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.file_size = Some(file_size);
                t.content_hash = Some(content_hash.to_string());
            }
            Ok(())
        }

        async fn mark_transferring(&self, transfer_id: &str, now_ms: i64) -> Result<bool> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                if t.status == TrackedFileTransferStatus::Pending {
                    t.status = TrackedFileTransferStatus::Transferring;
                    t.updated_at_ms = now_ms;
                    return Ok(true);
                }
            }
            Ok(false)
        }

        async fn refresh_activity(&self, transfer_id: &str, now_ms: i64) -> Result<()> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.updated_at_ms = now_ms;
            }
            Ok(())
        }

        async fn mark_completed(
            &self,
            transfer_id: &str,
            content_hash: Option<&str>,
            now_ms: i64,
        ) -> Result<bool> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.status = TrackedFileTransferStatus::Completed;
                t.content_hash = content_hash.map(|s| s.to_string());
                t.updated_at_ms = now_ms;
                Ok(true)
            } else {
                Ok(false)
            }
        }

        async fn mark_failed(&self, transfer_id: &str, reason: &str, now_ms: i64) -> Result<()> {
            let mut store = self.transfers.lock().unwrap();
            if let Some(t) = store.iter_mut().find(|t| t.transfer_id == transfer_id) {
                t.status = TrackedFileTransferStatus::Failed;
                t.failure_reason = Some(reason.to_string());
                t.updated_at_ms = now_ms;
            }
            Ok(())
        }

        async fn list_expired_inflight(
            &self,
            pending_cutoff_ms: i64,
            transferring_cutoff_ms: i64,
        ) -> Result<Vec<ExpiredInflightTransfer>> {
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .filter(|t| {
                    (t.status == TrackedFileTransferStatus::Pending
                        && t.updated_at_ms < pending_cutoff_ms)
                        || (t.status == TrackedFileTransferStatus::Transferring
                            && t.updated_at_ms < transferring_cutoff_ms)
                })
                .map(|t| ExpiredInflightTransfer {
                    transfer_id: t.transfer_id.clone(),
                    entry_id: t.entry_id.clone(),
                    cached_path: t.cached_path.clone(),
                    status: t.status,
                })
                .collect())
        }

        async fn bulk_fail_inflight(
            &self,
            reason: &str,
            now_ms: i64,
        ) -> Result<Vec<ExpiredInflightTransfer>> {
            let mut store = self.transfers.lock().unwrap();
            let mut cleanup = Vec::new();
            for t in store.iter_mut() {
                if t.status == TrackedFileTransferStatus::Pending
                    || t.status == TrackedFileTransferStatus::Transferring
                {
                    cleanup.push(ExpiredInflightTransfer {
                        transfer_id: t.transfer_id.clone(),
                        entry_id: t.entry_id.clone(),
                        cached_path: t.cached_path.clone(),
                        status: t.status,
                    });
                    t.status = TrackedFileTransferStatus::Failed;
                    t.failure_reason = Some(reason.to_string());
                    t.updated_at_ms = now_ms;
                }
            }
            Ok(cleanup)
        }

        async fn get_entry_transfer_summary(
            &self,
            entry_id: &str,
        ) -> Result<Option<EntryTransferSummary>> {
            let store = self.transfers.lock().unwrap();
            let entry_transfers: Vec<&TrackedFileTransfer> =
                store.iter().filter(|t| t.entry_id == entry_id).collect();

            if entry_transfers.is_empty() {
                return Ok(None);
            }

            let statuses: Vec<TrackedFileTransferStatus> =
                entry_transfers.iter().map(|t| t.status).collect();
            let aggregate =
                uc_core::ports::file_transfer_repository::compute_aggregate_status(&statuses)
                    .unwrap_or(TrackedFileTransferStatus::Pending);

            let failure_reason = if aggregate == TrackedFileTransferStatus::Failed {
                entry_transfers
                    .iter()
                    .find(|t| t.status == TrackedFileTransferStatus::Failed)
                    .and_then(|t| t.failure_reason.clone())
            } else {
                None
            };

            let transfer_ids: Vec<String> = entry_transfers
                .iter()
                .map(|t| t.transfer_id.clone())
                .collect();

            Ok(Some(EntryTransferSummary {
                entry_id: entry_id.to_string(),
                aggregate_status: aggregate,
                failure_reason,
                transfer_ids,
            }))
        }

        async fn list_transfers_for_entry(
            &self,
            entry_id: &str,
        ) -> Result<Vec<TrackedFileTransfer>> {
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .filter(|t| t.entry_id == entry_id)
                .cloned()
                .collect())
        }

        async fn get_entry_id_for_transfer(&self, transfer_id: &str) -> Result<Option<String>> {
            let store = self.transfers.lock().unwrap();
            Ok(store
                .iter()
                .find(|t| t.transfer_id == transfer_id)
                .map(|t| t.entry_id.clone()))
        }
    }

    #[tokio::test]
    async fn test_record_pending_and_mark_transferring() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![PendingInboundTransfer {
            transfer_id: "t1".to_string(),
            entry_id: "e1".to_string(),
            origin_device_id: "d1".to_string(),
            filename: "hello.txt".to_string(),
            cached_path: "/cache/t1/hello.txt".to_string(),
            created_at_ms: 1000,
        }];

        uc.record_pending_from_clipboard(transfers).await.unwrap();

        let t = repo.find("t1").unwrap();
        assert_eq!(t.status, TrackedFileTransferStatus::Pending);

        let promoted = uc.mark_transferring("t1", 2000).await.unwrap();
        assert!(promoted);

        let t = repo.find("t1").unwrap();
        assert_eq!(t.status, TrackedFileTransferStatus::Transferring);
        assert_eq!(t.updated_at_ms, 2000);
    }

    #[tokio::test]
    async fn test_refresh_activity_updates_timestamp() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![PendingInboundTransfer {
            transfer_id: "t1".to_string(),
            entry_id: "e1".to_string(),
            origin_device_id: "d1".to_string(),
            filename: "file.bin".to_string(),
            cached_path: "/cache/t1/file.bin".to_string(),
            created_at_ms: 1000,
        }];

        uc.record_pending_from_clipboard(transfers).await.unwrap();
        uc.mark_transferring("t1", 2000).await.unwrap();
        uc.refresh_activity("t1", 5000).await.unwrap();

        let t = repo.find("t1").unwrap();
        assert_eq!(t.updated_at_ms, 5000);
        assert_eq!(t.status, TrackedFileTransferStatus::Transferring);
    }

    #[tokio::test]
    async fn test_mark_completed() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![PendingInboundTransfer {
            transfer_id: "t1".to_string(),
            entry_id: "e1".to_string(),
            origin_device_id: "d1".to_string(),
            filename: "doc.pdf".to_string(),
            cached_path: "/cache/t1/doc.pdf".to_string(),
            created_at_ms: 1000,
        }];

        uc.record_pending_from_clipboard(transfers).await.unwrap();
        uc.mark_completed("t1", Some("abc123"), 3000).await.unwrap();

        let t = repo.find("t1").unwrap();
        assert_eq!(t.status, TrackedFileTransferStatus::Completed);
        assert_eq!(t.content_hash, Some("abc123".to_string()));
    }

    #[tokio::test]
    async fn test_mark_failed() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![PendingInboundTransfer {
            transfer_id: "t1".to_string(),
            entry_id: "e1".to_string(),
            origin_device_id: "d1".to_string(),
            filename: "img.png".to_string(),
            cached_path: "/cache/t1/img.png".to_string(),
            created_at_ms: 1000,
        }];

        uc.record_pending_from_clipboard(transfers).await.unwrap();
        uc.mark_failed("t1", "hash mismatch", 2000).await.unwrap();

        let t = repo.find("t1").unwrap();
        assert_eq!(t.status, TrackedFileTransferStatus::Failed);
        assert_eq!(t.failure_reason, Some("hash mismatch".to_string()));
    }

    #[tokio::test]
    async fn test_list_expired_inflight() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![
            PendingInboundTransfer {
                transfer_id: "t_pending_old".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "a.txt".to_string(),
                cached_path: "/cache/t_pending_old/a.txt".to_string(),
                created_at_ms: 1000,
            },
            PendingInboundTransfer {
                transfer_id: "t_pending_new".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "b.txt".to_string(),
                cached_path: "/cache/t_pending_new/b.txt".to_string(),
                created_at_ms: 100_000,
            },
        ];

        uc.record_pending_from_clipboard(transfers).await.unwrap();

        // now_ms = 100_000; pending timeout = 60_000
        // t_pending_old (updated_at=1000) should be expired
        // t_pending_new (updated_at=100_000) should NOT be expired
        let expired = uc.list_expired_inflight(100_000).await.unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].transfer_id, "t_pending_old");
    }

    #[tokio::test]
    async fn test_reconcile_inflight_after_startup() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![
            PendingInboundTransfer {
                transfer_id: "t1".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "f1.txt".to_string(),
                cached_path: "/cache/t1/f1.txt".to_string(),
                created_at_ms: 1000,
            },
            PendingInboundTransfer {
                transfer_id: "t2".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "f2.txt".to_string(),
                cached_path: "/cache/t2/f2.txt".to_string(),
                created_at_ms: 1000,
            },
        ];

        uc.record_pending_from_clipboard(transfers).await.unwrap();
        uc.mark_transferring("t1", 2000).await.unwrap();
        uc.mark_completed("t2", None, 3000).await.unwrap();

        // t1 is transferring, t2 is completed
        // reconcile should only fail t1
        let cleanup = uc.reconcile_inflight_after_startup(5000).await.unwrap();
        assert_eq!(cleanup.len(), 1);
        assert_eq!(cleanup[0].transfer_id, "t1");

        let t1 = repo.find("t1").unwrap();
        assert_eq!(t1.status, TrackedFileTransferStatus::Failed);
    }

    #[tokio::test]
    async fn test_get_entry_summary_aggregate() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let transfers = vec![
            PendingInboundTransfer {
                transfer_id: "t1".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "a.txt".to_string(),
                cached_path: "/cache/t1/a.txt".to_string(),
                created_at_ms: 1000,
            },
            PendingInboundTransfer {
                transfer_id: "t2".to_string(),
                entry_id: "e1".to_string(),
                origin_device_id: "d1".to_string(),
                filename: "b.txt".to_string(),
                cached_path: "/cache/t2/b.txt".to_string(),
                created_at_ms: 1000,
            },
        ];

        uc.record_pending_from_clipboard(transfers).await.unwrap();
        uc.mark_completed("t1", None, 2000).await.unwrap();
        uc.mark_failed("t2", "timeout", 3000).await.unwrap();

        let summary = uc.get_entry_summary("e1").await.unwrap().unwrap();
        // failed outranks completed
        assert_eq!(summary.aggregate_status, TrackedFileTransferStatus::Failed);
        assert_eq!(summary.failure_reason, Some("timeout".to_string()));
        assert_eq!(summary.transfer_ids.len(), 2);
    }

    #[tokio::test]
    async fn test_get_entry_summary_returns_none_for_unknown_entry() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        let summary = uc.get_entry_summary("nonexistent").await.unwrap();
        assert!(summary.is_none());
    }

    #[tokio::test]
    async fn test_record_pending_empty_is_noop() {
        let repo = Arc::new(MockFileTransferRepo::new());
        let uc = TrackInboundTransfersUseCase::new(repo.clone());

        uc.record_pending_from_clipboard(vec![]).await.unwrap();
        assert!(repo.transfers.lock().unwrap().is_empty());
    }
}
