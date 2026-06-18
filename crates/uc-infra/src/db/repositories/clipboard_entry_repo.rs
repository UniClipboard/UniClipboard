use crate::db::models::ClipboardEntryRow;
use crate::db::models::NewClipboardEntryRow;
use crate::db::models::NewClipboardSelectionRow;
use crate::db::ports::DbExecutor;
use crate::db::ports::{InsertMapper, RowMapper};
use crate::db::schema::{clipboard_entry, clipboard_event, clipboard_selection};
use anyhow::Result;
use diesel::query_dsl::methods::FilterDsl;
use diesel::query_dsl::methods::LimitDsl;
use diesel::query_dsl::methods::OffsetDsl;
use diesel::query_dsl::methods::OrderDsl;
use diesel::query_dsl::methods::SelectDsl;
use diesel::Connection;
use diesel::ExpressionMethods;
use diesel::OptionalExtension;
use diesel::RunQueryDsl;
use tracing::instrument;
use uc_core::clipboard::{ClipboardEntry, ClipboardRepositoryError, ClipboardSelectionDecision};
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::{
    DeleteClipboardEntryPort, FindEntryIdBySnapshotHashPort, GetClipboardEntryPort,
    ListClipboardEntriesPort, SaveClipboardEntryPort, TouchClipboardEntryPort,
};
use uc_core::ports::ClipboardEntryStore;

pub struct DieselClipboardEntryRepository<E, ME, MS, RE> {
    executor: E,
    entry_mapper: ME,
    selection_mapper: MS,
    row_entry_mapper: RE,
}

