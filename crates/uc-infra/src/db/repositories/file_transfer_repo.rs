use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use tracing::debug_span;

use crate::db::models::{EntryReceiveAttemptRow, FileTransferRow, NewFileTransferRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::{entry_receive_attempt, file_transfer};
use crate::file_transfer::persistence_cipher::{
    derive_transfer_persistence_cipher, TransferMetadata, TransferPersistenceCipher,
};
use uc_core::file_transfer::FileTransferCancellationReason;
use uc_core::ports::entry_receive_attempt::AttemptState;
use uc_core::ports::file_transfer::{
    compute_aggregate_status, CancelDirectoryAttemptTransfersPort, EntryReceiveProgress,
    EntryTransferSummary, ExpiredInflightTransfer, FailInflightTransfersPort,
    FileTransferProjectionError, FinalizeProvisionalReceivePort, FindAttemptIdForTransferPort,
    FindEntryIdForTransferPort, GetEntryReceiveProgressPort, GetEntryTransferSummaryPort,
    ListExpiredInflightTransfersPort, ListProvisionalReceivesPort, PendingInboundTransfer,
    ProvisionalInboundTransfer, ProvisionalReceiveAction, ProvisionalReceiveError,
    ProvisionalReceiveRecovery, RecordReceiverTransferPort, SeedProvisionalReceivePort,
    TrackedFileTransferStatus, UpdateProvisionalReceivePathPort,
};
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::DeriveSpaceSubkeyPort;

/// SQLite adapter for the receiver-side file-transfer projection ports.
pub struct DieselFileTransferRepository<E> {
    executor: E,
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselFileTransferRepository<E> {
    pub fn new(
        executor: E,
        derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
        current_profile: Arc<dyn CurrentProfilePort>,
    ) -> Self {
        Self {
            executor,
            derive_subkey,
            current_profile,
        }
    }
}

fn row_to_expired(
    row: &FileTransferRow,
    cipher: &TransferPersistenceCipher,
) -> anyhow::Result<ExpiredInflightTransfer> {
    let status = TrackedFileTransferStatus::from_str_value(&row.status)
        .unwrap_or(TrackedFileTransferStatus::Pending);
    let metadata = cipher.open_metadata(&row.transfer_id, &row.metadata_ciphertext)?;
    Ok(ExpiredInflightTransfer {
        transfer_id: row.transfer_id.clone(),
        entry_id: row.entry_id.clone().unwrap_or_default(),
        cached_path: metadata.cached_path.unwrap_or_default(),
        status,
    })
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
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(backend)?;
        let span = debug_span!(
            "infra.sqlite.upsert_pending_transfer",
            transfer_id = %transfer.transfer_id
        );
        let metadata_ciphertext = cipher
            .seal_metadata(
                &transfer.transfer_id,
                &TransferMetadata {
                    filename: transfer.filename.clone(),
                    cached_path: Some(transfer.cached_path.clone()),
                    failure_detail: None,
                },
            )
            .map_err(|error| backend(error.into()))?;
        let row = NewFileTransferRow {
            transfer_id: transfer.transfer_id.clone(),
            entry_id: Some(transfer.entry_id.clone()),
            file_size: transfer.file_size,
            attempt_id: transfer.attempt_id.clone(),
            binding_state: if transfer.attempt_id.is_some() {
                "attempt_bound".to_owned()
            } else {
                "legacy".to_owned()
            },
            receive_item_id: Some(transfer.transfer_id.clone()),
            item_role: None,
            content_hash: None,
            status: TrackedFileTransferStatus::Pending.as_str().to_string(),
            source_device: transfer.origin_device_id.clone(),
            failure_code: None,
            metadata_ciphertext,
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
                            file_transfer::attempt_id.eq(excluded(file_transfer::attempt_id)),
                            file_transfer::file_size.eq(excluded(file_transfer::file_size)),
                            file_transfer::source_device.eq(excluded(file_transfer::source_device)),
                            file_transfer::metadata_ciphertext
                                .eq(excluded(file_transfer::metadata_ciphertext)),
                        ))
                        .execute(conn)?;
                    Ok(())
                })?;
                Ok(())
            })
        })
        .map_err(backend)
    }
}

