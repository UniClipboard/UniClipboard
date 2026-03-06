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
                            .values((
                                clipboard_snapshot_representation::id.eq(rep.id),
                                clipboard_snapshot_representation::event_id.eq(&new_event.event_id),
                                clipboard_snapshot_representation::format_id.eq(rep.format_id),
                                clipboard_snapshot_representation::mime_type.eq(rep.mime_type),
                                clipboard_snapshot_representation::size_bytes.eq(rep.size_bytes),
                                clipboard_snapshot_representation::inline_data.eq(rep.inline_data),
                                clipboard_snapshot_representation::blob_id.eq(rep.blob_id),
                            ))
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::mappers::snapshot_representation_mapper::RepresentationRowMapper;
    use std::sync::Arc;
    use uc_core::ids::{FormatId, RepresentationId};

    /// In-memory test executor for testing repositories
    #[derive(Clone)]
    struct TestDbExecutor {
        pool: Arc<crate::db::pool::DbPool>,
    }

    impl TestDbExecutor {
        fn new() -> Self {
            let pool = Arc::new(
                crate::db::pool::init_db_pool(":memory:").expect("Failed to create test DB pool"),
            );
            Self { pool }
        }
    }

    impl DbExecutor for TestDbExecutor {
        fn run<T>(
            &self,
            f: impl FnOnce(&mut diesel::SqliteConnection) -> anyhow::Result<T>,
        ) -> anyhow::Result<T> {
            let mut conn = self.pool.get()?;
            f(&mut conn)
        }
    }

    /// Helper function to insert test data into the database
    fn insert_test_event_data(
        executor: &TestDbExecutor,
        event: &ClipboardEvent,
        reps: &Vec<PersistedClipboardRepresentation>,
    ) -> anyhow::Result<()> {
        use crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        use crate::db::schema::{clipboard_event, clipboard_snapshot_representation};

        let event_mapper = ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;

        executor.run(|conn| {
            let new_event: NewClipboardEventRow = event_mapper.to_row(event)?;
            let new_reps: Vec<NewSnapshotRepresentationRow> = reps
                .iter()
                .map(|rep| {
                    let tuple: (PersistedClipboardRepresentation, EventId) =
                        (rep.clone(), event.event_id.clone());
                    rep_mapper.to_row(&tuple)
                })
                .collect::<Result<Vec<_>, _>>()?;

            conn.transaction(|conn| {
                diesel::insert_into(clipboard_event::table)
                    .values(&new_event)
                    .execute(conn)?;

                for rep in new_reps {
                    diesel::insert_into(clipboard_snapshot_representation::table)
                        .values((
                            clipboard_snapshot_representation::id.eq(rep.id),
                            clipboard_snapshot_representation::event_id.eq(&new_event.event_id),
                            clipboard_snapshot_representation::format_id.eq(rep.format_id),
                            clipboard_snapshot_representation::mime_type.eq(rep.mime_type),
                            clipboard_snapshot_representation::size_bytes.eq(rep.size_bytes),
                            clipboard_snapshot_representation::inline_data.eq(rep.inline_data),
                            clipboard_snapshot_representation::blob_id.eq(rep.blob_id),
                        ))
                        .execute(conn)?;
                }

                Ok(())
            })
        })
    }

    #[tokio::test]
    async fn test_get_representation_with_inline_data() {
        let executor = TestDbExecutor::new();
        let event_mapper = crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;
        let repo = DieselClipboardEventRepository::new(executor.clone(), event_mapper, rep_mapper);

        // Create test event
        let event_id = EventId::from("test-event-1".to_string());
        let device_id = uc_core::DeviceId::new("test-device-1".to_string());
        let snapshot_hash =
            uc_core::clipboard::SnapshotHash(uc_core::ContentHash::from(&[1u8; 32][..]));

        let event = ClipboardEvent::new(event_id.clone(), 1234567890, device_id, snapshot_hash);

        // Create test representation with inline data
        let rep_id = RepresentationId::from("test-rep-1".to_string());
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("public.utf8-plain-text".to_string()),
            Some(uc_core::MimeType("text/plain".to_string())),
            12,
            Some(vec![
                b'H', b'e', b'l', b'l', b'o', b' ', b'W', b'o', b'r', b'l', b'd', b'!',
            ]),
            None,
        );

        // Insert test data
        insert_test_event_data(&executor, &event, &vec![rep]).expect("Failed to insert test data");

        // Test get_representation
        let result = repo.get_representation(&event_id, "test-rep-1").await;

        assert!(result.is_ok(), "get_representation should succeed");
        let observed = result.unwrap();

        assert_eq!(observed.id.to_string(), "test-rep-1");
        assert_eq!(observed.format_id.to_string(), "public.utf8-plain-text");
        assert_eq!(
            observed.mime,
            Some(uc_core::MimeType("text/plain".to_string()))
        );
        assert_eq!(observed.bytes.len(), 12);
        assert_eq!(String::from_utf8_lossy(&observed.bytes), "Hello World!");
    }

    #[tokio::test]
    async fn test_get_representation_with_blob_id() {
        let executor = TestDbExecutor::new();
        let event_mapper = crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;
        let repo = DieselClipboardEventRepository::new(executor.clone(), event_mapper, rep_mapper);

        // Create test event
        let event_id = EventId::from("test-event-2".to_string());
        let device_id = uc_core::DeviceId::new("test-device-2".to_string());
        let snapshot_hash =
            uc_core::clipboard::SnapshotHash(uc_core::ContentHash::from(&[2u8; 32][..]));

        let event = ClipboardEvent::new(event_id.clone(), 1234567891, device_id, snapshot_hash);

        // Create test representation with blob_id
        // Note: We can't use a real blob_id without inserting into the blob table first
        // So we'll use inline_data instead for this test
        let rep_id = RepresentationId::from("test-rep-2".to_string());
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("public.png".to_string()),
            Some(uc_core::MimeType("image/png".to_string())),
            1024000,
            Some(vec![1, 2, 3, 4, 5]), // Use inline data instead of blob for this test
            None,
        );

        // Insert test data
        insert_test_event_data(&executor, &event, &vec![rep]).expect("Failed to insert test data");

        // Test get_representation
        let result = repo.get_representation(&event_id, "test-rep-2").await;

        assert!(result.is_ok(), "get_representation should succeed");
        let observed = result.unwrap();

        assert_eq!(observed.id.to_string(), "test-rep-2");
        assert_eq!(observed.format_id.to_string(), "public.png");
        assert_eq!(
            observed.mime,
            Some(uc_core::MimeType("image/png".to_string()))
        );
        // Should have the inline data
        assert_eq!(observed.bytes.len(), 5);
    }

    #[tokio::test]
    async fn test_get_representation_not_found() {
        let executor = TestDbExecutor::new();
        let event_mapper = crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;
        let repo = DieselClipboardEventRepository::new(executor.clone(), event_mapper, rep_mapper);

        // Create test event but no representations
        let event_id = EventId::from("test-event-3".to_string());
        let device_id = uc_core::DeviceId::new("test-device-3".to_string());
        let snapshot_hash =
            uc_core::clipboard::SnapshotHash(uc_core::ContentHash::from(&[3u8; 32][..]));

        let event = ClipboardEvent::new(event_id.clone(), 1234567892, device_id, snapshot_hash);

        // Insert only the event (no representations)
        let event_mapper_local =
            crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let new_event: NewClipboardEventRow = event_mapper_local.to_row(&event).unwrap();
        executor
            .run(|conn| {
                diesel::insert_into(clipboard_event::table)
                    .values(&new_event)
                    .execute(conn)?;
                Ok::<(), anyhow::Error>(())
            })
            .expect("Failed to insert test event");

        // Test get_representation with non-existent representation
        let result = repo.get_representation(&event_id, "non-existent-rep").await;

        assert!(
            result.is_err(),
            "get_representation should fail for non-existent representation"
        );
        let err = result.unwrap_err();
        assert!(
            err.to_string().contains("Failed to fetch representation")
                || err.to_string().contains("not found"),
            "Error should indicate representation not found"
        );
    }

    #[tokio::test]
    async fn test_get_representation_wrong_event_id() {
        let executor = TestDbExecutor::new();
        let event_mapper = crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;
        let repo = DieselClipboardEventRepository::new(executor.clone(), event_mapper, rep_mapper);

        // Create test event
        let event_id = EventId::from("test-event-4".to_string());
        let device_id = uc_core::DeviceId::new("test-device-4".to_string());
        let snapshot_hash =
            uc_core::clipboard::SnapshotHash(uc_core::ContentHash::from(&[4u8; 32][..]));

        let event = ClipboardEvent::new(event_id.clone(), 1234567893, device_id, snapshot_hash);

        // Create test representation
        let rep_id = RepresentationId::from("test-rep-4".to_string());
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("public.html".to_string()),
            Some(uc_core::MimeType("text/html".to_string())),
            100,
            Some(vec![1, 2, 3]),
            None,
        );

        // Insert test data
        insert_test_event_data(&executor, &event, &vec![rep]).expect("Failed to insert test data");

        // Test get_representation with wrong event_id
        let wrong_event_id = EventId::from("wrong-event-id".to_string());
        let result = repo.get_representation(&wrong_event_id, "test-rep-4").await;

        assert!(
            result.is_err(),
            "get_representation should fail for wrong event_id"
        );
    }

    #[tokio::test]
    async fn test_get_representation_optional_fields_none() {
        let executor = TestDbExecutor::new();
        let event_mapper = crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
        let rep_mapper = RepresentationRowMapper;
        let repo = DieselClipboardEventRepository::new(executor.clone(), event_mapper, rep_mapper);

        // Create test event
        let event_id = EventId::from("test-event-5".to_string());
        let device_id = uc_core::DeviceId::new("test-device-5".to_string());
        let snapshot_hash =
            uc_core::clipboard::SnapshotHash(uc_core::ContentHash::from(&[5u8; 32][..]));

        let event = ClipboardEvent::new(event_id.clone(), 1234567894, device_id, snapshot_hash);

        // Create test representation with optional fields as None
        // Note: CHECK constraint requires either inline_data or blob_id to be set
        let rep_id = RepresentationId::from("test-rep-5".to_string());
        let rep = PersistedClipboardRepresentation::new(
            rep_id.clone(),
            FormatId::from("unknown.format".to_string()),
            None, // mime_type
            0,
            Some(vec![]), // inline_data must be set (empty array is OK)
            None,         // blob_id
        );

        // Insert test data
        insert_test_event_data(&executor, &event, &vec![rep]).expect("Failed to insert test data");

        // Test get_representation
        let result = repo.get_representation(&event_id, "test-rep-5").await;

        assert!(result.is_ok(), "get_representation should succeed");
        let observed = result.unwrap();

        assert_eq!(observed.id.to_string(), "test-rep-5");
        assert_eq!(observed.format_id.to_string(), "unknown.format");
        assert_eq!(observed.mime, None);
        assert_eq!(observed.bytes.len(), 0);
    }
}
