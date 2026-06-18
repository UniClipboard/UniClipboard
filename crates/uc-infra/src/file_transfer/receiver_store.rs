use anyhow::Result;
use async_trait::async_trait;
use diesel::Connection;

use crate::db::ports::DbExecutor;
use crate::file_transfer::event_store::sqlite::{append_event, load_events};
use crate::file_transfer::projection::sqlite::apply_event;
use uc_core::file_transfer::{FileTransferEvent, FileTransferEventStorePort};

/// Receiver-side durable store that keeps event log and projection updates in one SQLite transaction.
pub struct SqliteReceiverFileTransferStore<E> {
    executor: E,
}

impl<E> SqliteReceiverFileTransferStore<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E: DbExecutor> FileTransferEventStorePort for SqliteReceiverFileTransferStore<E> {
    async fn load(&self, transfer_id: &str) -> Result<Vec<FileTransferEvent>> {
        let transfer_id = transfer_id.to_string();
        self.executor
            .run(move |conn| load_events(conn, &transfer_id))
    }

    async fn append(&self, event: FileTransferEvent) -> Result<()> {
        self.executor.run(move |conn| {
            conn.transaction::<_, anyhow::Error, _>(|conn| {
                append_event(conn, event.clone())?;
                apply_event(conn, &event)?;
                Ok(())
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::models::FileTransferRow;
    use crate::db::pool::init_db_pool;
    use crate::db::ports::DbExecutor;
    use crate::db::repositories::DieselFileTransferRepository;
    use crate::db::schema::file_transfer;
    use diesel::prelude::*;
    use tempfile::{tempdir, TempDir};
    use uc_core::ports::file_transfer::{PendingInboundTransfer, TrackedFileTransferStatus};
    use uc_core::ports::RecordReceiverTransferPort;
    use uc_core::{FileTransferDirection, FileTransferProgress};

    fn make_store() -> (
        SqliteReceiverFileTransferStore<DieselSqliteExecutor>,
        DieselFileTransferRepository<DieselSqliteExecutor>,
        DieselSqliteExecutor,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("receiver-file-transfer-store.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let store = SqliteReceiverFileTransferStore::new(DieselSqliteExecutor::new(pool.clone()));
        let repo = DieselFileTransferRepository::new(DieselSqliteExecutor::new(pool.clone()));
        let reader = DieselSqliteExecutor::new(pool);

        (store, repo, reader, tempdir)
    }

    // Read projection rows back directly via the schema — the receiver
    // projection is verified at the infra layer, not through a domain port.
    fn load_rows(reader: &DieselSqliteExecutor, entry_id: &str) -> Vec<FileTransferRow> {
        let eid = entry_id.to_string();
        reader
            .run(move |conn| {
                Ok(file_transfer::table
                    .filter(file_transfer::entry_id.eq(&eid))
                    .load::<FileTransferRow>(conn)?)
            })
            .unwrap()
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
    async fn append_event_and_project_updates_both_event_log_and_projection() {
        let (store, repo, reader, _tempdir) = make_store();
        repo.upsert_pending_transfer(&pending_transfer())
            .await
            .unwrap();

        let started = FileTransferEvent::started("transfer-1", "peer-1", "report.pdf", Some(128));
        let progress = FileTransferEvent::Progress {
            transfer_id: "transfer-1".into(),
            peer_id: "peer-1".into(),
            progress: FileTransferProgress {
                direction: FileTransferDirection::Receiving,
                bytes_transferred: 64,
                total_bytes: Some(128),
            },
        };

        store.append(started.clone()).await.unwrap();
        store.append(progress.clone()).await.unwrap();

        assert_eq!(
            store.load("transfer-1").await.unwrap(),
            vec![started, progress]
        );

        let rows = load_rows(&reader, "entry-1");
        let row = &rows[0];
        assert_eq!(row.file_size, Some(128));
        assert_eq!(row.status, TrackedFileTransferStatus::Transferring.as_str());
    }

    #[tokio::test]
    async fn append_succeeds_without_receiver_context_for_sender_side_events() {
        // Sender-side transfers intentionally do not seed a receiver context.
        // The event log still records them; the receiver projection update is
        // simply a no-op when no row exists. This makes `store.append` safe to
        // call from both sides without the caller caring which one it is.
        let (store, _repo, _reader, _tempdir) = make_store();
        let event = FileTransferEvent::completed("sender-only-transfer", "peer-1");

        store.append(event.clone()).await.unwrap();

        assert_eq!(
            store.load("sender-only-transfer").await.unwrap(),
            vec![event]
        );
    }
}
