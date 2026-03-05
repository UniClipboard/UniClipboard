//! Clipboard Representation Repository
//!
//! Implements [`ClipboardRepresentationRepositoryPort`] for querying and updating
//! clipboard snapshot representations stored in SQLite.
//!
//! # Usage
//!
//! Create a repository with a database executor:
//!
//! ```ignore
//! let repo = DieselClipboardRepresentationRepository::new(executor);
//! ```
//!
//! Query a representation by event and representation ID:
//!
//! ```ignore
//! let rep = repo.get_representation(&event_id, &rep_id).await?;
//! ```
//!
//! Update blob_id after materialization:
//!
//! ```ignore
//! repo.update_blob_id(&rep_id, &blob_id).await?;
//! ```
//!
//! For concurrent pipelines, prefer `update_blob_id_if_none` to avoid overwriting
//! an existing blob_id written by another worker.

use crate::db::mappers::snapshot_representation_mapper::RepresentationRowMapper;
use crate::db::models::snapshot_representation::SnapshotRepresentationRow;
use crate::db::ports::{DbExecutor, RowMapper};
use crate::db::schema::clipboard_snapshot_representation;
use anyhow::Result;
use diesel::{BoolExpressionMethods, ExpressionMethods, OptionalExtension, QueryDsl, RunQueryDsl};
use uc_core::clipboard::{MimeType, PayloadAvailability, PersistedClipboardRepresentation};
use uc_core::ids::{EventId, RepresentationId};
use uc_core::ports::clipboard::{ClipboardRepresentationRepositoryPort, ProcessingUpdateOutcome};
use uc_core::BlobId;

pub struct DieselClipboardRepresentationRepository<E>
where
    E: DbExecutor,
{
    executor: E,
}

