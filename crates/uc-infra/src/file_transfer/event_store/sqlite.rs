use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use diesel::prelude::*;
use diesel::sqlite::SqliteConnection;
use serde_json;

use crate::db::ports::DbExecutor;
use crate::db::schema::file_transfer_events;
use uc_core::file_transfer::{FileTransferEvent, FileTransferEventStorePort};

#[allow(dead_code)]
#[derive(Debug, Clone, Queryable, Selectable)]
#[diesel(table_name = file_transfer_events)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub(crate) struct FileTransferEventRow {
    id: i32,
    transfer_id: String,
    sequence: i32,
    event_type: String,
    payload_json: String,
    occurred_at_ms: i64,
}

#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = file_transfer_events)]
pub(crate) struct NewFileTransferEventRow {
    transfer_id: String,
    sequence: i32,
    event_type: String,
    payload_json: String,
    occurred_at_ms: i64,
}

/// SQLite-backed event store for file transfer lifecycle events.
pub struct SqliteFileTransferEventStore<E> {
    executor: E,
}

impl<E> SqliteFileTransferEventStore<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E: DbExecutor> FileTransferEventStorePort for SqliteFileTransferEventStore<E> {
    async fn load(&self, transfer_id: &str) -> Result<Vec<FileTransferEvent>> {
        let transfer_id = transfer_id.to_string();

        self.executor
            .run(move |conn| load_events(conn, &transfer_id))
    }

    async fn append(&self, event: FileTransferEvent) -> Result<()> {
        self.executor.run(move |conn| append_event(conn, event))
    }
}

pub(crate) fn load_events(
    conn: &mut SqliteConnection,
    transfer_id: &str,
) -> Result<Vec<FileTransferEvent>> {
    let rows = file_transfer_events::table
        .filter(file_transfer_events::transfer_id.eq(transfer_id))
        .order(file_transfer_events::sequence.asc())
        .load::<FileTransferEventRow>(conn)
        .with_context(|| format!("failed to load file transfer events for `{transfer_id}`"))?;

    rows.into_iter()
        .map(|row| {
            let event: FileTransferEvent =
                serde_json::from_str(&row.payload_json).with_context(|| {
                    format!(
                        "failed to deserialize file transfer event `{}` for `{}` at sequence {}",
                        row.event_type, row.transfer_id, row.sequence
                    )
                })?;

            Ok(event)
        })
        .collect()
}

pub(crate) fn append_event(conn: &mut SqliteConnection, event: FileTransferEvent) -> Result<()> {
    let transfer_id = transfer_id_of(&event).to_string();
    let event_type = event_type_of(&event).to_string();
    let payload_json = serde_json::to_string(&event)
        .with_context(|| format!("failed to serialize file transfer event `{event_type}`"))?;
    let occurred_at_ms = Utc::now().timestamp_millis();

    let current_max: Option<i32> = file_transfer_events::table
        .filter(file_transfer_events::transfer_id.eq(&transfer_id))
        .select(diesel::dsl::max(file_transfer_events::sequence))
        .first(conn)
        .with_context(|| {
            format!("failed to read event sequence for file transfer `{transfer_id}`")
        })?;

    let sequence = current_max.unwrap_or(0) + 1;

    let row = NewFileTransferEventRow {
        transfer_id: transfer_id.clone(),
        sequence,
        event_type,
        payload_json,
        occurred_at_ms,
    };

    diesel::insert_into(file_transfer_events::table)
        .values(&row)
        .execute(conn)
        .with_context(|| {
            format!(
                "failed to append file transfer event for `{}` at sequence {}",
                transfer_id, sequence
            )
        })?;

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

fn event_type_of(event: &FileTransferEvent) -> &'static str {
    match event {
        FileTransferEvent::Started { .. } => "started",
        FileTransferEvent::Progress { .. } => "progress",
        FileTransferEvent::Completed { .. } => "completed",
        FileTransferEvent::Failed { .. } => "failed",
        FileTransferEvent::Cancelled { .. } => "cancelled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use tempfile::{tempdir, TempDir};
    use uc_core::{FileTransferCancellationReason, FileTransferDirection, FileTransferProgress};

    fn make_store() -> (SqliteFileTransferEventStore<DieselSqliteExecutor>, TempDir) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("file-transfer-events.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        (
            SqliteFileTransferEventStore::new(DieselSqliteExecutor::new(pool)),
            tempdir,
        )
    }

    #[tokio::test]
    async fn append_and_load_returns_events_in_sequence_order() {
        let (store, _tempdir) = make_store();
        let started = FileTransferEvent::started("transfer-1", "peer-1", "report.pdf", Some(128));
        let progress = FileTransferEvent::Progress {
            transfer_id: "transfer-1".into(),
            peer_id: "peer-1".into(),
            progress: FileTransferProgress {
                direction: FileTransferDirection::Receiving,
                bytes_transferred: 96,
                total_bytes: Some(128),
            },
        };

        store.append(started.clone()).await.unwrap();
        store.append(progress.clone()).await.unwrap();

        assert_eq!(
            store.load("transfer-1").await.unwrap(),
            vec![started, progress]
        );
    }

    #[tokio::test]
    async fn load_only_returns_events_for_requested_transfer() {
        let (store, _tempdir) = make_store();
        let first = FileTransferEvent::completed("transfer-1", "peer-1");
        let second = FileTransferEvent::cancelled(
            "transfer-2",
            "peer-2",
            FileTransferCancellationReason::RemotePeer,
        );

        store.append(first.clone()).await.unwrap();
        store.append(second).await.unwrap();

        assert_eq!(store.load("transfer-1").await.unwrap(), vec![first]);
        assert!(store.load("missing").await.unwrap().is_empty());
    }
}
