use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use diesel::prelude::*;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::DeriveSpaceSubkeyPort;
use uc_core::ports::AttemptState;
use uc_core::ports::{
    CommitInboundReceivePort, CompletedReceiveArtifacts, InboundReceiveCommitError,
    InboundReceiveRecord, InboundReceiveSettlement, NoEntryReceiveArtifacts,
    PartialReceiveArtifacts, PartialReceiveTerminal,
};

use super::clipboard_entry_repo::insert_entry_and_selection_rows;
use super::clipboard_event_repo::insert_clipboard_event_rows;
use super::entry_file_set_repo::{
    derive_file_set_cipher, encode_file_set_rows, replace_entry_file_set_rows,
};
use super::entry_replace_repo::{
    prepare_entry_replacement, replace_entry_content_rows, PreparedEntryReplacement,
};
use crate::db::mappers::clipboard_entry_mapper::ClipboardEntryRowMapper;
use crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
use crate::db::mappers::clipboard_selection_mapper::ClipboardSelectionRowMapper;
use crate::db::mappers::snapshot_representation_mapper::RepresentationRowMapper;
use crate::db::models::snapshot_representation::NewSnapshotRepresentationRow;
use crate::db::models::{NewClipboardEntryRow, NewClipboardEventRow, NewClipboardSelectionRow};
use crate::db::ports::{DbExecutor, InsertMapper};
use crate::db::schema::{directory_publish_log, entry_receive_attempt, receive_artifact_log};