impl<E, ME, MS, RE> DieselClipboardEntryRepository<E, ME, MS, RE> {
    pub fn new(executor: E, entry_mapper: ME, selection_mapper: MS, row_entry_mapper: RE) -> Self {
        Self {
            executor,
            entry_mapper,
            selection_mapper,
            row_entry_mapper,
        }
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> ClipboardEntryStore for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    #[instrument(
        name = "infra.sqlite.insert_clipboard_entry",
        skip_all,
        fields(
            operation = "save_entry",
            table = "clipboard_entry",
            entry_id = %entry.entry_id,
        )
    )]
    async fn save_entry_and_selection(
        &self,
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
    ) -> Result<()> {
        self.executor.run(|conn| {
            let new_entry_row = self.entry_mapper.to_row(entry)?;
            let new_selection_row = self.selection_mapper.to_row(selection)?;

            conn.transaction(|conn| {
                diesel::insert_into(clipboard_entry::table)
                    .values(&new_entry_row)
                    .execute(conn)?;

                diesel::insert_into(clipboard_selection::table)
                    .values(&new_selection_row)
                    .execute(conn)?;

                Ok(())
            })
        })
    }

    #[instrument(
        name = "infra.sqlite.query_clipboard_entry",
        skip_all,
        fields(
            operation = "get_entry",
            table = "clipboard_entry",
            entry_id = %entry_id,
        )
    )]
    async fn get_entry(&self, entry_id: &EntryId) -> Result<Option<ClipboardEntry>> {
        let entry_id_str = entry_id.to_string();
        self.executor.run(|conn| {
            let entry_row = clipboard_entry::table
                .filter(clipboard_entry::entry_id.eq(&entry_id_str))
                .first::<ClipboardEntryRow>(conn)
                .optional()?;

            match entry_row {
                Some(row) => {
                    let entry = self.row_entry_mapper.to_domain(&row)?;
                    Ok(Some(entry))
                }
                None => Ok(None),
            }
        })
    }

    /// Lists clipboard entries ordered by active time (newest first) with pagination.
    ///
    /// # Parameters
    ///
    /// - `limit`: Maximum number of entries to return.
    /// - `offset`: Number of entries to skip before collecting results (zero-based).
    ///
    /// # Returns
    ///
    /// A `Vec<ClipboardEntry>` containing entries ordered by `active_time_ms` descending.
    ///
    /// # Examples
    ///
    /// ```
    /// # use uc_core::ports::ClipboardEntryStore;
    /// # async fn example(repo: &impl ClipboardEntryStore) -> anyhow::Result<()> {
    /// let entries = repo.list_entries(10, 0).await?;
    /// assert!(entries.len() <= 10);
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(
        name = "infra.sqlite.query_clipboard_entries",
        skip_all,
        fields(
            operation = "list_entries",
            table = "clipboard_entry",
            limit = limit,
            offset = offset,
        )
    )]
    async fn list_entries(&self, limit: usize, offset: usize) -> Result<Vec<ClipboardEntry>> {
        self.executor.run(|conn| {
            let entry_rows = clipboard_entry::table
                .order(clipboard_entry::active_time_ms.desc())
                .limit(limit as i64)
                .offset(offset as i64)
                .load::<ClipboardEntryRow>(conn)?;

            entry_rows
                .into_iter()
                .map(|row| self.row_entry_mapper.to_domain(&row))
                .collect()
        })
    }

    #[instrument(
        name = "infra.sqlite.touch_clipboard_entry",
        skip_all,
        fields(
            operation = "touch_entry",
            table = "clipboard_entry",
            entry_id = %entry_id,
            active_time_ms = active_time_ms,
        )
    )]
    async fn touch_entry(&self, entry_id: &EntryId, active_time_ms: i64) -> Result<bool> {
        self.executor.run(|conn| {
            use crate::db::schema::clipboard_entry::dsl;

            let affected = diesel::update(dsl::clipboard_entry)
                .filter(dsl::entry_id.eq(entry_id.to_string()))
                .set(dsl::active_time_ms.eq(active_time_ms))
                .execute(conn)?;

            Ok(affected > 0)
        })
    }

    /// Deletes the clipboard entry with the given `EntryId` from the database.
    ///
    /// Attempts to remove the entry row whose `entry_id` matches `entry_id`. The operation returns `Ok(())` on success; if no row matches the provided `entry_id` the call still succeeds and returns `Ok(())`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use uc_core::ids::EntryId;
    /// # use uc_core::ports::ClipboardEntryStore;
    /// // Remove an entry by id
    /// # async fn run(repo: &impl ClipboardEntryStore, id: EntryId) -> anyhow::Result<()> {
    /// repo.delete_entry(&id).await?;
    /// # Ok(())
    /// # }
    /// ```
    #[instrument(
        name = "infra.sqlite.delete_clipboard_entry",
        skip_all,
        fields(
            operation = "delete_entry",
            table = "clipboard_entry",
            entry_id = %entry_id,
        )
    )]
    async fn delete_entry(&self, entry_id: &EntryId) -> Result<()> {
        let entry_id_str = entry_id.to_string();
        self.executor.run(|conn| {
            diesel::delete(clipboard_entry::table)
                .filter(clipboard_entry::entry_id.eq(&entry_id_str))
                .execute(conn)?;
            Ok(())
        })
    }

    /// Two-step lookup: first locate the event row for the given
    /// `snapshot_hash`, then locate the entry row whose `event_id`
    /// matches. Avoids `inner_join` import gymnastics (multiple
    /// `filter` candidates clash with the existing per-method DSL
    /// imports) while keeping the same dedup semantics.
    ///
    /// Two prepared statements vs one JOIN: trivially different cost on
    /// SQLite for an index-hit lookup (`snapshot_hash` is the natural
    /// dedup key), and Phase 3 calls this exactly once per inbound
    /// frame, so optimization is not warranted.
    #[instrument(
        name = "infra.sqlite.find_entry_by_snapshot_hash",
        skip_all,
        fields(
            operation = "find_entry_by_snapshot_hash",
            table = "clipboard_event + clipboard_entry",
            snapshot_hash_len = snapshot_hash.len(),
        )
    )]
    async fn find_entry_id_by_snapshot_hash(&self, snapshot_hash: &str) -> Result<Option<EntryId>> {
        let hash = snapshot_hash.to_string();
        self.executor.run(move |conn| {
            let event_id_str: Option<String> = clipboard_event::table
                .filter(clipboard_event::snapshot_hash.eq(&hash))
                .select(clipboard_event::event_id)
                .limit(1)
                .first::<String>(conn)
                .optional()?;

            let event_id_str = match event_id_str {
                Some(id) => id,
                None => return Ok(None),
            };

            let entry_id_str: Option<String> = clipboard_entry::table
                .filter(clipboard_entry::event_id.eq(&event_id_str))
                .select(clipboard_entry::entry_id)
                .limit(1)
                .first::<String>(conn)
                .optional()?;

            Ok(entry_id_str.map(EntryId::from))
        })
    }
}

// ---- Intent ports ------------------------------------------------------
//
// The single Diesel adapter is coerced into these narrow intent ports at the
// composition root. Each impl delegates to the aggregate trait above and
// translates the storage error into the typed domain error.

