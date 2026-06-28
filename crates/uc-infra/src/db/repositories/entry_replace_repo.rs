//! Adapter for [`ReplaceEntryContentPort`]: swaps the content behind an
//! existing `entry_id` in a single transaction, reusing the entry row (and its
//! sticky `pinned` / `active_time_ms` / `created_at_ms`) while replacing its
//! event, representations, selection, thumbnails, delivery and transfer rows.
//!
//! The delete/insert order is foreign-key safe under `PRAGMA foreign_keys = ON`:
//! the entry is re-pointed at the new event before the old event (and its
//! dependents) are removed, so nothing ever references a deleted parent.

use anyhow::{anyhow, Result};
use diesel::prelude::*;
use tracing::instrument;
use uc_core::clipboard::{
    ClipboardEvent, ClipboardRepositoryError, ClipboardSelectionDecision,
    PersistedClipboardRepresentation,
};
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::ReplaceEntryContentPort;

use crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
use crate::db::mappers::clipboard_selection_mapper::ClipboardSelectionRowMapper;
use crate::db::mappers::snapshot_representation_mapper::RepresentationRowMapper;
use crate::db::ports::{DbExecutor, InsertMapper};
use crate::db::schema::{
    clipboard_entry, clipboard_entry_delivery, clipboard_event, clipboard_representation_thumbnail,
    clipboard_selection, clipboard_snapshot_representation, file_transfer, file_transfer_events,
};

pub struct DieselClipboardEntryReplaceRepository<E> {
    executor: E,
}

impl<E> DieselClipboardEntryReplaceRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

fn to_repo_err(e: anyhow::Error) -> ClipboardRepositoryError {
    ClipboardRepositoryError::Storage(e.to_string())
}

