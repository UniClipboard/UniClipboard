use std::str::FromStr;

use async_trait::async_trait;
use diesel::prelude::*;

use crate::db::models::{EntryReceiveAttemptRow, NewEntryReceiveAttemptRow};
use crate::db::ports::DbExecutor;
use crate::db::schema::entry_receive_attempt;
use crate::db::schema::{
    clipboard_entry, directory_publish_log, file_transfer, file_transfer_events,
    receive_artifact_log,
};
use uc_core::ports::entry_receive_attempt::{
    AttemptError, AttemptState, BeginReceiveAttemptPort, BeginReceiveFailureOutcome,
    BeginReceiveFailurePort, BeginReceiveOutcome, ClaimReceiveCommitPort,
    DeleteReceiveStateForEntryPort, EntryReceiveAttempt, GetEntryAttemptPort,
    ListNonTerminalAttemptsPort, PurgeTerminalOrphanAttemptsPort,
    RequestReceiveCancellationOutcome, RequestReceiveCancellationPort,
};

/// SQLite adapter for authoritative directory receive attempts.
pub struct DieselEntryReceiveAttemptRepository<E> {
    executor: E,
}

impl<E> DieselEntryReceiveAttemptRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

fn backend(error: anyhow::Error) -> AttemptError {
    AttemptError::Backend(error.to_string())
}

fn to_attempt(row: EntryReceiveAttemptRow) -> Result<EntryReceiveAttempt, AttemptError> {
    Ok(EntryReceiveAttempt {
        entry_id: row.entry_id,
        current_attempt_id: row.current_attempt_id,
        state: AttemptState::from_str(&row.attempt_state)?,
        updated_at_ms: row.updated_at_ms,
    })
}

#[async_trait]
impl<E: DbExecutor> GetEntryAttemptPort for DieselEntryReceiveAttemptRepository<E> {
    async fn get_entry_attempt(
        &self,
        entry_id: &str,
    ) -> Result<Option<EntryReceiveAttempt>, AttemptError> {
        let entry_id = entry_id.to_owned();
        let row = self
            .executor
            .run(move |conn| {
                entry_receive_attempt::table
                    .filter(entry_receive_attempt::entry_id.eq(entry_id))
                    .select(EntryReceiveAttemptRow::as_select())
                    .first(conn)
                    .optional()
                    .map_err(anyhow::Error::from)
            })
            .map_err(backend)?;
        row.map(to_attempt).transpose()
    }
}

#[async_trait]
impl<E: DbExecutor> ListNonTerminalAttemptsPort for DieselEntryReceiveAttemptRepository<E> {
    async fn list_non_terminal_attempts(&self) -> Result<Vec<EntryReceiveAttempt>, AttemptError> {
        let rows = self
            .executor
            .run(move |conn| {
                entry_receive_attempt::table
                    .filter(
                        entry_receive_attempt::attempt_state
                            .eq(AttemptState::Receiving.to_string())
                            .or(entry_receive_attempt::attempt_state
                                .eq(AttemptState::Committing.to_string()))
                            .or(entry_receive_attempt::attempt_state
                                .eq(AttemptState::Cancelling.to_string()))
                            .or(entry_receive_attempt::attempt_state
                                .eq(AttemptState::Failing.to_string())),
                    )
                    .select(EntryReceiveAttemptRow::as_select())
                    .load(conn)
                    .map_err(anyhow::Error::from)
            })
            .map_err(backend)?;
        rows.into_iter().map(to_attempt).collect()
    }
}