#[derive(Debug, thiserror::Error)]
enum ProvisionalInvariantError {
    #[error("provisional receive does not exist")]
    NotFound,
    #[error("provisional receive is no longer claimable")]
    Conflict,
}

fn provisional_error(error: anyhow::Error) -> ProvisionalReceiveError {
    match error.downcast_ref::<ProvisionalInvariantError>() {
        Some(ProvisionalInvariantError::NotFound) => ProvisionalReceiveError::NotFound,
        Some(ProvisionalInvariantError::Conflict) => ProvisionalReceiveError::Conflict,
        None => ProvisionalReceiveError::Backend(error.to_string()),
    }
}

#[async_trait]
impl<E: DbExecutor> SeedProvisionalReceivePort for DieselFileTransferRepository<E> {
    async fn seed_provisional_receive(
        &self,
        transfer: &ProvisionalInboundTransfer,
    ) -> Result<(), ProvisionalReceiveError> {
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(|error| ProvisionalReceiveError::Backend(error.to_string()))?;
        let metadata_ciphertext = cipher
            .seal_metadata(
                &transfer.transfer_id,
                &TransferMetadata {
                    filename: transfer.filename.clone(),
                    cached_path: None,
                    failure_detail: None,
                },
            )
            .map_err(|error| ProvisionalReceiveError::Backend(error.to_string()))?;
        let row = NewFileTransferRow {
            transfer_id: transfer.transfer_id.clone(),
            entry_id: None,
            file_size: transfer.file_size,
            attempt_id: None,
            binding_state: "provisional".to_owned(),
            receive_item_id: None,
            item_role: None,
            content_hash: None,
            status: TrackedFileTransferStatus::Pending.as_str().to_owned(),
            source_device: transfer.origin_device_id.clone(),
            failure_code: None,
            metadata_ciphertext,
            created_at_ms: transfer.created_at_ms,
            updated_at_ms: transfer.created_at_ms,
        };
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let existing = file_transfer::table
                        .filter(file_transfer::transfer_id.eq(&row.transfer_id))
                        .select(file_transfer::binding_state)
                        .first::<String>(conn)
                        .optional()?;
                    match existing.as_deref() {
                        None => {
                            diesel::insert_into(file_transfer::table)
                                .values(&row)
                                .execute(conn)?;
                            Ok(())
                        }
                        Some("provisional") => Ok(()),
                        Some(_) => Err(ProvisionalInvariantError::Conflict.into()),
                    }
                })
            })
            .map_err(provisional_error)
    }
}

#[async_trait]
impl<E: DbExecutor> UpdateProvisionalReceivePathPort for DieselFileTransferRepository<E> {
    async fn update_provisional_receive_path(
        &self,
        provisional_transfer_id: &str,
        cached_path: &str,
        now_ms: i64,
    ) -> Result<(), ProvisionalReceiveError> {
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(|error| ProvisionalReceiveError::Backend(error.to_string()))?;
        let transfer_id = provisional_transfer_id.to_owned();
        let cached_path = cached_path.to_owned();
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let ciphertext = file_transfer::table
                        .filter(file_transfer::transfer_id.eq(&transfer_id))
                        .filter(file_transfer::binding_state.eq("provisional"))
                        .select(file_transfer::metadata_ciphertext)
                        .first::<Vec<u8>>(conn)
                        .optional()?
                        .ok_or(ProvisionalInvariantError::NotFound)?;
                    let mut metadata = cipher.open_metadata(&transfer_id, &ciphertext)?;
                    metadata.cached_path = Some(cached_path);
                    let ciphertext = cipher.seal_metadata(&transfer_id, &metadata)?;
                    let affected = diesel::update(
                        file_transfer::table
                            .filter(file_transfer::transfer_id.eq(&transfer_id))
                            .filter(file_transfer::binding_state.eq("provisional")),
                    )
                    .set((
                        file_transfer::metadata_ciphertext.eq(ciphertext),
                        file_transfer::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                    if affected == 1 {
                        Ok(())
                    } else {
                        Err(ProvisionalInvariantError::Conflict.into())
                    }
                })
            })
            .map_err(provisional_error)
    }
}

