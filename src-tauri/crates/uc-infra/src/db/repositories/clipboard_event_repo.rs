use crate::db::{
    models::{
        clipboard_event::NewClipboardEventRow,
        snapshot_representation::{NewSnapshotRepresentationRow, SnapshotRepresentationRow},
    },
    ports::{DbExecutor, InsertMapper, RowMapper},
    schema::{clipboard_event, clipboard_snapshot_representation},
};
use anyhow::Result;
use diesel::prelude::*;
use tracing::debug_span;
use uc_core::{
    clipboard::{ClipboardEvent, PersistedClipboardRepresentation},
    ids::EventId,
    ports::{ClipboardEventRepositoryPort, ClipboardEventWriterPort},
};

pub struct DieselClipboardEventRepository<E, ME, MS> {
    executor: E,
    event_mapper: ME,
    snapshot_mapper: MS,
}

impl<E, ME, MS> DieselClipboardEventRepository<E, ME, MS> {
    pub fn new(executor: E, event_mapper: ME, snapshot_mapper: MS) -> Self {
        Self {
            executor,
            event_mapper,
            snapshot_mapper,
        }
    }
}

#[async_trait::async_trait]
impl<E, ME, MS> ClipboardEventWriterPort for DieselClipboardEventRepository<E, ME, MS>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEvent, NewClipboardEventRow>,
    for<'a> MS: InsertMapper<
        (&'a PersistedClipboardRepresentation, &'a EventId),
        NewSnapshotRepresentationRow,
    >,
{
    /// Inserts a clipboard event and all its snapshot representations in a single database transaction.
    ///
    /// Converts the provided event and each persisted representation to their corresponding database rows and persists them; if any conversion or insert fails, the whole transaction is rolled back.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_core::{ClipboardEvent, PersistedClipboardRepresentation};
    /// # use uc_core::ports::ClipboardEventWriterPort;
    /// # async fn example(
    /// #     repo: &impl ClipboardEventWriterPort,
    /// #     event: &ClipboardEvent,
    /// #     reps: &Vec<PersistedClipboardRepresentation>,
    /// # ) -> anyhow::Result<()> {
    /// repo.insert_event(event, reps).await?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Returns
    ///
    /// `Ok(())` on success, `Err` if mapping or database operations fail.
    async fn insert_event(
        &self,
        event: &ClipboardEvent,
        reps: &Vec<PersistedClipboardRepresentation>,
    ) -> Result<()> {
        let span = debug_span!(
            "infra.sqlite.insert_clipboard_event",
            table = "clipboard_event",
            event_id = %event.event_id,
        );
        span.in_scope(|| {
            let new_event: NewClipboardEventRow = self.event_mapper.to_row(event)?;
            let new_reps: Vec<NewSnapshotRepresentationRow> = reps
                .iter()
                .map(|rep| self.snapshot_mapper.to_row(&(rep, &event.event_id)))
                .collect::<Result<Vec<_>, _>>()?;

            self.executor.run(|conn| {
                conn.transaction(|conn| {
                    diesel::insert_into(clipboard_event::table)
                        .values(&new_event)
                        .execute(conn)?;

                    for rep in new_reps {
                        diesel::insert_into(clipboard_snapshot_representation::table)
                            .values(&rep)
                            .execute(conn)?;
                    }

                    Ok(())
                })
            })
        })
    }

    /// Deletes the clipboard event and all associated snapshot representations for the given event ID.
    ///
    /// The deletions are performed inside a single database transaction: snapshot representations referencing
    /// the event are removed first, then the event row itself is deleted.
    ///
    /// # Returns
    ///
    /// `Ok(())` if the deletion succeeds, `Err` if a database error prevents the operation.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_core::ids::EventId;
    /// # use uc_core::ports::ClipboardEventWriterPort;
    /// # async fn run_example(
    /// #     repo: &impl ClipboardEventWriterPort,
    /// #     event_id: &EventId,
    /// # ) -> anyhow::Result<()> {
    /// repo.delete_event_and_representations(event_id).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn delete_event_and_representations(&self, event_id: &EventId) -> Result<()> {
        let span = debug_span!(
            "infra.sqlite.delete_clipboard_event",
            table = "clipboard_event",
            event_id = %event_id,
        );
        span.in_scope(|| {
            let event_id_str = event_id.to_string();
            self.executor.run(|conn| {
                conn.transaction(|conn| {
                    // Delete representations first (they reference the event)
                    diesel::delete(clipboard_snapshot_representation::table)
                        .filter(clipboard_snapshot_representation::event_id.eq(&event_id_str))
                        .execute(conn)?;

                    // Then delete the event
                    diesel::delete(clipboard_event::table)
                        .filter(clipboard_event::event_id.eq(&event_id_str))
                        .execute(conn)?;

                    Ok(())
                })
            })
        })
    }
}

#[async_trait::async_trait]
impl<E, ME, MS> ClipboardEventRepositoryPort for DieselClipboardEventRepository<E, ME, MS>
where
    E: DbExecutor,
    ME: Send + Sync,
    MS: RowMapper<SnapshotRepresentationRow, PersistedClipboardRepresentation> + Send + Sync,
{
    async fn get_representation(
        &self,
        event_id: &EventId,
        representation_id: &str,
    ) -> Result<uc_core::ObservedClipboardRepresentation> {
        let span = debug_span!(
            "infra.sqlite.query_snapshot_representation",
            table = "snapshot_representation",
            event_id = %event_id,
            representation_id = representation_id,
        );
        let rep_row = span.in_scope(|| {
            use crate::db::schema::clipboard_snapshot_representation;

            let event_id_str = event_id.as_ref().to_string();
            let rep_id_str = representation_id.to_string();

            self.executor
                .run(|conn| {
                    let rep = clipboard_snapshot_representation::table
                        .filter(clipboard_snapshot_representation::event_id.eq(&event_id_str))
                        .filter(clipboard_snapshot_representation::id.eq(&rep_id_str))
                        .first::<SnapshotRepresentationRow>(conn)
                        .map_err(|e| anyhow::anyhow!("Failed to fetch representation: {}", e))?;
                    Ok(rep)
                })
                .map_err(|e| anyhow::anyhow!("Database error: {}", e))
        })?;

        // Convert from PersistedClipboardRepresentation to ObservedClipboardRepresentation
        let persisted = self.snapshot_mapper.to_domain(&rep_row)?;
        Ok(uc_core::ObservedClipboardRepresentation::new(
            persisted.id,
            persisted.format_id,
            persisted.mime_type,
            persisted.inline_data.unwrap_or_default(),
        ))
    }

    async fn get_source_device(
        &self,
        event_id: &EventId,
    ) -> Result<Option<uc_core::ids::DeviceId>> {
        use crate::db::schema::clipboard_event;

        let event_id_str = event_id.as_ref().to_string();
        let source: Option<String> = self.executor.run(move |conn| {
            Ok(clipboard_event::table
                .filter(clipboard_event::event_id.eq(&event_id_str))
                .select(clipboard_event::source_device)
                .first::<String>(conn)
                .optional()?)
        })?;
        Ok(source.map(uc_core::ids::DeviceId::new))
    }
}