impl<E> DieselClipboardRepresentationRepository<E>
where
    E: DbExecutor,
{
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait::async_trait]
impl<E> ClipboardRepresentationRepositoryPort for DieselClipboardRepresentationRepository<E>
where
    E: DbExecutor,
{
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let event_id_str = event_id.to_string();
        let rep_id_str = representation_id.to_string();

        let row: Option<SnapshotRepresentationRow> = self.executor.run(|conn| {
            let result: Result<Option<SnapshotRepresentationRow>, diesel::result::Error> =
                clipboard_snapshot_representation::table
                    .filter(
                        clipboard_snapshot_representation::event_id
                            .eq(&event_id_str)
                            .and(clipboard_snapshot_representation::id.eq(&rep_id_str)),
                    )
                    .first::<SnapshotRepresentationRow>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        match row {
            Some(r) => {
                let mapper = RepresentationRowMapper;
                let rep = mapper.to_domain(&r)?;
                Ok(Some(rep))
            }
            None => Ok(None),
        }
    }

    async fn get_representation_by_id(
        &self,
        representation_id: &RepresentationId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let rep_id_str = representation_id.to_string();

        let row: Option<SnapshotRepresentationRow> = self.executor.run(|conn| {
            let result: Result<Option<SnapshotRepresentationRow>, diesel::result::Error> =
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::id.eq(&rep_id_str))
                    .first::<SnapshotRepresentationRow>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        match row {
            Some(r) => {
                let mapper = RepresentationRowMapper;
                let rep = mapper.to_domain(&r)?;
                Ok(Some(rep))
            }
            None => Ok(None),
        }
    }

    async fn get_representation_by_blob_id(
        &self,
        blob_id: &BlobId,
    ) -> Result<Option<PersistedClipboardRepresentation>> {
        let blob_id_str = blob_id.to_string();

        let row: Option<SnapshotRepresentationRow> = self.executor.run(|conn| {
            let result: Result<Option<SnapshotRepresentationRow>, diesel::result::Error> =
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::blob_id.eq(&blob_id_str))
                    .first::<SnapshotRepresentationRow>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        match row {
            Some(r) => {
                let mapper = RepresentationRowMapper;
                let rep = mapper.to_domain(&r)?;
                Ok(Some(rep))
            }
            None => Ok(None),
        }
    }

    async fn update_blob_id(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<()> {
        let rep_id_str = representation_id.to_string();
        let blob_id_str = blob_id.to_string();

        self.executor.run(|conn| {
            diesel::update(
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::id.eq(&rep_id_str)),
            )
            .set(clipboard_snapshot_representation::blob_id.eq(&blob_id_str))
            .execute(conn)?;
            Ok(())
        })
    }

    async fn update_blob_id_if_none(
        &self,
        representation_id: &RepresentationId,
        blob_id: &BlobId,
    ) -> Result<bool> {
        let rep_id_str = representation_id.to_string();
        let blob_id_str = blob_id.to_string();

        let updated_rows = self.executor.run(|conn| {
            let result: diesel::result::QueryResult<usize> = diesel::update(
                clipboard_snapshot_representation::table.filter(
                    clipboard_snapshot_representation::id
                        .eq(&rep_id_str)
                        .and(clipboard_snapshot_representation::blob_id.is_null()),
                ),
            )
            .set(clipboard_snapshot_representation::blob_id.eq(&blob_id_str))
            .execute(conn);
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        Ok(updated_rows > 0)
    }

    async fn update_processing_result(
        &self,
        rep_id: &RepresentationId,
        expected_states: &[PayloadAvailability],
        blob_id: Option<&BlobId>,
        new_state: PayloadAvailability,
        last_error: Option<&str>,
    ) -> Result<ProcessingUpdateOutcome> {
        let rep_id_str = rep_id.to_string();
        let expected_state_strs: Vec<String> = expected_states
            .iter()
            .map(|s| s.as_str().to_string())
            .collect();

        // First, verify the representation exists and get event_id
        let event_id_str: Option<String> = self.executor.run(|conn| {
            let result: Result<Option<String>, diesel::result::Error> =
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::id.eq(&rep_id_str))
                    .select(clipboard_snapshot_representation::event_id)
                    .first::<String>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        if event_id_str.is_none() {
            return Ok(ProcessingUpdateOutcome::NotFound);
        }

        // Perform the CAS update with all fields set in one statement
        let updated_rows = self.executor.run(|conn| {
            let base_filter = clipboard_snapshot_representation::table.filter(
                clipboard_snapshot_representation::id.eq(&rep_id_str).and(
                    clipboard_snapshot_representation::payload_state.eq_any(&expected_state_strs),
                ),
            );

            // Build the update statement with all fields in one set() call
            let update_result = if let Some(blob_id) = blob_id {
                // Keep DB constraint satisfied: inline_data and blob_id cannot coexist.
                diesel::update(base_filter)
                    .set((
                        clipboard_snapshot_representation::payload_state.eq(new_state.as_str()),
                        clipboard_snapshot_representation::last_error.eq(last_error),
                        clipboard_snapshot_representation::inline_data.eq::<Option<Vec<u8>>>(None),
                        clipboard_snapshot_representation::blob_id.eq(blob_id.to_string()),
                    ))
                    .execute(conn)
            } else {
                diesel::update(base_filter)
                    .set((
                        clipboard_snapshot_representation::payload_state.eq(new_state.as_str()),
                        clipboard_snapshot_representation::last_error.eq(last_error),
                    ))
                    .execute(conn)
            };

            update_result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        if updated_rows == 0 {
            return Ok(ProcessingUpdateOutcome::StateMismatch);
        }

        // Fetch and return the updated representation
        let updated: Option<SnapshotRepresentationRow> = self.executor.run(|conn| {
            let result: Result<Option<SnapshotRepresentationRow>, diesel::result::Error> =
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::id.eq(&rep_id_str))
                    .first::<SnapshotRepresentationRow>(conn)
                    .optional();
            result.map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        let row = updated.ok_or_else(|| {
            anyhow::anyhow!("Representation disappeared after update: {}", rep_id_str)
        })?;

        let mapper = RepresentationRowMapper;
        let representation = mapper.to_domain(&row)?;
        Ok(ProcessingUpdateOutcome::Updated(representation))
    }

    async fn update_mime_type(&self, rep_id: &RepresentationId, mime: &MimeType) -> Result<()> {
        let rep_id_str = rep_id.to_string();
        let mime_str = mime.as_str().to_string();

        self.executor.run(|conn| {
            diesel::update(
                clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::id.eq(&rep_id_str)),
            )
            .set(clipboard_snapshot_representation::mime_type.eq(Some(&mime_str)))
            .execute(conn)?;
            Ok(())
        })
    }
}
