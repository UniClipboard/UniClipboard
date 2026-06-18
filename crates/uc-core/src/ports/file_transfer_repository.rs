//! Port for receiver-side file transfer tracking.
//!
//! Defines the hexagonal contract for persisting and querying
//! file transfer lifecycle state on the receiving device.

// Types use String for entry_id to avoid coupling to uc_ids
// across crate boundaries (the port is implemented in uc-infra).

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

/// Port for receiver-side file transfer tracking.
///
/// Implemented by the infrastructure layer (Diesel/SQLite).
/// Used by app-layer use cases for state transitions and projections.
#[async_trait::async_trait]
pub trait FileTransferRepositoryPort: Send + Sync {
    /// Upsert a single pending transfer record.
    ///
    /// If no row exists for `transfer.transfer_id`, a fresh `pending` row
    /// is inserted with `created_at_ms == updated_at_ms == transfer.created_at_ms`.
    /// If a row already exists, `entry_id`, `filename`, `origin_device_id`,
    /// and `cached_path` are overwritten with the supplied values; status,
    /// timestamps, file_size and content_hash are left untouched.
    ///
    /// Idempotent — calling it twice with the same input is equivalent to
    /// calling it once.
    async fn upsert_pending_transfer(
        &self,
        transfer: &PendingInboundTransfer,
    ) -> anyhow::Result<()>;

    /// Mark a transfer as `failed` with a reason.
    async fn mark_failed(&self, transfer_id: &str, reason: &str, now_ms: i64)
        -> anyhow::Result<()>;

    /// List expired in-flight transfers for timeout sweep.
    ///
    /// Returns rows where:
    /// - status is `pending` and `updated_at_ms < pending_cutoff_ms`
    /// - status is `transferring` and `updated_at_ms < transferring_cutoff_ms`
    async fn list_expired_inflight(
        &self,
        pending_cutoff_ms: i64,
        transferring_cutoff_ms: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>>;

    /// Bulk-mark stale in-flight rows (pending/transferring) as failed.
    /// Returns cleanup targets (cached_path, etc.) for platform code to delete.
    async fn bulk_fail_inflight(
        &self,
        reason: &str,
        now_ms: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>>;

    /// Compute aggregate transfer status for an entry.
    async fn get_entry_transfer_summary(
        &self,
        entry_id: &str,
    ) -> anyhow::Result<Option<EntryTransferSummary>>;

    /// Look up a single transfer by transfer_id.
    /// Returns the entry_id for the transfer, if found.
    async fn get_entry_id_for_transfer(&self, transfer_id: &str) -> anyhow::Result<Option<String>>;

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
    ) -> anyhow::Result<bool>;
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

    if statuses
        .iter()
        .any(|s| *s == TrackedFileTransferStatus::Failed)
    {
        return Some(TrackedFileTransferStatus::Failed);
    }
    if statuses
        .iter()
        .any(|s| *s == TrackedFileTransferStatus::Transferring)
    {
        return Some(TrackedFileTransferStatus::Transferring);
    }
    if statuses
        .iter()
        .any(|s| *s == TrackedFileTransferStatus::Pending)
    {
        return Some(TrackedFileTransferStatus::Pending);
    }
    if statuses
        .iter()
        .any(|s| *s == TrackedFileTransferStatus::Cancelled)
    {
        return Some(TrackedFileTransferStatus::Cancelled);
    }
    Some(TrackedFileTransferStatus::Completed)
}