#[async_trait]
impl<E: DbExecutor> ListProvisionalReceivesPort for DieselFileTransferRepository<E> {
    async fn list_provisional_receives(
        &self,
    ) -> Result<Vec<ProvisionalReceiveRecovery>, ProvisionalReceiveError> {
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(|error| ProvisionalReceiveError::Backend(error.to_string()))?;
        self.executor
            .run(move |conn| {
                let rows = file_transfer::table
                    .filter(file_transfer::binding_state.eq("provisional"))
                    .select((
                        file_transfer::transfer_id,
                        file_transfer::metadata_ciphertext,
                    ))
                    .load::<(String, Vec<u8>)>(conn)?;
                rows.into_iter()
                    .map(|(transfer_id, ciphertext)| {
                        let metadata = cipher.open_metadata(&transfer_id, &ciphertext)?;
                        Ok(ProvisionalReceiveRecovery {
                            transfer_id,
                            cached_path: metadata.cached_path,
                        })
                    })
                    .collect::<anyhow::Result<Vec<_>>>()
            })
            .map_err(|error| ProvisionalReceiveError::Backend(error.to_string()))
    }
}

#[async_trait]
impl<E: DbExecutor> FinalizeProvisionalReceivePort for DieselFileTransferRepository<E> {
    async fn finalize_provisional_receive(
        &self,
        provisional_transfer_id: &str,
        action: ProvisionalReceiveAction,
        now_ms: i64,
    ) -> Result<(), ProvisionalReceiveError> {
        let transfer_id = provisional_transfer_id.to_owned();
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let binding = file_transfer::table
                        .filter(file_transfer::transfer_id.eq(&transfer_id))
                        .select(file_transfer::binding_state)
                        .first::<String>(conn)
                        .optional()?
                        .ok_or(ProvisionalInvariantError::NotFound)?;
                    if binding != "provisional" {
                        return Err(ProvisionalInvariantError::Conflict.into());
                    }

                    match action {
                        ProvisionalReceiveAction::DiscardAsFullyHeld => {
                            let affected = diesel::delete(
                                file_transfer::table
                                    .filter(file_transfer::transfer_id.eq(&transfer_id))
                                    .filter(file_transfer::binding_state.eq("provisional")),
                            )
                            .execute(conn)?;
                            if affected == 1 {
                                Ok(())
                            } else {
                                Err(ProvisionalInvariantError::Conflict.into())
                            }
                        }
                        ProvisionalReceiveAction::AdoptIntoAttempt {
                            entry_id,
                            attempt_id,
                            item_id,
                            role,
                        } => {
                            let current = entry_receive_attempt::table
                                .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                                .select((
                                    entry_receive_attempt::current_attempt_id,
                                    entry_receive_attempt::attempt_state,
                                ))
                                .first::<(String, String)>(conn)
                                .optional()?;
                            if current.as_ref().is_none_or(|(current_id, state)| {
                                current_id != &attempt_id
                                    || state != AttemptState::Receiving.to_string().as_str()
                            }) {
                                return Err(ProvisionalInvariantError::Conflict.into());
                            }
                            let affected = diesel::update(
                                file_transfer::table
                                    .filter(file_transfer::transfer_id.eq(&transfer_id))
                                    .filter(file_transfer::binding_state.eq("provisional"))
                                    .filter(file_transfer::entry_id.is_null())
                                    .filter(file_transfer::attempt_id.is_null()),
                            )
                            .set((
                                file_transfer::entry_id.eq(Some(entry_id)),
                                file_transfer::attempt_id.eq(Some(attempt_id)),
                                file_transfer::binding_state.eq("attempt_bound"),
                                file_transfer::receive_item_id.eq(Some(item_id)),
                                file_transfer::item_role.eq(Some(role.as_str().to_owned())),
                                file_transfer::updated_at_ms.eq(now_ms),
                            ))
                            .execute(conn)?;
                            if affected == 1 {
                                Ok(())
                            } else {
                                Err(ProvisionalInvariantError::Conflict.into())
                            }
                        }
                    }
                })
            })
            .map_err(provisional_error)
    }
}

