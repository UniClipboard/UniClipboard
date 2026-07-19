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
    ClipboardEntryContentCategory, ClipboardEvent, ClipboardRepositoryError,
    ClipboardSelectionDecision, PersistedClipboardRepresentation,
};
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::ReplaceEntryContentPort;

use crate::db::mappers::clipboard_event_mapper::ClipboardEventRowMapper;
use crate::db::mappers::clipboard_selection_mapper::ClipboardSelectionRowMapper;
use crate::db::mappers::snapshot_representation_mapper::RepresentationRowMapper;
use crate::db::models::snapshot_representation::NewSnapshotRepresentationRow;
use crate::db::models::{NewClipboardEventRow, NewClipboardSelectionRow};
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

pub(super) struct PreparedEntryReplacement {
    entry_id: String,
    new_event_id: String,
    new_event_row: NewClipboardEventRow,
    representation_rows: Vec<NewSnapshotRepresentationRow>,
    selection_row: NewClipboardSelectionRow,
    new_total_size: i64,
    new_content_category: String,
}

pub(super) fn prepare_entry_replacement(
    entry_id: &EntryId,
    new_event: &ClipboardEvent,
    new_representations: &[PersistedClipboardRepresentation],
    new_selection: &ClipboardSelectionDecision,
    new_total_size: i64,
    new_content_category: ClipboardEntryContentCategory,
) -> Result<PreparedEntryReplacement> {
    let new_event_row = ClipboardEventRowMapper.to_row(new_event)?;
    let representation_rows = new_representations
        .iter()
        .map(|representation| {
            RepresentationRowMapper.to_row(&(representation, &new_event.event_id))
        })
        .collect::<Result<Vec<_>>>()?;
    let entry_id = entry_id.to_string();
    let mut selection_row = ClipboardSelectionRowMapper.to_row(new_selection)?;
    selection_row.entry_id = entry_id.clone();
    Ok(PreparedEntryReplacement {
        entry_id,
        new_event_id: new_event.event_id.to_string(),
        new_event_row,
        representation_rows,
        selection_row,
        new_total_size,
        new_content_category: new_content_category.as_db_str().to_owned(),
    })
}

