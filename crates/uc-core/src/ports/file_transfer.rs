//! Receiver-side file-transfer projection ports.
//!
//! The receiver maintains a local projection of inbound file transfers. These
//! intent ports expose only the slices the application layer actually depends
//! on, split by responsibility direction (query vs command) so each consumer
//! holds the minimal capability it needs.

use async_trait::async_trait;

use super::file_transfer_repository::{
    EntryTransferSummary, ExpiredInflightTransfer, PendingInboundTransfer,
};

/// Failure of a receiver-side file-transfer projection operation.
#[derive(Debug, thiserror::Error)]
pub enum FileTransferProjectionError {
    /// The underlying projection store failed (I/O, database, serialization).
    #[error("file-transfer projection store error: {0}")]
    Backend(String),
}

/// Command: write receiver-side projection rows.
#[async_trait]
pub trait RecordReceiverTransferPort: Send + Sync {
    /// Upsert a single pending transfer record.
    ///
    /// If no row exists for `transfer.transfer_id`, a fresh `pending` row is
    /// inserted. If a row already exists, the mutable seed fields (`entry_id`,
    /// `filename`, `origin_device_id`, `cached_path`) are overwritten; status,
    /// timestamps, file_size and content_hash are left untouched.
    ///
    /// Idempotent — calling it twice with the same input is equivalent to
    /// calling it once.
    async fn upsert_pending_transfer(
        &self,
        transfer: &PendingInboundTransfer,
    ) -> Result<(), FileTransferProjectionError>;

    /// Re-associate a transfer with a different `entry_id`.
    ///
    /// The new association replaces any prior `entry_id` recorded for the
    /// transfer. Idempotent when the new value equals the existing one.
    ///
    /// Returns `true` if a row was updated, `false` if no matching
    /// transfer_id exists.
    async fn link_transfer_to_entry(
        &self,
        transfer_id: &str,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<bool, FileTransferProjectionError>;
}

/// Query: aggregate transfer status for a clipboard entry.
#[async_trait]
pub trait GetEntryTransferSummaryPort: Send + Sync {
    /// Compute the aggregate transfer status for an entry. Returns `None` when
    /// the entry has no tracked transfers.
    async fn get_entry_transfer_summary(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryTransferSummary>, FileTransferProjectionError>;
}

/// Query: resolve the entry a transfer belongs to.
#[async_trait]
pub trait FindEntryIdForTransferPort: Send + Sync {
    /// Return the `entry_id` recorded for a transfer, or `None` when no
    /// projection row exists for the given transfer_id.
    async fn get_entry_id_for_transfer(
        &self,
        transfer_id: &str,
    ) -> Result<Option<String>, FileTransferProjectionError>;
}

/// Query: list in-flight transfers that have exceeded their deadlines.
#[async_trait]
pub trait ListExpiredInflightTransfersPort: Send + Sync {
    /// List in-flight transfers past their deadline:
    /// - status `pending` and `updated_at_ms < pending_cutoff_ms`
    /// - status `transferring` and `updated_at_ms < transferring_cutoff_ms`
    async fn list_expired_inflight(
        &self,
        pending_cutoff_ms: i64,
        transferring_cutoff_ms: i64,
    ) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError>;
}

/// Command: finalize in-flight transfers as failed.
#[async_trait]
pub trait FailInflightTransfersPort: Send + Sync {
    /// Mark a single transfer as `failed` with a reason.
    async fn mark_failed(
        &self,
        transfer_id: &str,
        reason: &str,
        now_ms: i64,
    ) -> Result<(), FileTransferProjectionError>;

    /// Bulk-mark all in-flight rows (pending/transferring) as failed.
    /// Returns cleanup targets (cached_path, etc.) for platform code to delete.
    async fn bulk_fail_inflight(
        &self,
        reason: &str,
        now_ms: i64,
    ) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError>;
}
