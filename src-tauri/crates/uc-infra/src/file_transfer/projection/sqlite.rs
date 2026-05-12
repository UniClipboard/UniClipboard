use anyhow::Result;
use chrono::Utc;
use diesel::prelude::*;
use tracing::debug;

use crate::db::schema::file_transfer;
use uc_core::file_transfer::{
    FileTransferCancellationReason, FileTransferEvent, FileTransferFailureReason,
};
use uc_core::ports::file_transfer_repository::TrackedFileTransferStatus;

pub(crate) fn apply_event(
    conn: &mut diesel::sqlite::SqliteConnection,
    event: &FileTransferEvent,
) -> Result<()> {
    let now_ms = Utc::now().timestamp_millis();
    let transfer_id = transfer_id_of(event).to_string();

    let affected = match event {
        FileTransferEvent::Started {
            transfer_id,
            filename,
            file_size,
            ..
        } => {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Transferring.as_str()),
                    file_transfer::filename.eq(filename.as_str()),
                    file_transfer::file_size.eq(file_size.map(u64_to_i64)),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?
        }
        FileTransferEvent::Progress { transfer_id, .. } => {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Transferring.as_str()),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?
        }
        FileTransferEvent::Completed { transfer_id, .. } => {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?
        }
        FileTransferEvent::Failed {
            transfer_id,
            reason,
            ..
        } => {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                    file_transfer::failure_reason.eq(Some(failure_reason_of(*reason))),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?
        }
        FileTransferEvent::Cancelled {
            transfer_id,
            reason,
            ..
        } => {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(transfer_id)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                    file_transfer::failure_reason.eq(Some(cancellation_reason_of(*reason))),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?
        }
    };

    if affected == 0 {
        // No projection row — expected for sender-side transfers, which do not
        // seed a receiver context. The event is still recorded in the event log
        // (the transaction wrapping this call handles both); only the
        // receiver-specific projection update is a no-op.
        debug!(
            transfer_id,
            "no receiver projection row for event; skipping projection update (sender-side or pre-seed)"
        );
    }

    Ok(())
}

fn transfer_id_of(event: &FileTransferEvent) -> &str {
    match event {
        FileTransferEvent::Started { transfer_id, .. }
        | FileTransferEvent::Progress { transfer_id, .. }
        | FileTransferEvent::Completed { transfer_id, .. }
        | FileTransferEvent::Failed { transfer_id, .. }
        | FileTransferEvent::Cancelled { transfer_id, .. } => transfer_id,
    }
}

fn u64_to_i64(value: u64) -> i64 {
    value.min(i64::MAX as u64) as i64
}

fn failure_reason_of(reason: FileTransferFailureReason) -> &'static str {
    match reason {
        FileTransferFailureReason::NetworkUnavailable => "network_unavailable",
        FileTransferFailureReason::TimedOut => "timed_out",
        FileTransferFailureReason::AccessDenied => "access_denied",
        FileTransferFailureReason::StorageUnavailable => "storage_unavailable",
        FileTransferFailureReason::IntegrityCheckFailed => "integrity_check_failed",
        FileTransferFailureReason::Unknown => "unknown",
    }
}

