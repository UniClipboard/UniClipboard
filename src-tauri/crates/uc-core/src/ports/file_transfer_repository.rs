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
    /// Transfer failed (timeout, hash mismatch, network error, or orphaned on restart).
    Failed,
}

impl TrackedFileTransferStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Transferring => "transferring",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }

    /// Parse from stored string representation.
    pub fn from_str_value(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(Self::Pending),
            "transferring" => Some(Self::Transferring),
            "completed" => Some(Self::Completed),
            "failed" => Some(Self::Failed),
            _ => None,
        }
    }
}

impl std::fmt::Display for TrackedFileTransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A tracked inbound file transfer record.
#[derive(Debug, Clone)]
pub struct TrackedFileTransfer {
    pub transfer_id: String,
    pub entry_id: String,
    pub origin_device_id: String,
    pub filename: String,
    pub cached_path: String,
    pub status: TrackedFileTransferStatus,
    pub failure_reason: Option<String>,
    /// Nullable until announce metadata fills it in.
    pub file_size: Option<i64>,
    /// Nullable until completion with hash verification.
    pub content_hash: Option<String>,
    /// Epoch milliseconds of the last durable activity.
    pub updated_at_ms: i64,
    pub created_at_ms: i64,
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
    /// Seed pending records from clipboard metadata.
    async fn insert_pending_transfers(
        &self,
        transfers: &[PendingInboundTransfer],
    ) -> anyhow::Result<()>;

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

    /// Backfill announce metadata (file_size, content_hash) when available later.
    async fn backfill_announce_metadata(
        &self,
        transfer_id: &str,
        file_size: i64,
        content_hash: &str,
    ) -> anyhow::Result<()>;

    /// Mark a transfer as `transferring` (first data chunk received).
    async fn mark_transferring(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<bool>;

    /// Refresh in-flight liveness without changing semantic status.
    async fn refresh_activity(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<()>;

    /// Mark a transfer as `completed`.
    ///
    /// Returns `true` if a row was actually updated, `false` if no matching
    /// row existed (e.g., the pending record hasn't been seeded yet).
    async fn mark_completed(
        &self,
        transfer_id: &str,
        content_hash: Option<&str>,
        now_ms: i64,
    ) -> anyhow::Result<bool>;

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

    /// List transfers for an entry.
    async fn list_transfers_for_entry(
        &self,
        entry_id: &str,
    ) -> anyhow::Result<Vec<TrackedFileTransfer>>;

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

    /// Look up the full projection row for a transfer_id.
    ///
    /// Used by receiver-side workers that need the locally-recorded
    /// `cached_path` (and other context) once the domain timeline reports
    /// completion. Returns `None` when no receiver projection row exists
    /// for the given id — e.g. sender-side transfers.
    async fn get_transfer(&self, transfer_id: &str) -> anyhow::Result<Option<TrackedFileTransfer>>;
}

/// No-op stub for `FileTransferRepositoryPort` used at construction sites
/// before the real Diesel adapter is wired in (Plan 02).
pub struct NoopFileTransferRepositoryPort;

#[async_trait::async_trait]
impl FileTransferRepositoryPort for NoopFileTransferRepositoryPort {
    async fn insert_pending_transfers(&self, _: &[PendingInboundTransfer]) -> anyhow::Result<()> {
        Ok(())
    }
    async fn upsert_pending_transfer(&self, _: &PendingInboundTransfer) -> anyhow::Result<()> {
        Ok(())
    }
    async fn backfill_announce_metadata(&self, _: &str, _: i64, _: &str) -> anyhow::Result<()> {
        Ok(())
    }
    async fn mark_transferring(&self, _: &str, _: i64) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn refresh_activity(&self, _: &str, _: i64) -> anyhow::Result<()> {
        Ok(())
    }
    async fn mark_completed(&self, _: &str, _: Option<&str>, _: i64) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn mark_failed(&self, _: &str, _: &str, _: i64) -> anyhow::Result<()> {
        Ok(())
    }
    async fn list_expired_inflight(
        &self,
        _: i64,
        _: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>> {
        Ok(vec![])
    }
    async fn bulk_fail_inflight(
        &self,
        _: &str,
        _: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>> {
        Ok(vec![])
    }
    async fn get_entry_transfer_summary(
        &self,
        _: &str,
    ) -> anyhow::Result<Option<EntryTransferSummary>> {
        Ok(None)
    }
    async fn list_transfers_for_entry(&self, _: &str) -> anyhow::Result<Vec<TrackedFileTransfer>> {
        Ok(vec![])
    }
    async fn get_entry_id_for_transfer(&self, _: &str) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    async fn get_transfer(&self, _: &str) -> anyhow::Result<Option<TrackedFileTransfer>> {
        Ok(None)
    }
    async fn link_transfer_to_entry(&self, _: &str, _: &str, _: i64) -> anyhow::Result<bool> {
        Ok(false)
    }
}

/// Compute aggregate status from a list of individual transfer statuses.
///
/// Rule: failed > transferring > pending > completed.
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
    Some(TrackedFileTransferStatus::Completed)
}