pub struct DieselInboundReceiveCommitRepository<E> {
    executor: E,
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselInboundReceiveCommitRepository<E> {
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

enum PreparedRecord {
    Create {
        entry: NewClipboardEntryRow,
        event: NewClipboardEventRow,
        representations: Vec<NewSnapshotRepresentationRow>,
        selection: NewClipboardSelectionRow,
    },
    Replace(PreparedEntryReplacement),
}

#[derive(Clone, Copy)]
enum ArtifactAction {
    None,
    Landed,
    RolledBack,
    DirectoryPublished,
}

#[derive(Debug, thiserror::Error)]
enum CommitInvariantError {
    #[error("attempt state mismatch")]
    AttemptStateMismatch,
    #[error("artifact record missing")]
    ArtifactRecordMissing,
    #[error("directory publish record missing")]
    DirectoryPublishRecordMissing,
}

fn map_error(error: anyhow::Error) -> InboundReceiveCommitError {
    match error.downcast_ref::<CommitInvariantError>() {
        Some(CommitInvariantError::AttemptStateMismatch) => {
            InboundReceiveCommitError::AttemptStateMismatch
        }
        Some(CommitInvariantError::ArtifactRecordMissing) => {
            InboundReceiveCommitError::ArtifactRecordMissing
        }
        Some(CommitInvariantError::DirectoryPublishRecordMissing) => {
            InboundReceiveCommitError::DirectoryPublishRecordMissing
        }
        None => InboundReceiveCommitError::Backend(error.to_string()),
    }
}

fn prepare_record(record: &InboundReceiveRecord) -> Result<PreparedRecord> {
    match record {
        InboundReceiveRecord::Create {
            entry,
            event,
            representations,
            selection,
        } => Ok(PreparedRecord::Create {
            entry: ClipboardEntryRowMapper.to_row(entry)?,
            event: ClipboardEventRowMapper.to_row(event)?,
            representations: representations
                .iter()
                .map(|representation| {
                    RepresentationRowMapper.to_row(&(representation, &event.event_id))
                })
                .collect::<Result<Vec<_>>>()?,
            selection: ClipboardSelectionRowMapper.to_row(selection)?,
        }),
        InboundReceiveRecord::Replace {
            entry_id,
            new_event,
            new_representations,
            new_selection,
            new_total_size,
            new_content_category,
        } => Ok(PreparedRecord::Replace(prepare_entry_replacement(
            entry_id,
            new_event,
            new_representations,
            new_selection,
            *new_total_size,
            *new_content_category,
        )?)),
    }
}

#[async_trait]
impl<E: DbExecutor> CommitInboundReceivePort for DieselInboundReceiveCommitRepository<E> {
    async fn commit_inbound_receive(
        &self,
        settlement: &InboundReceiveSettlement,
    ) -> Result<(), InboundReceiveCommitError> {
        let (record, entry_id, attempt_id, file_set, expected, target, artifacts, now_ms) =
            match settlement {
                InboundReceiveSettlement::Complete {
                    record,
                    attempt_id,
                    file_set,
                    artifacts,
                    now_ms,
                } => (
                    Some(record),
                    record.entry_id().to_string(),
                    attempt_id.clone(),
                    file_set.as_ref(),
                    AttemptState::Committing,
                    AttemptState::Completed,
                    match artifacts {
                        CompletedReceiveArtifacts::None => ArtifactAction::None,
                        CompletedReceiveArtifacts::Landed => ArtifactAction::Landed,
                        CompletedReceiveArtifacts::DirectoryPublished => {
                            ArtifactAction::DirectoryPublished
                        }
                    },
                    *now_ms,
                ),
                InboundReceiveSettlement::Partial {
                    record,
                    attempt_id,
                    terminal,
                    file_set,
                    artifacts,
                    now_ms,
                } => (
                    Some(record),
                    record.entry_id().to_string(),
                    attempt_id.clone(),
                    file_set.as_ref(),
                    terminal_expected(*terminal),
                    terminal_target(*terminal),
                    match artifacts {
                        PartialReceiveArtifacts::None => ArtifactAction::None,
                        PartialReceiveArtifacts::Landed => ArtifactAction::Landed,
                        PartialReceiveArtifacts::RolledBack => ArtifactAction::RolledBack,
                    },
                    *now_ms,
                ),
                InboundReceiveSettlement::NoEntry {
                    entry_id,
                    attempt_id,
                    terminal,
                    artifacts,
                    now_ms,
                } => (
                    None,
                    entry_id.to_string(),
                    attempt_id.clone(),
                    None,
                    terminal_expected(*terminal),
                    terminal_target(*terminal),
                    match artifacts {
                        NoEntryReceiveArtifacts::None => ArtifactAction::None,
                        NoEntryReceiveArtifacts::RolledBack => ArtifactAction::RolledBack,
                    },
                    *now_ms,
                ),
            };

        let prepared_record = record
            .map(prepare_record)
            .transpose()
            .map_err(|error| InboundReceiveCommitError::Backend(error.to_string()))?;
        let file_set_rows = if let Some(file_set) = file_set {
            let cipher =
                derive_file_set_cipher(self.derive_subkey.as_ref(), self.current_profile.as_ref())
                    .await
                    .map_err(|error| InboundReceiveCommitError::Backend(error.to_string()))?;
            Some(
                encode_file_set_rows(
                    &cipher,
                    &uc_core::ids::EntryId::from(entry_id.as_str()),
                    file_set,
                )
                .map_err(|error| InboundReceiveCommitError::Backend(error.to_string()))?,
            )
        } else {
            None
        };

        self.executor
            .run(move |conn| {
                conn.transaction::<_, anyhow::Error, _>(|conn| {
                    if let Some(prepared_record) = &prepared_record {
                        match prepared_record {
                            PreparedRecord::Create {
                                entry,
                                event,
                                representations,
                                selection,
                            } => {
                                insert_clipboard_event_rows(conn, event, representations)?;
                                insert_entry_and_selection_rows(conn, entry, selection)?;
                            }
                            PreparedRecord::Replace(prepared) => {
                                replace_entry_content_rows(conn, prepared, true)?;
                            }
                        }
                    }
                    if let Some(file_set_rows) = &file_set_rows {
                        replace_entry_file_set_rows(conn, &entry_id, file_set_rows)?;
                    }

                    let attempt_updated = diesel::update(
                        entry_receive_attempt::table
                            .filter(entry_receive_attempt::entry_id.eq(&entry_id))
                            .filter(entry_receive_attempt::current_attempt_id.eq(&attempt_id))
                            .filter(entry_receive_attempt::attempt_state.eq(expected.to_string())),
                    )
                    .set((
                        entry_receive_attempt::attempt_state.eq(target.to_string()),
                        entry_receive_attempt::updated_at_ms.eq(now_ms),
                    ))
                    .execute(conn)?;
                    if attempt_updated != 1 {
                        return Err(CommitInvariantError::AttemptStateMismatch.into());
                    }

                    settle_artifacts(conn, &entry_id, &attempt_id, artifacts, now_ms)?;
                    Ok(())
                })
            })
            .map_err(map_error)
    }
}

fn terminal_expected(terminal: PartialReceiveTerminal) -> AttemptState {
    match terminal {
        PartialReceiveTerminal::Cancelled => AttemptState::Cancelling,
        PartialReceiveTerminal::Failed => AttemptState::Failing,
    }
}

fn terminal_target(terminal: PartialReceiveTerminal) -> AttemptState {
    match terminal {
        PartialReceiveTerminal::Cancelled => AttemptState::Cancelled,
        PartialReceiveTerminal::Failed => AttemptState::Failed,
    }
}

fn settle_artifacts(
    conn: &mut diesel::sqlite::SqliteConnection,
    entry_id: &str,
    attempt_id: &str,
    action: ArtifactAction,
    now_ms: i64,
) -> Result<()> {
    match action {
        ArtifactAction::None => Ok(()),
        ArtifactAction::Landed => {
            settle_generic_artifacts(conn, entry_id, attempt_id, "landed", now_ms)
        }
        ArtifactAction::RolledBack => {
            settle_generic_artifacts(conn, entry_id, attempt_id, "rolled_back", now_ms)
        }
        ArtifactAction::DirectoryPublished => {
            let updated = diesel::update(
                directory_publish_log::table
                    .filter(directory_publish_log::entry_id.eq(entry_id))
                    .filter(directory_publish_log::attempt_id.eq(attempt_id)),
            )
            .set((
                directory_publish_log::phase.eq("landed"),
                directory_publish_log::landed.eq(true),
                directory_publish_log::updated_at_ms.eq(now_ms),
            ))
            .execute(conn)?;
            if updated != 1 {
                return Err(CommitInvariantError::DirectoryPublishRecordMissing.into());
            }
            Ok(())
        }
    }
}

fn settle_generic_artifacts(
    conn: &mut diesel::sqlite::SqliteConnection,
    entry_id: &str,
    attempt_id: &str,
    resolution: &str,
    now_ms: i64,
) -> Result<()> {
    let updated = diesel::update(
        receive_artifact_log::table
            .filter(receive_artifact_log::entry_id.eq(entry_id))
            .filter(receive_artifact_log::attempt_id.eq(attempt_id)),
    )
    .set((
        receive_artifact_log::phase.eq("settled"),
        receive_artifact_log::resolution.eq(resolution),
        receive_artifact_log::updated_at_ms.eq(now_ms),
    ))
    .execute(conn)?;
    if updated != 1 {
        return Err(CommitInvariantError::ArtifactRecordMissing.into());
    }
    Ok(())
}