fn cancellation_reason_of(reason: FileTransferCancellationReason) -> &'static str {
    match reason {
        FileTransferCancellationReason::LocalUser => "cancelled:local_user",
        FileTransferCancellationReason::RemotePeer => "cancelled:remote_peer",
        FileTransferCancellationReason::Replaced => "cancelled:replaced",
        FileTransferCancellationReason::Unknown => "cancelled:unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use crate::db::repositories::DieselFileTransferRepository;
    use crate::file_transfer::receiver_store::SqliteReceiverFileTransferStore;
    use tempfile::{tempdir, TempDir};
    use uc_core::file_transfer::{FileTransferEventStorePort, FileTransferProgress};
    use uc_core::ports::file_transfer_repository::PendingInboundTransfer;
    use uc_core::ports::FileTransferRepositoryPort;
    use uc_core::FileTransferDirection;

    fn make_setup() -> (
        SqliteReceiverFileTransferStore<DieselSqliteExecutor>,
        DieselFileTransferRepository<DieselSqliteExecutor>,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("file-transfer-projection.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let store = SqliteReceiverFileTransferStore::new(DieselSqliteExecutor::new(pool.clone()));
        let repo = DieselFileTransferRepository::new(DieselSqliteExecutor::new(pool));

        (store, repo, tempdir)
    }

    fn pending_transfer() -> PendingInboundTransfer {
        PendingInboundTransfer {
            transfer_id: "transfer-1".into(),
            entry_id: "entry-1".into(),
            origin_device_id: "device-1".into(),
            filename: "report.pdf".into(),
            cached_path: "/tmp/report.pdf".into(),
            created_at_ms: 10,
        }
    }

    #[tokio::test]
    async fn upsert_pending_transfer_creates_pending_projection_row() {
        let (_store, repo, _tempdir) = make_setup();

        repo.upsert_pending_transfer(&pending_transfer())
            .await
            .unwrap();

        let transfers = repo.list_transfers_for_entry("entry-1").await.unwrap();
        assert_eq!(transfers.len(), 1);
        let transfer = &transfers[0];
        assert_eq!(transfer.transfer_id, "transfer-1");
        assert_eq!(transfer.origin_device_id, "device-1");
        assert_eq!(transfer.status, TrackedFileTransferStatus::Pending);
        assert_eq!(transfer.cached_path, "/tmp/report.pdf");
        assert_eq!(transfer.file_size, None);
    }

    #[tokio::test]
    async fn apply_event_projects_started_and_completed_states() {
        let (store, repo, _tempdir) = make_setup();
        repo.upsert_pending_transfer(&pending_transfer())
            .await
            .unwrap();

        store
            .append(FileTransferEvent::started(
                "transfer-1",
                "peer-1",
                "report.pdf",
                Some(128),
            ))
            .await
            .unwrap();
        store
            .append(FileTransferEvent::completed("transfer-1", "peer-1"))
            .await
            .unwrap();

        let transfers = repo.list_transfers_for_entry("entry-1").await.unwrap();
        let transfer = &transfers[0];
        assert_eq!(transfer.status, TrackedFileTransferStatus::Completed);
        assert_eq!(transfer.file_size, Some(128));
    }

    #[tokio::test]
    async fn progress_and_cancelled_events_update_projection_as_expected() {
        let (store, repo, _tempdir) = make_setup();
        repo.upsert_pending_transfer(&pending_transfer())
            .await
            .unwrap();

        store
            .append(FileTransferEvent::Progress {
                transfer_id: "transfer-1".into(),
                peer_id: "peer-1".into(),
                progress: FileTransferProgress {
                    direction: FileTransferDirection::Receiving,
                    bytes_transferred: 64,
                    total_bytes: Some(128),
                },
            })
            .await
            .unwrap();
        store
            .append(FileTransferEvent::cancelled(
                "transfer-1",
                "peer-1",
                FileTransferCancellationReason::RemotePeer,
            ))
            .await
            .unwrap();

        let transfers = repo.list_transfers_for_entry("entry-1").await.unwrap();
        let transfer = &transfers[0];
        assert_eq!(transfer.status, TrackedFileTransferStatus::Failed);
        assert_eq!(
            transfer.failure_reason.as_deref(),
            Some("cancelled:remote_peer")
        );
    }

    #[tokio::test]
    async fn apply_event_is_noop_when_no_receiver_row() {
        // Sender-side transfers legitimately flow through this projection
        // updater without ever seeding a receiver context. The update must
        // silently no-op rather than erroring, otherwise sender-side event
        // append would fail.
        let (store, _repo, _tempdir) = make_setup();

        store
            .append(FileTransferEvent::completed(
                "sender-only-transfer",
                "peer-1",
            ))
            .await
            .unwrap();
    }
}