fn to_repo_err(e: anyhow::Error) -> ClipboardRepositoryError {
    ClipboardRepositoryError::Storage(e.to_string())
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> GetClipboardEntryPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn get_entry(
        &self,
        entry_id: &EntryId,
    ) -> Result<Option<ClipboardEntry>, ClipboardRepositoryError> {
        ClipboardEntryStore::get_entry(self, entry_id)
            .await
            .map_err(to_repo_err)
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> ListClipboardEntriesPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn list_entries(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<ClipboardEntry>, ClipboardRepositoryError> {
        ClipboardEntryStore::list_entries(self, limit, offset)
            .await
            .map_err(to_repo_err)
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> SaveClipboardEntryPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn save_entry_and_selection(
        &self,
        entry: &ClipboardEntry,
        selection: &ClipboardSelectionDecision,
    ) -> Result<(), ClipboardRepositoryError> {
        ClipboardEntryStore::save_entry_and_selection(self, entry, selection)
            .await
            .map_err(to_repo_err)
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> TouchClipboardEntryPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn touch_entry(
        &self,
        entry_id: &EntryId,
        active_time_ms: i64,
    ) -> Result<bool, ClipboardRepositoryError> {
        ClipboardEntryStore::touch_entry(self, entry_id, active_time_ms)
            .await
            .map_err(to_repo_err)
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> DeleteClipboardEntryPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn delete_entry(&self, entry_id: &EntryId) -> Result<(), ClipboardRepositoryError> {
        ClipboardEntryStore::delete_entry(self, entry_id)
            .await
            .map_err(to_repo_err)
    }
}

#[async_trait::async_trait]
impl<E, ME, MS, RE> FindEntryIdBySnapshotHashPort for DieselClipboardEntryRepository<E, ME, MS, RE>
where
    E: DbExecutor,
    ME: InsertMapper<ClipboardEntry, NewClipboardEntryRow>,
    MS: InsertMapper<ClipboardSelectionDecision, NewClipboardSelectionRow>,
    RE: RowMapper<ClipboardEntryRow, ClipboardEntry>,
{
    async fn find_entry_id_by_snapshot_hash(
        &self,
        snapshot_hash: &str,
    ) -> Result<Option<EntryId>, ClipboardRepositoryError> {
        ClipboardEntryStore::find_entry_id_by_snapshot_hash(self, snapshot_hash)
            .await
            .map_err(to_repo_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::mappers::clipboard_entry_mapper::ClipboardEntryRowMapper;
    use crate::db::mappers::clipboard_selection_mapper::ClipboardSelectionRowMapper;
    use crate::db::models::{NewClipboardEntryRow, NewClipboardEventRow};
    use crate::db::pool::init_db_pool;
    use crate::db::ports::DbExecutor;
    use tempfile::{tempdir, TempDir};

    type Repo = DieselClipboardEntryRepository<
        DieselSqliteExecutor,
        ClipboardEntryRowMapper,
        ClipboardSelectionRowMapper,
        ClipboardEntryRowMapper,
    >;

    /// Build a repo + a second executor (sharing the same pool so
    /// direct-insert fixtures land in the same in-memory DB as the repo
    /// reads). `DieselSqliteExecutor::new` takes a pool by value, so we
    /// build two executors from the same file-backed pool.
    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let database_url = tempdir.path().join("entry-repo.sqlite");
        let path = database_url.to_str().unwrap();
        let pool_for_repo = init_db_pool(path).unwrap();
        let pool_for_seed = init_db_pool(path).unwrap();
        let repo = DieselClipboardEntryRepository::new(
            DieselSqliteExecutor::new(pool_for_repo),
            ClipboardEntryRowMapper,
            ClipboardSelectionRowMapper,
            ClipboardEntryRowMapper,
        );
        (repo, DieselSqliteExecutor::new(pool_for_seed), tempdir)
    }

    /// Seed one `clipboard_event` + `clipboard_entry` row pair carrying
    /// the given `snapshot_hash`. Bypasses the mapper pipeline — this
    /// test only exercises the read-side lookup contract, not the write
    /// path (which is covered by higher-level use case tests).
    fn seed_event_and_entry(executor: &DieselSqliteExecutor, snapshot_hash: &str) -> String {
        use crate::db::schema::{clipboard_entry, clipboard_event};

        let event_id = format!("ev-{}", uuid::Uuid::new_v4());
        let entry_id = format!("entry-{}", uuid::Uuid::new_v4());

        let event_row = NewClipboardEventRow {
            event_id: event_id.clone(),
            captured_at_ms: 1_700_000_000_000,
            source_device: "test-device".into(),
            snapshot_hash: snapshot_hash.into(),
        };
        let entry_row = NewClipboardEntryRow {
            entry_id: entry_id.clone(),
            event_id: event_id.clone(),
            created_at_ms: 1_700_000_000_000,
            active_time_ms: 1_700_000_000_000,
            title: Some("test".into()),
            total_size: 0,
            pinned: false,
            delivery_tracked: false,
        };

        executor
            .run(move |conn| {
                diesel::insert_into(clipboard_event::table)
                    .values(&event_row)
                    .execute(conn)?;
                diesel::insert_into(clipboard_entry::table)
                    .values(&entry_row)
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();

        entry_id
    }

    #[tokio::test]
    async fn find_entry_id_by_snapshot_hash_returns_existing_entry() {
        let (repo, executor, _tempdir) = make_repo();
        let hash = "blake3v1:deadbeef00000000000000000000000000000000000000000000000000000000";
        let expected_entry_id = seed_event_and_entry(&executor, hash);

        let actual = ClipboardEntryStore::find_entry_id_by_snapshot_hash(&repo, hash)
            .await
            .expect("query ok");
        assert_eq!(
            actual.map(|e| e.into_inner()),
            Some(expected_entry_id),
            "existing snapshot_hash must resolve to the seeded entry_id"
        );
    }

    #[tokio::test]
    async fn find_entry_id_by_snapshot_hash_returns_none_for_missing() {
        let (repo, _executor, _tempdir) = make_repo();
        let result = ClipboardEntryStore::find_entry_id_by_snapshot_hash(
            &repo,
            "blake3v1:ffffffff00000000000000000000000000000000000000000000000000000000",
        )
        .await
        .expect("query ok");
        assert!(result.is_none(), "unknown hash must return None");
    }
}