pub(super) fn replace_entry_content_rows(
    conn: &mut SqliteConnection,
    prepared: &PreparedEntryReplacement,
    preserve_directory_attempts: bool,
) -> Result<()> {
    let old_event_id = clipboard_entry::table
        .filter(clipboard_entry::entry_id.eq(&prepared.entry_id))
        .select(clipboard_entry::event_id)
        .first::<String>(conn)
        .optional()?
        .ok_or_else(|| {
            anyhow!(
                "replace_entry_content: no entry with id {}",
                prepared.entry_id
            )
        })?;
    let old_rep_ids = clipboard_snapshot_representation::table
        .filter(clipboard_snapshot_representation::event_id.eq(&old_event_id))
        .select(clipboard_snapshot_representation::id)
        .load::<String>(conn)?;
    let mut transfers = file_transfer::table
        .filter(file_transfer::entry_id.eq(&prepared.entry_id))
        .into_boxed();
    if preserve_directory_attempts {
        transfers = transfers.filter(file_transfer::attempt_id.is_null());
    }
    let old_transfer_ids = transfers
        .select(file_transfer::transfer_id)
        .load::<String>(conn)?;

    diesel::insert_into(clipboard_event::table)
        .values(&prepared.new_event_row)
        .execute(conn)?;
    diesel::update(clipboard_entry::table.filter(clipboard_entry::entry_id.eq(&prepared.entry_id)))
        .set((
            clipboard_entry::event_id.eq(&prepared.new_event_id),
            clipboard_entry::total_size.eq(prepared.new_total_size),
            clipboard_entry::content_category.eq(&prepared.new_content_category),
        ))
        .execute(conn)?;
    for representation in &prepared.representation_rows {
        diesel::insert_into(clipboard_snapshot_representation::table)
            .values(representation)
            .execute(conn)?;
    }
    diesel::delete(
        clipboard_selection::table.filter(clipboard_selection::entry_id.eq(&prepared.entry_id)),
    )
    .execute(conn)?;
    diesel::insert_into(clipboard_selection::table)
        .values(&prepared.selection_row)
        .execute(conn)?;

    if !old_rep_ids.is_empty() {
        diesel::delete(
            clipboard_representation_thumbnail::table
                .filter(clipboard_representation_thumbnail::representation_id.eq_any(&old_rep_ids)),
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
            file_transfer_events::table
                .filter(file_transfer_events::transfer_id.eq_any(&old_transfer_ids)),
        )
        .execute(conn)?;
        diesel::delete(
            file_transfer::table.filter(file_transfer::transfer_id.eq_any(&old_transfer_ids)),
        )
        .execute(conn)?;
    }
    diesel::delete(
        clipboard_entry_delivery::table
            .filter(clipboard_entry_delivery::entry_id.eq(&prepared.entry_id)),
    )
    .execute(conn)?;
    diesel::delete(clipboard_event::table.filter(clipboard_event::event_id.eq(old_event_id)))
        .execute(conn)?;
    Ok(())
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
        new_total_size: i64,
        new_content_category: ClipboardEntryContentCategory,
    ) -> Result<(), ClipboardRepositoryError> {
        let prepared = prepare_entry_replacement(
            entry_id,
            new_event,
            new_representations,
            new_selection,
            new_total_size,
            new_content_category,
        )
        .map_err(to_repo_err)?;
        self.executor
            .run(move |conn| {
                conn.transaction(|conn| replace_entry_content_rows(conn, &prepared, false))
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
                        total_size: 10,
                        pinned: true,
                        delivery_tracked: true,
                        // Seeded true so the replacement test proves favorite
                        // state survives like the other sticky columns.
                        is_favorited: true,
                        content_category: "text".into(),
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
                     (transfer_id, entry_id, binding_state, receive_item_id, status, \
                      source_device, metadata_ciphertext, created_at_ms, updated_at_ms) \
                     VALUES ('t1', 'e1', 'legacy', 't1', 'pending', 'dev-a', X'00', 0, 0)",
                )
                .execute(conn)?;
                diesel::sql_query(
                    "INSERT INTO file_transfer_events \
                     (transfer_id, sequence, event_type, payload_ciphertext, occurred_at_ms) \
                     VALUES ('t1', 0, 'started', X'00', 0)",
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
            99,
            ClipboardEntryContentCategory::Image,
        )
        .await
        .expect("replace ok");

        executor
            .run(|conn| {
                // Entry kept its id + sticky fields; content pointer updated.
                let (event_id, created, active, total, pinned, is_favorited, content_category): (
                    String,
                    i64,
                    i64,
                    i64,
                    bool,
                    bool,
                    String,
                ) = clipboard_entry::table
                    .filter(clipboard_entry::entry_id.eq("e1"))
                    .select((
                        clipboard_entry::event_id,
                        clipboard_entry::created_at_ms,
                        clipboard_entry::active_time_ms,
                        clipboard_entry::total_size,
                        clipboard_entry::pinned,
                        clipboard_entry::is_favorited,
                        clipboard_entry::content_category,
                    ))
                    .first(conn)?;
                assert_eq!(event_id, "new-ev", "entry re-pointed at the new event");
                assert_eq!(created, 2222, "created_at_ms preserved");
                assert_eq!(active, 1111, "active_time_ms preserved");
                assert!(pinned, "pinned preserved");
                assert!(is_favorited, "is_favorited preserved");
                assert_eq!(content_category, "image", "content_category updated");
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
                0,
                ClipboardEntryContentCategory::Text,
            )
            .await;
        assert!(
            result.is_err(),
            "replace must not implicitly create an entry"
        );
    }
}
