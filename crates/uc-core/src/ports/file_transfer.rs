//! Receiver-side file-transfer projection ports.
//!
//! The receiver maintains a local projection of inbound file transfers. These
//! intent ports expose only the slices the application layer actually depends
//! on, split by responsibility direction (query vs command) so each consumer
//! holds the minimal capability it needs.

// Types use String for transfer_id / entry_id to keep the receiver projection
// DTOs decoupled from id value objects across crate boundaries.

use async_trait::async_trait;

/// Failure while completing the one-shot privacy maintenance required after
/// migrating receiver transfer persistence from plaintext to ciphertext.
#[derive(Debug, thiserror::Error)]
pub enum FileTransferPrivacyMaintenanceError {
    #[error("file transfer privacy maintenance failed: {0}")]
    Backend(String),
}

/// Ensure dropped plaintext transfer data has been physically removed before
/// any receiver worker is allowed to read or write transfer state.
///
/// Implementations must be idempotent. They may mark maintenance complete only
/// after the WAL is truncated and the database has been compacted successfully.
#[async_trait]
pub trait EnsureFileTransferPrivacyMaintenancePort: Send + Sync {
    async fn ensure_file_transfer_privacy_maintenance(
        &self,
    ) -> Result<(), FileTransferPrivacyMaintenanceError>;
}

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
    pub attempt_id: Option<String>,
    pub origin_device_id: String,
    pub filename: String,
    pub file_size: Option<i64>,
    pub cached_path: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone)]
pub struct ProvisionalInboundTransfer {
    pub transfer_id: String,
    pub origin_device_id: String,
    pub filename: String,
    pub file_size: Option<i64>,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvisionalReceiveRecovery {
    pub transfer_id: String,
    pub cached_path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiveItemRole {
    Representation,
    File,
}

impl ReceiveItemRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Representation => "representation",
            Self::File => "file",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProvisionalReceiveAction {
    AdoptIntoAttempt {
        entry_id: String,
        attempt_id: String,
        item_id: String,
        role: ReceiveItemRole,
    },
    DiscardAsFullyHeld,
}

#[derive(Debug, thiserror::Error)]
pub enum ProvisionalReceiveError {
    #[error("provisional receive does not exist")]
    NotFound,
    #[error("provisional receive is no longer claimable")]
    Conflict,
    #[error("provisional receive store error: {0}")]
    Backend(String),
}

#[async_trait]
pub trait SeedProvisionalReceivePort: Send + Sync {
    async fn seed_provisional_receive(
        &self,
        transfer: &ProvisionalInboundTransfer,
    ) -> Result<(), ProvisionalReceiveError>;
}

#[async_trait]
pub trait UpdateProvisionalReceivePathPort: Send + Sync {
    async fn update_provisional_receive_path(
        &self,
        provisional_transfer_id: &str,
        cached_path: &str,
        now_ms: i64,
    ) -> Result<(), ProvisionalReceiveError>;
}

#[async_trait]
pub trait ListProvisionalReceivesPort: Send + Sync {
    async fn list_provisional_receives(
        &self,
    ) -> Result<Vec<ProvisionalReceiveRecovery>, ProvisionalReceiveError>;
}

#[async_trait]
pub trait FinalizeProvisionalReceivePort: Send + Sync {
    async fn finalize_provisional_receive(
        &self,
        provisional_transfer_id: &str,
        action: ProvisionalReceiveAction,
        now_ms: i64,
    ) -> Result<(), ProvisionalReceiveError>;
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
    /// Transfer IDs belonging to this entry, sorted for deterministic reads.
    pub transfer_ids: Vec<String>,
}

/// Current aggregate progress for any remote inbound receive attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryReceiveProgress {
    pub entry_id: String,
    pub attempt_id: String,
    pub state: crate::ports::entry_receive_attempt::AttemptState,
    pub total_bytes: i64,
    pub completed_bytes: i64,
    pub items_total: u32,
    pub items_completed: u32,
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
    /// `attempt_id`, `filename`, `origin_device_id`, `file_size`, `cached_path`)
    /// are overwritten; status, timestamps, and content_hash are left untouched.
    ///
    /// Idempotent — calling it twice with the same input is equivalent to
    /// calling it once.
    async fn upsert_pending_transfer(
        &self,
        transfer: &PendingInboundTransfer,
    ) -> Result<(), FileTransferProjectionError>;
}

/// Command: cancel every receiver transfer owned by one directory attempt.
#[async_trait]
pub trait CancelDirectoryAttemptTransfersPort: Send + Sync {
    /// Move every non-terminal member of the exact `(entry_id, attempt_id)`
    /// pair to `Cancelled` and return the number of rows changed.
    async fn cancel_attempt_transfers(
        &self,
        entry_id: &str,
        attempt_id: &str,
        reason: crate::file_transfer::FileTransferCancellationReason,
        now_ms: i64,
    ) -> Result<u32, FileTransferProjectionError>;
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

/// Query aggregate progress for the current remote inbound receive attempt.
#[async_trait]
pub trait GetEntryReceiveProgressPort: Send + Sync {
    async fn get_entry_receive_progress(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryReceiveProgress>, FileTransferProjectionError>;
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

#[async_trait]
pub trait FindAttemptIdForTransferPort: Send + Sync {
    async fn get_attempt_id_for_transfer(
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