#[async_trait]
impl<E: DbExecutor> CancelDirectoryAttemptTransfersPort for DieselFileTransferRepository<E> {
    async fn cancel_attempt_transfers(
        &self,
        entry_id: &str,
        attempt_id: &str,
        reason: FileTransferCancellationReason,
        now_ms: i64,
    ) -> Result<u32, FileTransferProjectionError> {
        let entry_id = entry_id.to_owned();
        let attempt_id = attempt_id.to_owned();
        self.executor
            .run(move |conn| {
                let affected = diesel::update(
                    file_transfer::table
                        .filter(file_transfer::entry_id.eq(entry_id))
                        .filter(file_transfer::attempt_id.eq(attempt_id))
                        .filter(file_transfer::status.eq_any([
                            TrackedFileTransferStatus::Pending.as_str(),
                            TrackedFileTransferStatus::Transferring.as_str(),
                        ])),
                )
                .set((
                    file_transfer::status.eq(TrackedFileTransferStatus::Cancelled.as_str()),
                    file_transfer::failure_code.eq(Some(reason.as_str().to_owned())),
                    file_transfer::updated_at_ms.eq(now_ms),
                ))
                .execute(conn)?;
                u32::try_from(affected).map_err(anyhow::Error::from)
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
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(backend)?;
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

                // Pick the encrypted detail, falling back to the stable code.
                let failure_reason = if aggregate_status == TrackedFileTransferStatus::Failed {
                    rows.iter()
                        .find(|r| r.status == TrackedFileTransferStatus::Failed.as_str())
                        .map(|row| {
                            cipher
                                .open_metadata(&row.transfer_id, &row.metadata_ciphertext)
                                .map(|metadata| {
                                    metadata.failure_detail.or_else(|| row.failure_code.clone())
                                })
                        })
                        .transpose()?
                        .flatten()
                } else {
                    None
                };

                let mut transfer_ids: Vec<String> =
                    rows.iter().map(|row| row.transfer_id.clone()).collect();
                transfer_ids.sort();

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
impl<E: DbExecutor> GetEntryReceiveProgressPort for DieselFileTransferRepository<E> {
    async fn get_entry_receive_progress(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryReceiveProgress>, FileTransferProjectionError> {
        let entry_id = entry_id.to_owned();
        let result = self
            .executor
            .run(move |conn| {
                let attempt = entry_receive_attempt::table
                    .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                    .select(EntryReceiveAttemptRow::as_select())
                    .first(conn)
                    .optional()?;
                let Some(attempt) = attempt else {
                    return Ok(None);
                };
                let rows = file_transfer::table
                    .filter(file_transfer::entry_id.eq(&entry_id))
                    .filter(file_transfer::attempt_id.eq(Some(&attempt.current_attempt_id)))
                    .load::<FileTransferRow>(conn)?;
                Ok(Some((attempt, rows)))
            })
            .map_err(backend)?;

        let Some((attempt, rows)) = result else {
            return Ok(None);
        };
        let state = AttemptState::from_str(&attempt.attempt_state)
            .map_err(|error| FileTransferProjectionError::Backend(error.to_string()))?;
        let items_total = u32::try_from(rows.len()).map_err(|_| {
            FileTransferProjectionError::Backend("receive item count exceeds u32".into())
        })?;
        let items_completed = u32::try_from(
            rows.iter()
                .filter(|row| row.status == TrackedFileTransferStatus::Completed.as_str())
                .count(),
        )
        .map_err(|_| {
            FileTransferProjectionError::Backend("completed receive item count exceeds u32".into())
        })?;
        let total_bytes = rows
            .iter()
            .try_fold(0_i64, |sum, row| match row.file_size {
                Some(size) if size < 0 => Err(FileTransferProjectionError::Backend(
                    "negative receive item size".into(),
                )),
                Some(size) => sum.checked_add(size).ok_or_else(|| {
                    FileTransferProjectionError::Backend("receive byte total overflow".into())
                }),
                None => Ok(sum),
            })?;
        let completed_bytes = rows.iter().try_fold(0_i64, |sum, row| {
            if row.status != TrackedFileTransferStatus::Completed.as_str() {
                return Ok(sum);
            }
            match row.file_size {
                Some(size) if size < 0 => Err(FileTransferProjectionError::Backend(
                    "negative receive item size".into(),
                )),
                Some(size) => sum.checked_add(size).ok_or_else(|| {
                    FileTransferProjectionError::Backend(
                        "completed receive byte total overflow".into(),
                    )
                }),
                None => Ok(sum),
            }
        })?;

        Ok(Some(EntryReceiveProgress {
            entry_id: attempt.entry_id,
            attempt_id: attempt.current_attempt_id,
            state,
            total_bytes,
            completed_bytes,
            items_total,
            items_completed,
        }))
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
                    .first::<Option<String>>(conn)
                    .optional()?
                    .flatten();
                Ok(entry_id)
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> FindAttemptIdForTransferPort for DieselFileTransferRepository<E> {
    async fn get_attempt_id_for_transfer(
        &self,
        transfer_id: &str,
    ) -> Result<Option<String>, FileTransferProjectionError> {
        let transfer_id = transfer_id.to_owned();
        self.executor
            .run(move |conn| {
                file_transfer::table
                    .filter(file_transfer::transfer_id.eq(transfer_id))
                    .select(file_transfer::attempt_id)
                    .first::<Option<String>>(conn)
                    .optional()
                    .map(Option::flatten)
                    .map_err(Into::into)
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
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(backend)?;
        self.executor
            .run(move |conn| {
                let rows = file_transfer::table
                    .filter(file_transfer::binding_state.eq("legacy"))
                    .filter(
                        file_transfer::status
                            .eq(TrackedFileTransferStatus::Pending.as_str())
                            .and(file_transfer::updated_at_ms.lt(pending_cutoff_ms))
                            .or(file_transfer::status
                                .eq(TrackedFileTransferStatus::Transferring.as_str())
                                .and(file_transfer::updated_at_ms.lt(transferring_cutoff_ms))),
                    )
                    .load::<FileTransferRow>(conn)?;
                rows.iter()
                    .map(|row| row_to_expired(row, &cipher))
                    .collect::<anyhow::Result<Vec<_>>>()
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
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(backend)?;
        self.executor
            .run(move |conn| {
                let Some(mut metadata) = load_transfer_metadata(conn, &tid, &cipher)? else {
                    return Ok(());
                };
                metadata.failure_detail = Some(reason);
                let metadata_ciphertext = cipher.seal_metadata(&tid, &metadata)?;
                diesel::update(file_transfer::table.filter(file_transfer::transfer_id.eq(&tid)))
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                        file_transfer::failure_code.eq(Some("interrupted")),
                        file_transfer::metadata_ciphertext.eq(metadata_ciphertext),
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
        let cipher = derive_transfer_persistence_cipher(&self.derive_subkey, &self.current_profile)
            .await
            .map_err(backend)?;
        self.executor
            .run(move |conn| {
                // Legacy rows have no attempt authority. Attempt-bound and
                // provisional rows are reconciled by their owning workflows.
                let rows = file_transfer::table
                    .filter(file_transfer::binding_state.eq("legacy"))
                    .filter(
                        file_transfer::status
                            .eq(TrackedFileTransferStatus::Pending.as_str())
                            .or(file_transfer::status
                                .eq(TrackedFileTransferStatus::Transferring.as_str())),
                    )
                    .load::<FileTransferRow>(conn)?;

                let targets = rows
                    .iter()
                    .map(|row| row_to_expired(row, &cipher))
                    .collect::<anyhow::Result<Vec<_>>>()?;

                for row in &rows {
                    let mut metadata =
                        cipher.open_metadata(&row.transfer_id, &row.metadata_ciphertext)?;
                    metadata.failure_detail = Some(reason.clone());
                    let metadata_ciphertext = cipher.seal_metadata(&row.transfer_id, &metadata)?;
                    diesel::update(
                        file_transfer::table
                            .filter(file_transfer::transfer_id.eq(&row.transfer_id)),
                    )
                    .set((
                        file_transfer::status.eq(TrackedFileTransferStatus::Failed.as_str()),
                        file_transfer::failure_code.eq(Some("interrupted")),
                        file_transfer::metadata_ciphertext.eq(metadata_ciphertext),
                        file_transfer::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                }

                Ok(targets)
            })
            .map_err(backend)
    }
}

fn load_transfer_metadata(
    conn: &mut diesel::sqlite::SqliteConnection,
    transfer_id: &str,
    cipher: &TransferPersistenceCipher,
) -> anyhow::Result<Option<TransferMetadata>> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use crate::db::ports::DbExecutor;
    use tempfile::{tempdir, TempDir};

    type Repo = DieselFileTransferRepository<DieselSqliteExecutor>;

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("file-transfer-repo.sqlite");
        let path_str = path.to_str().unwrap();
        let (derive_subkey, current_profile) =
            crate::file_transfer::persistence_cipher::test_keys::ports();
        let repo = Repo::new(
            DieselSqliteExecutor::new(init_db_pool(path_str).unwrap()),
            derive_subkey,
            current_profile,
        );
        let writer = DieselSqliteExecutor::new(init_db_pool(path_str).unwrap());
        (repo, writer, tempdir)
    }

    async fn seed(repo: &Repo, transfer_id: &str, filename: &str) {
        repo.upsert_pending_transfer(&PendingInboundTransfer {
            transfer_id: transfer_id.to_string(),
            entry_id: "entry-directory".to_string(),
            attempt_id: None,
            origin_device_id: "peer-a".to_string(),
            filename: filename.to_string(),
            file_size: None,
            cached_path: String::new(),
            created_at_ms: 1,
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn entry_summary_sorts_transfer_ids_and_aggregates_failure() {
        let (repo, writer, _tempdir) = make_repo();
        // Seed out of order to prove the summary sorts deterministically.
        seed(&repo, "transfer-c", "c.txt").await;
        seed(&repo, "transfer-a", "a.txt").await;
        seed(&repo, "transfer-b", "b.txt").await;
        writer
            .run(|conn| {
                diesel::update(
                    file_transfer::table.filter(file_transfer::transfer_id.eq("transfer-a")),
                )
                .set(file_transfer::status.eq(TrackedFileTransferStatus::Completed.as_str()))
                .execute(conn)?;
                diesel::update(
                    file_transfer::table.filter(file_transfer::transfer_id.eq("transfer-b")),
                )
                .set(file_transfer::updated_at_ms.eq(2))
                .execute(conn)?;
                Ok(())
            })
            .unwrap();
        repo.mark_failed("transfer-b", "network", 2).await.unwrap();

        let summary = repo
            .get_entry_transfer_summary("entry-directory")
            .await
            .unwrap()
            .unwrap();

        assert_eq!(summary.aggregate_status, TrackedFileTransferStatus::Failed);
        assert_eq!(summary.failure_reason.as_deref(), Some("network"));
        assert_eq!(
            summary.transfer_ids,
            vec![
                "transfer-a".to_string(),
                "transfer-b".to_string(),
                "transfer-c".to_string()
            ]
        );
    }
}
