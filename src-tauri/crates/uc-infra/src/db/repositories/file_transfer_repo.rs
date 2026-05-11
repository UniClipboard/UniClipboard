use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use tracing::debug_span;

use crate::db::models::{FileTransferRow, NewFileTransferRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::file_transfer;
use uc_core::ports::file_transfer_repository::{
    compute_aggregate_status, EntryTransferSummary, ExpiredInflightTransfer,
    FileTransferRepositoryPort, PendingInboundTransfer, TrackedFileTransfer,
    TrackedFileTransferStatus,
};

/// SQLite adapter for `FileTransferRepositoryPort`.
pub struct DieselFileTransferRepository<E> {
    executor: E,
}

impl<E> DieselFileTransferRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

fn row_to_domain(row: &FileTransferRow) -> TrackedFileTransfer {
    let status = TrackedFileTransferStatus::from_str_value(&row.status)
        .unwrap_or(TrackedFileTransferStatus::Pending);
    TrackedFileTransfer {
        transfer_id: row.transfer_id.clone(),
        entry_id: row.entry_id.clone(),
        origin_device_id: row.source_device.clone(),
        filename: row.filename.clone(),
        cached_path: row.cached_path.clone().unwrap_or_default(),
        status,
        failure_reason: row.failure_reason.clone(),
        file_size: row.file_size,
        content_hash: row.content_hash.clone(),
        updated_at_ms: row.updated_at_ms,
        created_at_ms: row.created_at_ms,
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

#[async_trait]
impl<E: DbExecutor> FileTransferRepositoryPort for DieselFileTransferRepository<E> {
    async fn insert_pending_transfers(
        &self,
        transfers: &[PendingInboundTransfer],
    ) -> anyhow::Result<()> {
        let span = debug_span!(
            "infra.sqlite.insert_pending_transfers",
            count = transfers.len()
        );
        let rows: Vec<NewFileTransferRow> = transfers
            .iter()
            .map(|t| NewFileTransferRow {
                transfer_id: t.transfer_id.clone(),
                entry_id: t.entry_id.clone(),
                filename: t.filename.clone(),
                file_size: None,
                content_hash: None,
                status: TrackedFileTransferStatus::Pending.as_str().to_string(),
                source_device: t.origin_device_id.clone(),
                cached_path: Some(t.cached_path.clone()),
                failure_reason: None,
                created_at_ms: t.created_at_ms,
                updated_at_ms: t.created_at_ms,
            })
            .collect();

        span.in_scope(|| {
            self.executor.run(|conn| {
                for row in &rows {
                    diesel::insert_into(file_transfer::table)
                        .values(row)
                        .execute(conn)?;
                }
                Ok(())
            })
        })
    }

    async fn upsert_pending_transfer(
        &self,
        transfer: &PendingInboundTransfer,
    ) -> anyhow::Result<()> {
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
                            tracing::warn!(
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
    }

    async fn backfill_announce_metadata(
        &self,
        transfer_id: &str,
        file_size: i64,
        content_hash: &str,
    ) -> anyhow::Result<()> {
        let span = debug_span!(
            "infra.sqlite.backfill_announce_metadata",
            transfer_id = transfer_id
        );
        let tid = transfer_id.to_string();
        let hash = content_hash.to_string();
        span.in_scope(|| {
            self.executor.run(move |conn| {
                diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                    .set((
                        file_transfer::file_size.eq(Some(file_size)),
                        file_transfer::content_hash.eq(Some(&hash)),
                    ))
                    .execute(conn)?;
                Ok(())
            })
        })
    }

    async fn mark_transferring(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<bool> {
        let span = debug_span!("infra.sqlite.mark_transferring", transfer_id = transfer_id);
        let tid = transfer_id.to_string();
        span.in_scope(|| {
            self.executor.run(move |conn| {
                let affected = diesel::update(
                    file_transfer::table
                        .filter(file_transfer::transfer_id.eq(&tid))
                        .filter(
                            file_transfer::status
                                .eq(TrackedFileTransferStatus::Pending.as_str())
                                .or(file_transfer::status
                                    .eq(TrackedFileTransferStatus::Transferring.as_str())),
                        ),
                )
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Transferring.as_str()),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?;
                Ok(affected > 0)
            })
        })
    }

    async fn refresh_activity(&self, transfer_id: &str, now_ms: i64) -> anyhow::Result<()> {
        let tid = transfer_id.to_string();
        self.executor.run(move |conn| {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                .set(file_transfer::updated_at_ms.eq(now_ms))
                .execute(conn)?;
            Ok(())
        })
    }

    async fn mark_completed(
        &self,
        transfer_id: &str,
        content_hash: Option<&str>,
        now_ms: i64,
    ) -> anyhow::Result<bool> {
        let tid = transfer_id.to_string();
        let hash = content_hash.map(|h| h.to_string());
        self.executor.run(move |conn| {
            // Always set status and updated_at_ms; optionally set content_hash
            let affected = if let Some(h) = &hash {
                diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()),
                        file_transfer::updated_at_ms.eq(now_ms),
                        file_transfer::content_hash.eq(Some(h)),
                    ))
                    .execute(conn)?
            } else {
                diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()),
                        file_transfer::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?
            };
            Ok(affected > 0)
        })
    }

    async fn mark_failed(
        &self,
        transfer_id: &str,
        reason: &str,
        now_ms: i64,
    ) -> anyhow::Result<()> {
        let tid = transfer_id.to_string();
        let reason = reason.to_string();
        self.executor.run(move |conn| {
            diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                    file_transfer::failure_reason.eq(Some(&reason)),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?;
            Ok(())
        })
    }

    async fn list_expired_inflight(
        &self,
        pending_cutoff_ms: i64,
        transferring_cutoff_ms: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>> {
        self.executor.run(move |conn| {
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
    }

    async fn bulk_fail_inflight(
        &self,
        reason: &str,
        now_ms: i64,
    ) -> anyhow::Result<Vec<ExpiredInflightTransfer>> {
        let reason = reason.to_string();
        self.executor.run(move |conn| {
            // First, select all in-flight rows
            let rows = file_transfer::table
                .filter(
                    file_transfer::status
                        .eq(TrackedFileTransferStatus::Pending.as_str())
                        .or(file_transfer::status
                            .eq(TrackedFileTransferStatus::Transferring.as_str())),
                )
                .load::<FileTransferRow>(conn)?;

            let targets: Vec<ExpiredInflightTransfer> = rows.iter().map(row_to_expired).collect();

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
    }

    async fn get_entry_transfer_summary(
        &self,
        entry_id: &str,
    ) -> anyhow::Result<Option<EntryTransferSummary>> {
        let eid = entry_id.to_string();
        self.executor.run(move |conn| {
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
    }

    async fn list_transfers_for_entry(
        &self,
        entry_id: &str,
    ) -> anyhow::Result<Vec<TrackedFileTransfer>> {
        let eid = entry_id.to_string();
        self.executor.run(move |conn| {
            let rows = file_transfer::table
                .filter(file_transfer::entry_id.eq(&eid))
                .load::<FileTransferRow>(conn)?;
            Ok(rows.iter().map(row_to_domain).collect())
        })
    }

    async fn get_entry_id_for_transfer(&self, transfer_id: &str) -> anyhow::Result<Option<String>> {
        let tid = transfer_id.to_string();
        self.executor.run(move |conn| {
            let entry_id = file_transfer::table
                .filter(file_transfer::transfer_id.eq(&tid))
                .select(file_transfer::entry_id)
                .first::<String>(conn)
                .optional()?;
            Ok(entry_id)
        })
    }

    async fn get_transfer(&self, transfer_id: &str) -> anyhow::Result<Option<TrackedFileTransfer>> {
        let tid = transfer_id.to_string();
        self.executor.run(move |conn| {
            let row = file_transfer::table
                .filter(file_transfer::transfer_id.eq(&tid))
                .first::<FileTransferRow>(conn)
                .optional()?;
            Ok(row.as_ref().map(row_to_domain))
        })
    }

    async fn link_transfer_to_entry(
        &self,
        transfer_id: &str,
        entry_id: &str,
        now_ms: i64,
    ) -> anyhow::Result<bool> {
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
    }
}
