use anyhow::Result;
use chrono::Utc;
use diesel::prelude::*;
use tracing::debug;

use crate::db::schema::file_transfer;
use crate::file_transfer::persistence_cipher::{TransferMetadata, TransferPersistenceCipher};
use uc_core::file_transfer::{
    FileTransferCancellationReason, FileTransferEvent, FileTransferFailureReason,
};
use uc_core::ports::file_transfer::TrackedFileTransferStatus;

pub(crate) fn apply_event(
    conn: &mut diesel::sqlite::SqliteConnection,
    event: &FileTransferEvent,
    cipher: &TransferPersistenceCipher,
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
            let Some(mut metadata) = load_metadata(conn, transfer_id, cipher)? else {
                return Ok(());
            };
            metadata.filename = filename.clone();
            let metadata_ciphertext = cipher.seal_metadata(transfer_id, &metadata)?;
            diesel::update(
                file_transfer::table
                    .filter(file_transfer::transfer_id.eq(transfer_id))
                    .filter(file_transfer::status.eq_any([
                        TrackedFileTransferStatus::Pending.as_str(),
                        TrackedFileTransferStatus::Transferring.as_str(),
                    ])),
            )
            .set((
                file_transfer::status.eq(TrackedFileTransferStatus::Transferring.as_str()),
                file_transfer::file_size.eq(file_size.map(u64_to_i64)),
                file_transfer::metadata_ciphertext.eq(metadata_ciphertext),
                file_transfer::updated_at_ms.eq(now_ms),
            ))
            .execute(conn)?
        }
        FileTransferEvent::Progress { transfer_id, .. } => diesel::update(
            file_transfer::table
                .filter(file_transfer::transfer_id.eq(transfer_id))
                .filter(file_transfer::status.eq_any([
                    TrackedFileTransferStatus::Pending.as_str(),
                    TrackedFileTransferStatus::Transferring.as_str(),
                ])),
        )
        .set((
            file_transfer::status.eq(TrackedFileTransferStatus::Transferring.as_str()),
            file_transfer::updated_at_ms.eq(now_ms),
        ))
        .execute(conn)?,
        FileTransferEvent::Completed { transfer_id, .. } => diesel::update(
            file_transfer::table
                .filter(file_transfer::transfer_id.eq(transfer_id))
                .filter(file_transfer::status.eq_any([
                    TrackedFileTransferStatus::Pending.as_str(),
                    TrackedFileTransferStatus::Transferring.as_str(),
                    TrackedFileTransferStatus::Completed.as_str(),
                ])),
        )
        .set((
            file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()),
            file_transfer::updated_at_ms.eq(now_ms),
        ))
        .execute(conn)?,
        FileTransferEvent::Failed {
            transfer_id,
            reason,
            ..
        } => diesel::update(
            file_transfer::table
                .filter(file_transfer::transfer_id.eq(transfer_id))
                .filter(file_transfer::status.eq_any([
                    TrackedFileTransferStatus::Pending.as_str(),
                    TrackedFileTransferStatus::Transferring.as_str(),
                    TrackedFileTransferStatus::Failed.as_str(),
                ])),
        )
        .set((
            file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
            file_transfer::failure_code.eq(Some(failure_reason_of(*reason))),
            file_transfer::updated_at_ms.eq(now_ms),
        ))
        .execute(conn)?,
        FileTransferEvent::Cancelled {
            transfer_id,
            reason,
            ..
        } => diesel::update(
            file_transfer::table
                .filter(file_transfer::transfer_id.eq(transfer_id))
                .filter(file_transfer::status.eq_any([
                    TrackedFileTransferStatus::Pending.as_str(),
                    TrackedFileTransferStatus::Transferring.as_str(),
                    TrackedFileTransferStatus::Cancelled.as_str(),
                ])),
        )
        .set((
            file_transfer::status.eq(TrackedFileTransferStatus::Cancelled.as_str()),
            file_transfer::failure_code.eq(Some(cancellation_reason_of(*reason))),
            file_transfer::updated_at_ms.eq(now_ms),
        ))
        .execute(conn)?,
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

