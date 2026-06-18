use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use tracing::debug_span;

use crate::db::models::{FileTransferRow, NewFileTransferRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::file_transfer;
use uc_core::ports::file_transfer::{
    compute_aggregate_status, EntryTransferSummary, ExpiredInflightTransfer,
    FailInflightTransfersPort, FileTransferProjectionError, FindEntryIdForTransferPort,
    GetEntryTransferSummaryPort, ListExpiredInflightTransfersPort, PendingInboundTransfer,
    RecordReceiverTransferPort, TrackedFileTransferStatus,
};

/// SQLite adapter for the receiver-side file-transfer projection ports.
pub struct DieselFileTransferRepository<E> {
    executor: E,
}

impl<E> DieselFileTransferRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

fn row_to_expired(row: &FileTransferRow) -> ExpiredInflightTransfer {
    let status = TrackedFileTransferStatus::from_str_value(&row.status)
        .unwrap_or(TrackedFileTransferStatus::Pending);
    ExpiredInflightTransfer {
        transfer_id: row.transfer_id.clone(),
        entry_id: row.entry_id.clone(),
        cached_path: row.cached_path.clone().unwrap_or_default(),
        status,
    }
}

/// Map a backend (Diesel/I-O) failure onto the domain projection error.
fn backend(err: anyhow::Error) -> FileTransferProjectionError {
    FileTransferProjectionError::Backend(err.to_string())
}

#[async_trait]
impl<E: DbExecutor> RecordReceiverTransferPort for DieselFileTransferRepository<E> {
    async fn upsert_pending_transfer(
        &self,
        transfer: &PendingInboundTransfer,
    ) -> Result<(), FileTransferProjectionError> {
        let span = debug_span!(
            "infra.sqlite.upsert_pending_transfer",
            transfer_id = %transfer.transfer_id
        );
        let row = NewFileTransferRow {
            transfer_id: transfer.transfer_id.clone(),
            entry_id: transfer.entry_id.clone(),
            filename: transfer.filename.clone(),
            file_size: None,
            content_hash: None,
            status: TrackedFileTransferStatus::Pending.as_str().to_string(),
            source_device: transfer.origin_device_id.clone(),
            cached_path: Some(transfer.cached_path.clone()),
            failure_reason: None,
            created_at_ms: transfer.created_at_ms,
            updated_at_ms: transfer.created_at_ms,
        };

        span.in_scope(|| {
            self.executor.run(move |conn| {
                // 仅当行不存在 或 现有行 status='pending' 时才执行 upsert,
                // 防止重试 seed 把已 transferring / completed / failed /
                // cancelled 的终止态行覆盖回 pending。包在 transaction 里
                // 保证 SELECT-then-INSERT 原子。
                conn.transaction::<_, diesel::result::Error, _>(|conn| {
                    let existing_status: Option<String> = file_transfer::table
                        .filter(file_transfer::transfer_id.eq(&row.transfer_id))
                        .select(file_transfer::status)
                        .first::<String>(conn)
                        .optional()?;
                    if let Some(status) = existing_status.as_deref() {
                        if status != TrackedFileTransferStatus::Pending.as_str() {
                            tracing::debug!(
                                transfer_id = %row.transfer_id,
                                existing_status = status,
                                "upsert_pending_transfer: skipping — existing row is not pending"
                            );
                            return Ok(());
                        }
                    }
                    diesel::insert_into(file_transfer::table)
                        .values(&row)
                        .on_conflict(file_transfer::transfer_id)
                        .do_update()
                        .set((
                            file_transfer::entry_id.eq(excluded(file_transfer::entry_id)),
                            file_transfer::filename.eq(excluded(file_transfer::filename)),
                            file_transfer::source_device.eq(excluded(file_transfer::source_device)),
                            file_transfer::cached_path.eq(excluded(file_transfer::cached_path)),
                        ))
                        .execute(conn)?;
                    Ok(())
                })?;
                Ok(())
            })
        })
        .map_err(backend)
    }

    async fn link_transfer_to_entry(
        &self,
        transfer_id: &str,
        entry_id: &str,
        now_ms: i64,
    ) -> Result<bool, FileTransferProjectionError> {
        let span = debug_span!(
            "infra.sqlite.link_transfer_to_entry",
            transfer_id = transfer_id
        );
        let tid = transfer_id.to_string();
        let eid = entry_id.to_string();
        span.in_scope(|| {
            self.executor.run(move |conn| {
                let affected = diesel::update(
                    file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)),
                )
                .set((
                    file_transfer::entry_id.eq(&eid),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?;
                Ok(affected > 0)
            })
        })
        .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> GetEntryTransferSummaryPort for DieselFileTransferRepository<E> {
    async fn get_entry_transfer_summary(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryTransferSummary>, FileTransferProjectionError> {
        let eid = entry_id.to_string();
        self.executor
            .run(move |conn| {
                let rows = file_transfer::table
                    .filter(file_transfer::entry_id.eq(&eid))
                    .load::<FileTransferRow>(conn)?;

                if rows.is_empty() {
                    return Ok(None);
                }

                let statuses: Vec<TrackedFileTransferStatus> = rows
                    .iter()
                    .map(|r| {
                        TrackedFileTransferStatus::from_str_value(&r.status)
                            .unwrap_or(TrackedFileTransferStatus::Pending)
                    })
                    .collect();

                let aggregate_status = match compute_aggregate_status(&statuses) {
                    Some(s) => s,
                    None => return Ok(None),
                };

                // Pick failure_reason from any failed transfer
                let failure_reason = if aggregate_status == TrackedFileTransferStatus::Failed {
                    rows.iter()
                        .find(|r| r.status == TrackedFileTransferStatus::Failed.as_str())
                        .and_then(|r| r.failure_reason.clone())
                } else {
                    None
                };

                let transfer_ids = rows.iter().map(|r| r.transfer_id.clone()).collect();

                Ok(Some(EntryTransferSummary {
                    entry_id: eid,
                    aggregate_status,
                    failure_reason,
                    transfer_ids,
                }))
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> FindEntryIdForTransferPort for DieselFileTransferRepository<E> {
    async fn get_entry_id_for_transfer(
        &self,
        transfer_id: &str,
    ) -> Result<Option<String>, FileTransferProjectionError> {
        let tid = transfer_id.to_string();
        self.executor
            .run(move |conn| {
                let entry_id = file_transfer::table
                    .filter(file_transfer::transfer_id.eq(&tid))
                    .select(file_transfer::entry_id)
                    .first::<String>(conn)
                    .optional()?;
                Ok(entry_id)
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> ListExpiredInflightTransfersPort for DieselFileTransferRepository<E> {
    async fn list_expired_inflight(
        &self,
        pending_cutoff_ms: i64,
        transferring_cutoff_ms: i64,
    ) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError> {
        self.executor
            .run(move |conn| {
                let rows = file_transfer::table
                    .filter(
                        file_transfer::status
                            .eq(TrackedFileTransferStatus::Pending.as_str())
                            .and(file_transfer::updated_at_ms.lt(pending_cutoff_ms))
                            .or(file_transfer::status
                                .eq(TrackedFileTransferStatus::Transferring.as_str())
                                .and(file_transfer::updated_at_ms.lt(transferring_cutoff_ms))),
                    )
                    .load::<FileTransferRow>(conn)?;
                Ok(rows.iter().map(row_to_expired).collect())
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> FailInflightTransfersPort for DieselFileTransferRepository<E> {
    async fn mark_failed(
        &self,
        transfer_id: &str,
        reason: &str,
        now_ms: i64,
    ) -> Result<(), FileTransferProjectionError> {
        let tid = transfer_id.to_string();
        let reason = reason.to_string();
        self.executor
            .run(move |conn| {
                diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                        file_transfer::failure_reason.eq(Some(&reason)),
                        file_transfer::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                Ok(())
            })
            .map_err(backend)
    }

    async fn bulk_fail_inflight(
        &self,
        reason: &str,
        now_ms: i64,
    ) -> Result<Vec<ExpiredInflightTransfer>, FileTransferProjectionError> {
        let reason = reason.to_string();
        self.executor
            .run(move |conn| {
                // First, select all in-flight rows
                let rows = file_transfer::table
                    .filter(
                        file_transfer::status
                            .eq(TrackedFileTransferStatus::Pending.as_str())
                            .or(file_transfer::status
                                .eq(TrackedFileTransferStatus::Transferring.as_str())),
                    )
                    .load::<FileTransferRow>(conn)?;

                let targets: Vec<ExpiredInflightTransfer> =
                    rows.iter().map(row_to_expired).collect();

                // Then bulk-update them to failed
                if !targets.is_empty() {
                    diesel::update(
                        file_transfer::table.filter(
                            file_transfer::status
                                .eq(TrackedFileTransferStatus::Pending.as_str())
                                .or(file_transfer::status
                                    .eq(TrackedFileTransferStatus::Transferring.as_str())),
                        ),
                    )
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                        file_transfer::failure_reason.eq(Some(&reason)),
                        file_transfer::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                }

                Ok(targets)
            })
            .map_err(backend)
    }
}