#[async_trait::async_trait]
impl<E> ReplaceEntryContentPort for DieselClipboardEntryReplaceRepository<E>
where
    E: DbExecutor,
{
    #[instrument(
        name = "infra.sqlite.replace_entry_content",
        skip_all,
        fields(operation = "replace_entry_content", entry_id = %entry_id)
    )]
    async fn replace_entry_content(
        &self,
        entry_id: &EntryId,
        new_event: &ClipboardEvent,
        new_representations: &[PersistedClipboardRepresentation],
        new_selection: &ClipboardSelectionDecision,
        new_title: Option<String>,
        new_total_size: i64,
    ) -> Result<(), ClipboardRepositoryError> {
        // Map domain → rows up front (pure, outside the transaction).
        let new_event_row = ClipboardEventRowMapper
            .to_row(new_event)
            .map_err(to_repo_err)?;
        let new_event_id = new_event.event_id.to_string();
        let rep_rows = new_representations
            .iter()
            .map(|rep| RepresentationRowMapper.to_row(&(rep, &new_event.event_id)))
            .collect::<Result<Vec<_>>>()
            .map_err(to_repo_err)?;
        let mut new_selection_row = ClipboardSelectionRowMapper
            .to_row(new_selection)
            .map_err(to_repo_err)?;
        let entry_id_str = entry_id.to_string();
        // Bind the selection to the authoritative entry_id. The port contract
        // requires `new_selection` to reference `entry_id`; force it here so a
        // mismatched decision can never attach selection to a different entry
        // inside the transaction (which deletes selection by `entry_id_str`).
        new_selection_row.entry_id = entry_id_str.clone();

        self.executor
            .run(move |conn| {
                conn.transaction(|conn| {
                    // 1. Resolve the entry's current event; absent → error
                    //    (replace never implicitly creates).
                    let old_event_id: Option<String> = clipboard_entry::table
                        .filter(clipboard_entry::entry_id.eq(&entry_id_str))
                        .select(clipboard_entry::event_id)
                        .first::<String>(conn)
                        .optional()?;
                    let old_event_id = old_event_id.ok_or_else(|| {
                        anyhow!("replace_entry_content: no entry with id {entry_id_str}")
                    })?;

                    // 2. Capture old child ids needed for cascades before delete.
                    let old_rep_ids: Vec<String> = clipboard_snapshot_representation::table
                        .filter(clipboard_snapshot_representation::event_id.eq(&old_event_id))
                        .select(clipboard_snapshot_representation::id)
                        .load::<String>(conn)?;
                    let old_transfer_ids: Vec<String> = file_transfer::table
                        .filter(file_transfer::entry_id.eq(&entry_id_str))
                        .select(file_transfer::transfer_id)
                        .load::<String>(conn)?;

                    // 3. Insert the new event, then re-point the entry at it.
                    //    Sticky fields (pinned/active_time_ms/created_at_ms) are
                    //    intentionally left untouched.
                    diesel::insert_into(clipboard_event::table)
                        .values(&new_event_row)
                        .execute(conn)?;
                    diesel::update(
                        clipboard_entry::table.filter(clipboard_entry::entry_id.eq(&entry_id_str)),
                    )
                    .set((
                        clipboard_entry::event_id.eq(new_event_id.as_str()),
                        clipboard_entry::title.eq(new_title.as_deref()),
                        clipboard_entry::total_size.eq(new_total_size),
                    ))
                    .execute(conn)?;

                    // 4. Insert the new representations (FK → new event).
                    for rep in &rep_rows {
                        diesel::insert_into(clipboard_snapshot_representation::table)
                            .values(rep)
                            .execute(conn)?;
                    }

                    // 5. Replace the selection (PK = entry_id ⇒ delete then insert).
                    diesel::delete(
                        clipboard_selection::table
                            .filter(clipboard_selection::entry_id.eq(&entry_id_str)),
                    )
                    .execute(conn)?;
                    diesel::insert_into(clipboard_selection::table)
                        .values(&new_selection_row)
                        .execute(conn)?;

                    // 6. Drop the old content's dependents.
                    if !old_rep_ids.is_empty() {
                        diesel::delete(
                            clipboard_representation_thumbnail::table.filter(
                                clipboard_representation_thumbnail::representation_id
                                    .eq_any(&old_rep_ids),
                            ),
                        )
                        .execute(conn)?;
                    }
                    diesel::delete(
                        clipboard_snapshot_representation::table
                            .filter(clipboard_snapshot_representation::event_id.eq(&old_event_id)),
                    )
                    .execute(conn)?;
                    if !old_transfer_ids.is_empty() {
                        diesel::delete(
                            file_transfer_events::table.filter(
                                file_transfer_events::transfer_id.eq_any(&old_transfer_ids),
                            ),
                        )
                        .execute(conn)?;
                    }
                    diesel::delete(
                        file_transfer::table.filter(file_transfer::entry_id.eq(&entry_id_str)),
                    )
                    .execute(conn)?;
                    diesel::delete(
                        clipboard_entry_delivery::table
                            .filter(clipboard_entry_delivery::entry_id.eq(&entry_id_str)),
                    )
                    .execute(conn)?;

                    // 7. Finally remove the now-orphaned old event.
                    diesel::delete(
                        clipboard_event::table.filter(clipboard_event::event_id.eq(&old_event_id)),
                    )
                    .execute(conn)?;

                    Ok(())
                })
            })
            .map_err(to_repo_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::models::snapshot_representation::NewSnapshotRepresentationRow;
    use crate::db::models::{NewClipboardEntryRow, NewClipboardEventRow, NewClipboardSelectionRow};
    use crate::db::pool::init_db_pool;
    use std::sync::Arc;
    use tempfile::{tempdir, TempDir};
    use uc_core::clipboard::{
        ClipboardSelection, MimeType, PersistedClipboardRepresentation, SelectionPolicyVersion,
    };
    use uc_core::ids::{DeviceId, EntryId, EventId, FormatId, RepresentationId};
    use uc_core::SnapshotHash;

    fn hash(hex64: &str) -> SnapshotHash {
        SnapshotHash::parse(&format!("blake3v1:{hex64}")).expect("valid snapshot hash")
    }

    const OLD_HASH: &str = "1111111111111111111111111111111111111111111111111111111111111111";
    const NEW_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";

    fn make_repo() -> (
        DieselClipboardEntryReplaceRepository<Arc<DieselSqliteExecutor>>,
        Arc<DieselSqliteExecutor>,
        TempDir,
    ) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("replace-repo.sqlite");
        let pool = init_db_pool(path.to_str().unwrap()).unwrap();
        let executor = Arc::new(DieselSqliteExecutor::new(pool));
        let repo = DieselClipboardEntryReplaceRepository::new(Arc::clone(&executor));
        (repo, executor, dir)
    }

    /// Seed one full entry (event + entry + 2 reps + selection + thumbnail +
    /// delivery + transfer + transfer event) so the replace cascade has every
    /// dependent kind to remove.
    fn seed_full_entry(executor: &Arc<DieselSqliteExecutor>) {
        executor
            .run(|conn| {
                diesel::insert_into(clipboard_event::table)
                    .values(&NewClipboardEventRow {
                        event_id: "old-ev".into(),
                        captured_at_ms: 500,
                        source_device: "dev-a".into(),
                        snapshot_hash: format!("blake3v1:{OLD_HASH}"),
                    })
                    .execute(conn)?;
                diesel::insert_into(clipboard_entry::table)
                    .values(&NewClipboardEntryRow {
                        entry_id: "e1".into(),
                        event_id: "old-ev".into(),
                        created_at_ms: 2222,
                        active_time_ms: 1111,
                        title: Some("old title".into()),
                        total_size: 10,
                        pinned: true,
                        delivery_tracked: true,
                        // Seeded true so the replacement test proves favorite
                        // state survives like the other sticky columns.
                        is_favorited: true,
                    })
                    .execute(conn)?;
                for rep_id in ["r1", "r2"] {
                    diesel::insert_into(clipboard_snapshot_representation::table)
                        .values(&NewSnapshotRepresentationRow {
                            id: rep_id.into(),
                            event_id: "old-ev".into(),
                            format_id: "text".into(),
                            mime_type: Some("text/plain".into()),
                            size_bytes: 3,
                            inline_data: Some(b"old".to_vec()),
                            blob_id: None,
                            payload_state: "Inline".into(),
                            last_error: None,
                        })
                        .execute(conn)?;
                }
                diesel::insert_into(clipboard_selection::table)
                    .values(&NewClipboardSelectionRow {
                        entry_id: "e1".into(),
                        primary_rep_id: "r1".into(),
                        secondary_rep_ids: String::new(),
                        preview_rep_id: "r1".into(),
                        paste_rep_id: "r1".into(),
                        policy_version: "v1".into(),
                    })
                    .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO clipboard_representation_thumbnail \
                     (representation_id, thumbnail_blob_id, thumbnail_mime_type, \
                      original_width, original_height, original_size_bytes) \
                     VALUES ('r1', 'blob-thumb', 'image/png', 1, 1, 1)",
                )
                .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO clipboard_entry_delivery \
                     (entry_id, target_device_id, status, updated_at_ms) \
                     VALUES ('e1', 'dev-b', 'pending', 0)",
                )
                .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO file_transfer \
                     (transfer_id, entry_id, filename, status, source_device, \
                      created_at_ms, updated_at_ms) \
                     VALUES ('t1', 'e1', 'f.bin', 'pending', 'dev-a', 0, 0)",
                )
                .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO file_transfer_events \
                     (transfer_id, sequence, event_type, payload_json, occurred_at_ms) \
                     VALUES ('t1', 0, 'started', '{}', 0)",
                )
                .execute(conn)?;
                Ok(())
            })
            .unwrap();
    }

    fn new_content() -> (
        uc_core::clipboard::ClipboardEvent,
        Vec<PersistedClipboardRepresentation>,
        ClipboardSelectionDecision,
    ) {
        let new_event = ClipboardEvent::new(
            EventId::from("new-ev"),
            900,
            DeviceId::new("dev-c".to_string()),
            hash(NEW_HASH),
        );
        let reps = vec![PersistedClipboardRepresentation::new(
            RepresentationId::from("nr1"),
            FormatId::from("text"),
            Some(MimeType("text/plain".to_string())),
            5,
            Some(b"hello".to_vec()),
            None,
        )];
        let selection = ClipboardSelectionDecision::new(
            EntryId::from("e1"),
            ClipboardSelection {
                primary_rep_id: RepresentationId::from("nr1"),
                secondary_rep_ids: vec![],
                preview_rep_id: RepresentationId::from("nr1"),
                paste_rep_id: RepresentationId::from("nr1"),
                policy_version: SelectionPolicyVersion::V1,
            },
        );
        (new_event, reps, selection)
    }

    #[tokio::test]
    async fn replace_reuses_entry_id_preserves_sticky_state_and_cascades() {
        let (repo, executor, _dir) = make_repo();
        seed_full_entry(&executor);
        let (new_event, reps, selection) = new_content();

        repo.replace_entry_content(
            &EntryId::from("e1"),
            &new_event,
            &reps,
            &selection,
            Some("new title".to_string()),
            99,
        )
        .await
        .expect("replace ok");

        executor
            .run(|conn| {
                // Entry kept its id + sticky fields; content pointer updated.
                let (event_id, created, active, title, total, pinned, is_favorited): (
                    String,
                    i64,
                    i64,
                    Option<String>,
                    i64,
                    bool,
                    bool,
                ) = clipboard_entry::table
                    .filter(clipboard_entry::entry_id.eq("e1"))
                    .select((
                        clipboard_entry::event_id,
                        clipboard_entry::created_at_ms,
                        clipboard_entry::active_time_ms,
                        clipboard_entry::title,
                        clipboard_entry::total_size,
                        clipboard_entry::pinned,
                        clipboard_entry::is_favorited,
                    ))
                    .first(conn)?;
                assert_eq!(event_id, "new-ev", "entry re-pointed at the new event");
                assert_eq!(created, 2222, "created_at_ms preserved");
                assert_eq!(active, 1111, "active_time_ms preserved");
                assert!(pinned, "pinned preserved");
                assert!(is_favorited, "is_favorited preserved");
                assert_eq!(title.as_deref(), Some("new title"), "title updated");
                assert_eq!(total, 99, "total_size updated");

                // Exactly one event row, and it is the new one.
                let event_count: i64 = clipboard_event::table.count().get_result(conn)?;
                assert_eq!(event_count, 1, "old event removed, only the new remains");
                let new_hash: String = clipboard_event::table
                    .filter(clipboard_event::event_id.eq("new-ev"))
                    .select(clipboard_event::snapshot_hash)
                    .first(conn)?;
                assert_eq!(new_hash, format!("blake3v1:{NEW_HASH}"));

                // No representations orphaned on the old event; new rep present.
                let old_reps: i64 = clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::event_id.eq("old-ev"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(old_reps, 0, "old representations removed");
                let new_reps: i64 = clipboard_snapshot_representation::table
                    .filter(clipboard_snapshot_representation::event_id.eq("new-ev"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(new_reps, 1, "new representation inserted");

                // Selection replaced (now points at the new rep).
                let primary: String = clipboard_selection::table
                    .filter(clipboard_selection::entry_id.eq("e1"))
                    .select(clipboard_selection::primary_rep_id)
                    .first(conn)?;
                assert_eq!(primary, "nr1", "selection replaced");

                // All old dependents cascaded away.
                let thumbs: i64 = clipboard_representation_thumbnail::table
                    .filter(clipboard_representation_thumbnail::representation_id.eq("r1"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(thumbs, 0, "old thumbnail removed");
                let deliveries: i64 = clipboard_entry_delivery::table
                    .filter(clipboard_entry_delivery::entry_id.eq("e1"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(deliveries, 0, "old delivery removed");
                let transfers: i64 = file_transfer::table
                    .filter(file_transfer::entry_id.eq("e1"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(transfers, 0, "old transfer removed");
                let transfer_events: i64 = file_transfer_events::table
                    .filter(file_transfer_events::transfer_id.eq("t1"))
                    .count()
                    .get_result(conn)?;
                assert_eq!(transfer_events, 0, "old transfer events removed");
                Ok(())
            })
            .unwrap();
    }

    #[tokio::test]
    async fn replace_errors_when_entry_absent() {
        let (repo, _executor, _dir) = make_repo();
        let (new_event, reps, selection) = new_content();
        let result = repo
            .replace_entry_content(
                &EntryId::from("does-not-exist"),
                &new_event,
                &reps,
                &selection,
                None,
                0,
            )
            .await;
        assert!(
            result.is_err(),
            "replace must not implicitly create an entry"
        );
    }
}