#[async_trait]
impl<E: DbExecutor> BeginReceiveAttemptPort for DieselEntryReceiveAttemptRepository<E> {
    async fn begin_first_receive(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveOutcome, AttemptError> {
        let entry_id = entry_id.to_owned();
        let attempt_id = attempt_id.to_owned();
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let existing = entry_receive_attempt::table
                        .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                        .select(EntryReceiveAttemptRow::as_select())
                        .first::<EntryReceiveAttemptRow>(conn)
                        .optional()?;
                    if let Some(existing) = existing {
                        let state = AttemptState::from_str(&existing.attempt_state)
                            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                        return Ok(if state.is_terminal() {
                            BeginReceiveOutcome::Superseded
                        } else {
                            BeginReceiveOutcome::AlreadyReceiving
                        });
                    }
                    diesel::insert_into(entry_receive_attempt::table)
                        .values(NewEntryReceiveAttemptRow {
                            entry_id,
                            current_attempt_id: attempt_id,
                            attempt_state: AttemptState::Receiving.to_string(),
                            updated_at_ms: now_ms,
                        })
                        .execute(conn)?;
                    Ok(BeginReceiveOutcome::Begun)
                })
            })
            .map_err(backend)
    }

    async fn begin_redelivery(
        &self,
        entry_id: &str,
        expected_attempt_id: &str,
        new_attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveOutcome, AttemptError> {
        let entry_id = entry_id.to_owned();
        let expected_attempt_id = expected_attempt_id.to_owned();
        let new_attempt_id = new_attempt_id.to_owned();
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let affected = diesel::update(
                        entry_receive_attempt::table
                            .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                            .filter(
                                entry_receive_attempt::current_attempt_id.eq(&expected_attempt_id),
                            )
                            .filter(
                                entry_receive_attempt::attempt_state
                                    .eq(AttemptState::Cancelled.to_string())
                                    .or(entry_receive_attempt::attempt_state
                                        .eq(AttemptState::Failed.to_string())),
                            ),
                    )
                    .set((
                        entry_receive_attempt::current_attempt_id.eq(new_attempt_id),
                        entry_receive_attempt::attempt_state
                            .eq(AttemptState::Receiving.to_string()),
                        entry_receive_attempt::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                    if affected == 1 {
                        return Ok(BeginReceiveOutcome::Begun);
                    }
                    let current = entry_receive_attempt::table
                        .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                        .select((
                            entry_receive_attempt::current_attempt_id,
                            entry_receive_attempt::attempt_state,
                        ))
                        .first::<(String, String)>(conn)
                        .optional()?;
                    match current {
                        Some((current_id, _)) if current_id != expected_attempt_id => {
                            Ok(BeginReceiveOutcome::Superseded)
                        }
                        Some((_, state)) => {
                            let state = AttemptState::from_str(&state)
                                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
                            if state.is_terminal() {
                                Ok(BeginReceiveOutcome::Superseded)
                            } else {
                                Ok(BeginReceiveOutcome::AlreadyReceiving)
                            }
                        }
                        None => Ok(BeginReceiveOutcome::Superseded),
                    }
                })
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> ClaimReceiveCommitPort for DieselEntryReceiveAttemptRepository<E> {
    async fn claim_receive_commit(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<bool, AttemptError> {
        cas_state(
            &self.executor,
            entry_id,
            attempt_id,
            &[AttemptState::Receiving],
            AttemptState::Committing,
            now_ms,
        )
        .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> RequestReceiveCancellationPort for DieselEntryReceiveAttemptRepository<E> {
    async fn request_receive_cancellation(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<RequestReceiveCancellationOutcome, AttemptError> {
        if cas_state(
            &self.executor,
            entry_id,
            attempt_id,
            &[AttemptState::Receiving],
            AttemptState::Cancelling,
            now_ms,
        )
        .map_err(backend)?
        {
            return Ok(RequestReceiveCancellationOutcome::Requested);
        }
        let current = self.get_entry_attempt(entry_id).await?;
        Ok(match current {
            Some(current) if current.current_attempt_id != attempt_id => {
                RequestReceiveCancellationOutcome::Superseded
            }
            Some(current) => match current.state {
                AttemptState::Cancelling => RequestReceiveCancellationOutcome::AlreadyCancelling,
                AttemptState::Committing | AttemptState::Completed => {
                    RequestReceiveCancellationOutcome::TooLate
                }
                AttemptState::Failed | AttemptState::Cancelled => {
                    RequestReceiveCancellationOutcome::Terminal
                }
                AttemptState::Failing => RequestReceiveCancellationOutcome::AlreadyCancelling,
                AttemptState::Receiving => RequestReceiveCancellationOutcome::AlreadyCancelling,
            },
            None => RequestReceiveCancellationOutcome::Superseded,
        })
    }
}

#[async_trait]
impl<E: DbExecutor> BeginReceiveFailurePort for DieselEntryReceiveAttemptRepository<E> {
    async fn begin_receive_failure(
        &self,
        entry_id: &str,
        attempt_id: &str,
        now_ms: i64,
    ) -> Result<BeginReceiveFailureOutcome, AttemptError> {
        if cas_state(
            &self.executor,
            entry_id,
            attempt_id,
            &[AttemptState::Receiving, AttemptState::Committing],
            AttemptState::Failing,
            now_ms,
        )
        .map_err(backend)?
        {
            return Ok(BeginReceiveFailureOutcome::Begun);
        }
        let current = self.get_entry_attempt(entry_id).await?;
        Ok(match current {
            Some(current) if current.current_attempt_id != attempt_id => {
                BeginReceiveFailureOutcome::Superseded
            }
            Some(current) if current.state == AttemptState::Cancelling => {
                BeginReceiveFailureOutcome::CancellationWon
            }
            Some(current) if current.state.is_terminal() => BeginReceiveFailureOutcome::Terminal,
            Some(_) => BeginReceiveFailureOutcome::Terminal,
            None => BeginReceiveFailureOutcome::Superseded,
        })
    }
}

fn cas_state<E: DbExecutor>(
    executor: &E,
    entry_id: &str,
    attempt_id: &str,
    from: &[AttemptState],
    to: AttemptState,
    now_ms: i64,
) -> anyhow::Result<bool> {
    let entry_id = entry_id.to_owned();
    let attempt_id = attempt_id.to_owned();
    let from = from.iter().map(ToString::to_string).collect::<Vec<_>>();
    executor.run(move |conn| {
        let affected = diesel::update(
            entry_receive_attempt::table
                .filter(entry_receive_attempt::entry_id.eq(entry_id))
                .filter(entry_receive_attempt::current_attempt_id.eq(attempt_id))
                .filter(entry_receive_attempt::attempt_state.eq_any(from)),
        )
        .set((
            entry_receive_attempt::attempt_state.eq(to.to_string()),
            entry_receive_attempt::updated_at_ms.eq(now_ms),
        ))
        .execute(conn)?;
        Ok(affected == 1)
    })
}

#[async_trait]
impl<E: DbExecutor> DeleteReceiveStateForEntryPort for DieselEntryReceiveAttemptRepository<E> {
    async fn delete_receive_state_for_entry(&self, entry_id: &str) -> Result<bool, AttemptError> {
        let entry_id = entry_id.to_owned();
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    delete_receive_state(conn, &entry_id)
                })
            })
            .map_err(backend)
    }
}

#[async_trait]
impl<E: DbExecutor> PurgeTerminalOrphanAttemptsPort for DieselEntryReceiveAttemptRepository<E> {
    async fn purge_terminal_orphan_attempts(
        &self,
        older_than_ms: i64,
    ) -> Result<u32, AttemptError> {
        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    let terminal_states = [
                        AttemptState::Completed.to_string(),
                        AttemptState::Cancelled.to_string(),
                        AttemptState::Failed.to_string(),
                    ];
                    let candidates = entry_receive_attempt::table
                        .filter(entry_receive_attempt::attempt_state.eq_any(terminal_states))
                        .filter(entry_receive_attempt::updated_at_ms.lt(older_than_ms))
                        .select(entry_receive_attempt::entry_id)
                        .load::<String>(conn)?;
                    let mut purged = 0_u32;
                    for entry_id in candidates {
                        let has_entry = clipboard_entry::table
                            .filter(clipboard_entry::entry_id.eq(&entry_id))
                            .select(clipboard_entry::entry_id)
                            .first::<String>(conn)
                            .optional()?
                            .is_some();
                        let has_pending_artifacts = receive_artifact_log::table
                            .filter(receive_artifact_log::entry_id.eq(&entry_id))
                            .filter(receive_artifact_log::resolution.eq("pending"))
                            .select(receive_artifact_log::entry_id)
                            .first::<String>(conn)
                            .optional()?
                            .is_some();
                        if !has_entry
                            && !has_pending_artifacts
                            && delete_receive_state(conn, &entry_id)?
                        {
                            purged = purged.saturating_add(1);
                        }
                    }
                    Ok(purged)
                })
            })
            .map_err(backend)
    }
}

pub(crate) fn delete_receive_state(
    conn: &mut diesel::sqlite::SqliteConnection,
    entry_id: &str,
) -> anyhow::Result<bool> {
    let transfer_ids = file_transfer::table
        .filter(file_transfer::entry_id.eq(entry_id))
        .select(file_transfer::transfer_id)
        .load::<String>(conn)?;
    if !transfer_ids.is_empty() {
        diesel::delete(
            file_transfer_events::table
                .filter(file_transfer_events::transfer_id.eq_any(&transfer_ids)),
        )
        .execute(conn)?;
    }
    diesel::delete(file_transfer::table.filter(file_transfer::entry_id.eq(entry_id)))
        .execute(conn)?;
    diesel::delete(receive_artifact_log::table.filter(receive_artifact_log::entry_id.eq(entry_id)))
        .execute(conn)?;
    diesel::delete(
        directory_publish_log::table.filter(directory_publish_log::entry_id.eq(entry_id)),
    )
    .execute(conn)?;
    let deleted = diesel::delete(
        entry_receive_attempt::table.filter(entry_receive_attempt::entry_id.eq(entry_id)),
    )
    .execute(conn)?;
    Ok(deleted == 1)
}
