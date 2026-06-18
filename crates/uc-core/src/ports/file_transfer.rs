//! Receiver-side file-transfer projection ports.
//!
//! The receiver maintains a local projection of inbound file transfers. These
//! intent ports expose only the slices the application layer actually depends
//! on, split by responsibility direction (query vs command) so each consumer
//! holds the minimal capability it needs.

// Types use String for transfer_id / entry_id to keep the receiver projection
// DTOs decoupled from id value objects across crate boundaries.

use async_trait::async_trait;

/// Durable status of a tracked inbound file transfer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackedFileTransferStatus {
    /// Metadata received, waiting for blob transfer to start.
    Pending,
    /// First data chunk received, blob transfer in progress.
    Transferring,
    /// All chunks received, hash verified, file ready.
    Completed,
    /// Transfer failed (hash mismatch, network error, or orphaned on restart).
    Failed,
    /// Transfer was cancelled (local user action, remote peer cancel,
    /// inactivity timeout, replaced by newer content). Distinguished from
    /// `Failed` so UI can render a neutral "cancelled" state instead of an
    /// error indication. Sub-reason lives in the accompanying `reason` field.
    Cancelled,
}

impl TrackedFileTransferStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Transferring => "transferring",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parse from stored string representation.
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "transferring" => Some(Self::Transferring),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            "cancelled" => Some(Self::Cancelled),
            _ => None,
        }
    }
}

impl std::fmt::Display for TrackedFileTransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Input for seeding a pending transfer record from clipboard metadata.
#[derive(Debug, Clone)]
pub struct PendingInboundTransfer {
    pub transfer_id: String,
    pub entry_id: String,
    pub origin_device_id: String,
    pub filename: String,
    pub cached_path: String,
    pub created_at_ms: i64,
}

/// Aggregate transfer status for a clipboard entry.
///
/// Aggregation rule:
/// - any failed => `Failed`
/// - else any transferring => `Transferring`
/// - else any pending => `Pending`
/// - else all completed => `Completed`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryTransferSummary {
    pub entry_id: String,
    pub aggregate_status: TrackedFileTransferStatus,
    /// Human-readable reason when aggregate is `Failed`.
    pub failure_reason: Option<String>,
    /// Transfer IDs belonging to this entry.
    pub transfer_ids: Vec<String>,
}

/// Expired in-flight record with cleanup target.
#[derive(Debug, Clone)]
pub struct ExpiredInflightTransfer {
    pub transfer_id: String,
    pub entry_id: String,
    pub cached_path: String,
    pub status: TrackedFileTransferStatus,
}

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

/// Compute aggregate status from a list of individual transfer statuses.
///
/// Rule: failed > transferring > pending > cancelled > completed.
///
/// `Cancelled` 排在 `Completed` 之前是因为:聚合视图里只要有任何一个
/// transfer 被取消,整条 entry 就不是"全部成功"的语义。但 `Cancelled`
/// 又低于 `Failed` —— 真失败比"用户放弃"更需要被看到。
pub fn compute_aggregate_status(
    statuses: &[TrackedFileTransferStatus],
) -> Option<TrackedFileTransferStatus> {
    if statuses.is_empty() {
        return None;
    }

    if statuses.contains(&TrackedFileTransferStatus::Failed) {
        return Some(TrackedFileTransferStatus::Failed);
    }
    if statuses.contains(&TrackedFileTransferStatus::Transferring) {
        return Some(TrackedFileTransferStatus::Transferring);
    }
    if statuses.contains(&TrackedFileTransferStatus::Pending) {
        return Some(TrackedFileTransferStatus::Pending);
    }
    if statuses.contains(&TrackedFileTransferStatus::Cancelled) {
        return Some(TrackedFileTransferStatus::Cancelled);
    }
    Some(TrackedFileTransferStatus::Completed)
}
