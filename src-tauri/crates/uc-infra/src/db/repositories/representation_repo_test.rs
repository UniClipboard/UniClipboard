use super::*;
use crate::db::models::snapshot_representation::NewSnapshotRepresentationRow;
use crate::db::ports::DbExecutor;
use crate::db::schema::{blob, clipboard_event, clipboard_snapshot_representation};
use anyhow::Result;
use diesel::{ExpressionMethods, RunQueryDsl};
use std::sync::Arc;
use uc_core::clipboard::PayloadAvailability;
use uc_core::ids::RepresentationId;
use uc_core::ports::clipboard::{ClipboardRepresentationRepositoryPort, ProcessingUpdateOutcome};

/// In-memory test executor for testing repositories.
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

// Note: This requires a test database setup.
// For now, we provide the test structure that can be run with proper test DB.
// Actual execution requires test container or in-memory SQLite setup.

#[tokio::test]
async fn test_get_representation_found() {
    // TODO: Set up test database connection
    // This test requires DbExecutor implementation for testing

    // let executor = TestDbExecutor::new();
    // let repo = DieselClipboardRepresentationRepository::new(executor);

    // // Insert test data
    // executor.run(|conn| {
    //     diesel::insert_into(clipboard_snapshot_representation::table)
    //         .values(&NewSnapshotRepresentationRow {
    //             id: "test-rep-1".to_string(),
    //             event_id: "test-event-1".to_string(),
    //             format_id: "public.text".to_string(),
    //             mime_type: Some("text/plain".to_string()),
    //             size_bytes: 10,
    //             inline_data: Some(vec![1, 2, 3]),
    //             blob_id: None,
    //             payload_state: "Inline".to_string(),
    //             last_error: None,
    //         })
    //         .execute(conn)
    //         .unwrap();
    // });

    // let result = repo
    //     .get_representation(
    //         &EventId::from("test-event-1".to_string()),
    //         &RepresentationId::from("test-rep-1".to_string()),
    //     )
    //     .await
    //     .unwrap();

    // assert!(result.is_some());
    // let rep = result.unwrap();
    // assert_eq!(rep.format_id.to_string(), "public.text");
}

#[tokio::test]
async fn test_get_representation_not_found() {
    // TODO: Set up test database
    // Test that Ok(None) is returned for non-existent representation
}

#[tokio::test]
async fn test_update_blob_id() {
    // TODO: Set up test database
    // Test that blob_id is correctly updated
}

#[tokio::test]
async fn test_update_processing_result_cas() {
    // TODO: Set up test database
    // Test CAS semantics:
    // 1. Create representation with state=Staged
    // 2. Call update_processing_result with expected_states=[Staged, Processing]
    // 3. Should succeed and return updated representation with new state
    // 4. Call again with expected_states=[Staged] (but state is now BlobReady)
    // 5. Should fail with CAS error

    // Example test structure:
    // let executor = TestDbExecutor::new();
    // let repo = DieselClipboardRepresentationRepository::new(executor);
    // let rep_id = RepresentationId::new();
    // let blob_id = BlobId::new();
    //
    // // Insert Staged representation
    // ...
    //
    // // Should succeed - state is Staged
    // let result = repo.update_processing_result(
    //     &rep_id,
    //     &[PayloadAvailability::Staged, PayloadAvailability::Processing],
    //     Some(&blob_id),
    //     PayloadAvailability::BlobReady,
    //     None,
    // ).await.unwrap();
    //
    // assert_eq!(result.payload_state(), PayloadAvailability::BlobReady);
    // assert_eq!(result.blob_id, Some(blob_id));
    //
    // // Should fail - state is now BlobReady, not in expected states
    // let err = repo.update_processing_result(
    //     &rep_id,
    //     &[PayloadAvailability::Staged],
    //     None,
    //     PayloadAvailability::Lost,
    //     Some("test error"),
    // ).await.unwrap_err();
    //
    // assert!(err.to_string().contains("CAS update failed"));
}