fn load_metadata(
    conn: &mut diesel::sqlite::SqliteConnection,
    transfer_id: &str,
    cipher: &TransferPersistenceCipher,
) -> Result<Option<TransferMetadata>> {
    let ciphertext = file_transfer::table
        .filter(file_transfer::transfer_id.eq(transfer_id))
        .select(file_transfer::metadata_ciphertext)
        .first::<Vec<u8>>(conn)
        .optional()?;
    ciphertext
        .map(|ciphertext| {
            cipher
                .open_metadata(transfer_id, &ciphertext)
                .map_err(Into::into)
        })
        .transpose()
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

/// 取消的子原因。reason 列与 status 列共同表达取消语义,status 已经
/// 是 `cancelled` 不再需要 `cancelled:` 前缀。
///
/// 历史数据兼容:0.7.x 之前同一字段塞 `failed + cancelled:local_user`,
/// 前端 resolver 仍 fallback 识别该前缀,不在此层做迁移。
fn cancellation_reason_of(reason: FileTransferCancellationReason) -> &'static str {
    reason.as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::models::FileTransferRow;
    use crate::db::pool::init_db_pool;
    use crate::db::pool::MIGRATIONS;
    use crate::db::ports::DbExecutor;
    use crate::db::repositories::DieselFileTransferRepository;
    use crate::file_transfer::receiver_store::SqliteReceiverFileTransferStore;
    use crate::file_transfer::SqliteFileTransferPrivacyMaintenance;
    use diesel_migrations::MigrationHarness;
    use tempfile::{tempdir, TempDir};
    use uc_core::file_transfer::{FileTransferEventStorePort, FileTransferProgress};
    use uc_core::ports::file_transfer::PendingInboundTransfer;
    use uc_core::ports::{EnsureFileTransferPrivacyMaintenancePort, RecordReceiverTransferPort};
    use uc_core::FileTransferDirection;

    fn make_setup() -> (
        SqliteReceiverFileTransferStore<DieselSqliteExecutor>,
        DieselFileTransferRepository<DieselSqliteExecutor>,
        DieselSqliteExecutor,
        TempDir,
    ) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("file-transfer-projection.sqlite");
        let pool = init_db_pool(database_url.to_str().unwrap()).unwrap();
        let (derive_subkey, current_profile) =
            crate::file_transfer::persistence_cipher::test_keys::ports();
        let store = SqliteReceiverFileTransferStore::new(
            DieselSqliteExecutor::new(pool.clone()),
            derive_subkey.clone(),
            current_profile.clone(),
        );
        let repo = DieselFileTransferRepository::new(
            DieselSqliteExecutor::new(pool.clone()),
            derive_subkey,
            current_profile,
        );
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
            attempt_id: None,
            origin_device_id: "device-1".into(),
            filename: "report.pdf".into(),
            file_size: None,
            cached_path: "/tmp/report.pdf".into(),
            created_at_ms: 10,
        }
    }

    async fn test_cipher() -> crate::file_transfer::persistence_cipher::TransferPersistenceCipher {
        let (derive_subkey, current_profile) =
            crate::file_transfer::persistence_cipher::test_keys::ports();
        crate::file_transfer::persistence_cipher::derive_transfer_persistence_cipher(
            &derive_subkey,
            &current_profile,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn upsert_pending_transfer_creates_pending_projection_row() {
        let (_store, repo, reader, _tempdir) = make_setup();

        repo.upsert_pending_transfer(&pending_transfer())
            .await
            .unwrap();

        let rows = load_rows(&reader, "entry-1");
        assert_eq!(rows.len(), 1);
        let row = &rows[0];
        assert_eq!(row.transfer_id, "transfer-1");
        assert_eq!(row.source_device, "device-1");
        assert_eq!(row.status, TrackedFileTransferStatus::Pending.as_str());
        let metadata = test_cipher()
            .await
            .open_metadata(&row.transfer_id, &row.metadata_ciphertext)
            .unwrap();
        assert_eq!(metadata.cached_path.as_deref(), Some("/tmp/report.pdf"));
        assert_eq!(row.file_size, None);
    }

    #[tokio::test]
    async fn apply_event_projects_started_and_completed_states() {
        let (store, repo, reader, _tempdir) = make_setup();
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

        let rows = load_rows(&reader, "entry-1");
        let row = &rows[0];
        assert_eq!(row.status, TrackedFileTransferStatus::Completed.as_str());
        assert_eq!(row.file_size, Some(128));
    }

    #[tokio::test]
    async fn progress_and_cancelled_events_update_projection_as_expected() {
        let (store, repo, reader, _tempdir) = make_setup();
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

        let rows = load_rows(&reader, "entry-1");
        let row = &rows[0];
        assert_eq!(row.status, TrackedFileTransferStatus::Cancelled.as_str());
        assert_eq!(row.failure_code.as_deref(), Some("remote_peer"));
    }

    #[tokio::test]
    async fn apply_event_is_noop_when_no_receiver_row() {
        // Sender-side transfers legitimately flow through this projection
        // updater without ever seeding a receiver context. The update must
        // silently no-op rather than erroring, otherwise sender-side event
        // append would fail.
        let (store, _repo, _reader, _tempdir) = make_setup();

        store
            .append(FileTransferEvent::completed(
                "sender-only-transfer",
                "peer-1",
            ))
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn transfer_user_metadata_is_not_present_in_sqlite_files() {
        const PRIVATE_NAME: &str = "private-transfer-name-7f3a9c.txt";
        const PRIVATE_PATH: &str = "/tmp/private-transfer-path-7f3a9c.txt";

        let (store, repo, _reader, tempdir) = make_setup();
        repo.upsert_pending_transfer(&PendingInboundTransfer {
            transfer_id: "transfer-private".into(),
            entry_id: "entry-private".into(),
            attempt_id: Some("attempt-private".into()),
            origin_device_id: "device-private".into(),
            filename: PRIVATE_NAME.into(),
            file_size: Some(32),
            cached_path: PRIVATE_PATH.into(),
            created_at_ms: 10,
        })
        .await
        .unwrap();
        store
            .append(FileTransferEvent::started(
                "transfer-private",
                "peer-private",
                PRIVATE_NAME,
                Some(32),
            ))
            .await
            .unwrap();

        let database_path = tempdir.path().join("file-transfer-projection.sqlite");
        for path in [
            database_path.clone(),
            database_path.with_extension("sqlite-wal"),
            database_path.with_extension("sqlite-shm"),
        ] {
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            for sentinel in [PRIVATE_NAME.as_bytes(), PRIVATE_PATH.as_bytes()] {
                assert!(
                    !bytes
                        .windows(sentinel.len())
                        .any(|window| window == sentinel),
                    "private transfer metadata leaked into {}",
                    path.display()
                );
            }
        }
    }

    #[tokio::test]
    async fn upgrade_purge_removes_legacy_transfer_plaintext_residue() {
        const SENTINEL: &str = "legacy-private-transfer-4d8e2a.txt";
        let tempdir = tempdir().unwrap();
        let database_path = tempdir.path().join("legacy-transfer.sqlite");
        let pool = init_db_pool(database_path.to_str().unwrap()).unwrap();
        let mut conn = pool.get().unwrap();

        for _ in 0..4 {
            conn.revert_last_migration(MIGRATIONS).unwrap();
        }
        diesel::sql_query(format!(
            "INSERT INTO file_transfer \
             (transfer_id, entry_id, filename, file_size, attempt_id, content_hash, status, \
              source_device, cached_path, failure_reason, created_at_ms, updated_at_ms) \
             VALUES ('legacy-transfer', 'legacy-entry', '{SENTINEL}', 1, NULL, NULL, \
                     'pending', 'legacy-peer', '/tmp/{SENTINEL}', NULL, 1, 1)"
        ))
        .execute(&mut conn)
        .unwrap();
        diesel::sql_query(format!(
            "INSERT INTO file_transfer_events \
             (transfer_id, sequence, event_type, payload_json, occurred_at_ms) \
             VALUES ('legacy-transfer', 1, 'started', '{{\"filename\":\"{SENTINEL}\"}}', 1)"
        ))
        .execute(&mut conn)
        .unwrap();
        conn.run_pending_migrations(MIGRATIONS).unwrap();
        drop(conn);

        let maintenance = SqliteFileTransferPrivacyMaintenance::new(std::sync::Arc::new(
            DieselSqliteExecutor::new(pool.clone()),
        ));
        maintenance
            .ensure_file_transfer_privacy_maintenance()
            .await
            .unwrap();

        // The completed marker makes retries a cheap no-op.
        maintenance
            .ensure_file_transfer_privacy_maintenance()
            .await
            .unwrap();

        let mut conn = pool.get().unwrap();
        let state = crate::db::schema::file_transfer_privacy_maintenance::table
            .select(crate::db::schema::file_transfer_privacy_maintenance::state)
            .first::<String>(&mut conn)
            .unwrap();
        assert_eq!(state, "completed");
        drop(conn);
        drop(pool);

        for suffix in ["", "-wal", "-shm"] {
            let path = std::path::PathBuf::from(format!("{}{suffix}", database_path.display()));
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            assert!(
                !bytes
                    .windows(SENTINEL.len())
                    .any(|window| window == SENTINEL.as_bytes()),
                "legacy transfer plaintext remained in {}",
                path.display()
            );
        }
    }
}