#[tokio::test]
async fn test_update_processing_result_returns_state_mismatch() -> Result<()> {
    let executor = TestDbExecutor::new();
    let repo = DieselClipboardRepresentationRepository::new(executor.clone());
    let rep_id = RepresentationId::new();

    executor.run(|conn| {
        diesel::insert_into(clipboard_event::table)
            .values((
                clipboard_event::event_id.eq("test-event-1"),
                clipboard_event::captured_at_ms.eq(1704067200000i64),
                clipboard_event::source_device.eq("test-device"),
                clipboard_event::snapshot_hash.eq("blake3v1:testhash"),
            ))
            .execute(conn)?;

        diesel::insert_into(clipboard_snapshot_representation::table)
            .values(NewSnapshotRepresentationRow {
                id: rep_id.to_string(),
                event_id: "test-event-1".to_string(),
                format_id: "public.text".to_string(),
                mime_type: Some("text/plain".to_string()),
                size_bytes: 3,
                inline_data: None,
                blob_id: None,
                payload_state: PayloadAvailability::BlobReady.as_str().to_string(),
                last_error: None,
            })
            .execute(conn)?;

        Ok(())
    })?;

    let outcome = repo
        .update_processing_result(
            &rep_id,
            &[PayloadAvailability::Staged],
            None,
            PayloadAvailability::Lost,
            Some("state mismatch"),
        )
        .await?;

    assert!(matches!(outcome, ProcessingUpdateOutcome::StateMismatch));

    Ok(())
}

#[tokio::test]
async fn test_update_processing_result_with_blob_clears_inline_data() -> Result<()> {
    let executor = TestDbExecutor::new();
    let repo = DieselClipboardRepresentationRepository::new(executor.clone());
    let rep_id = RepresentationId::new();
    let blob_id = uc_core::BlobId::from("blob-42");

    executor.run(|conn| {
        diesel::insert_into(clipboard_event::table)
            .values((
                clipboard_event::event_id.eq("test-event-2"),
                clipboard_event::captured_at_ms.eq(1704067200001i64),
                clipboard_event::source_device.eq("test-device"),
                clipboard_event::snapshot_hash.eq("blake3v1:testhash2"),
            ))
            .execute(conn)?;

        diesel::insert_into(clipboard_snapshot_representation::table)
            .values(NewSnapshotRepresentationRow {
                id: rep_id.to_string(),
                event_id: "test-event-2".to_string(),
                format_id: "public.utf8-plain-text".to_string(),
                mime_type: Some("text/plain".to_string()),
                size_bytes: 1024,
                inline_data: Some(b"preview".to_vec()),
                blob_id: None,
                payload_state: PayloadAvailability::Staged.as_str().to_string(),
                last_error: None,
            })
            .execute(conn)?;

        diesel::insert_into(blob::table)
            .values((
                blob::blob_id.eq("blob-42"),
                blob::storage_path.eq("/tmp/test-blob-42"),
                blob::storage_backend.eq("filesystem"),
                blob::size_bytes.eq(1024i64),
                blob::content_hash.eq("blake3v1:testhash42"),
                blob::encryption_algo.eq::<Option<String>>(None),
                blob::created_at_ms.eq(1704067200001i64),
                blob::compressed_size.eq::<Option<i64>>(None),
            ))
            .execute(conn)?;

        Ok(())
    })?;

    let outcome = repo
        .update_processing_result(
            &rep_id,
            &[PayloadAvailability::Staged, PayloadAvailability::Processing],
            Some(&blob_id),
            PayloadAvailability::BlobReady,
            None,
        )
        .await?;

    let updated = match outcome {
        ProcessingUpdateOutcome::Updated(rep) => rep,
        other => panic!("expected Updated outcome, got {:?}", other),
    };

    assert_eq!(updated.blob_id, Some(blob_id));
    assert_eq!(updated.inline_data, None);
    assert_eq!(updated.payload_state(), PayloadAvailability::BlobReady);

    Ok(())
}
