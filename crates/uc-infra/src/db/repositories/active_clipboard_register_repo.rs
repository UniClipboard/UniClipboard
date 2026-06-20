//! SQLite adapter for the single-row active-clipboard LWW register.

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::upsert::excluded;
use tracing::debug_span;

use crate::db::ports::DbExecutor;
use crate::db::schema::active_clipboard_register;
use uc_core::clipboard::ActiveClipboardState;
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::clipboard::{
    ActiveClipboardRegisterError, AdvanceActiveClipboardPort, LoadActiveClipboardPort,
    ResetActiveClipboardPort,
};

/// Fixed primary key for the single register row (the table is pinned to a
/// single row via a `CHECK (id = 1)` constraint).
const REGISTER_ROW_ID: i32 = 1;

#[derive(Debug, Insertable)]
#[diesel(table_name = active_clipboard_register)]
struct NewRegisterRow {
    id: i32,
    snapshot_hash: String,
    entry_id: String,
    activated_at_ms: i64,
    activated_by: String,
}

/// SQLite adapter implementing the active-clipboard register port.
pub struct DieselActiveClipboardRegisterRepository<E> {
    executor: E,
}

impl<E> DieselActiveClipboardRegisterRepository<E> {
    pub fn new(executor: E) -> Self {
        Self { executor }
    }
}

#[async_trait]
impl<E: DbExecutor> AdvanceActiveClipboardPort for DieselActiveClipboardRegisterRepository<E> {
    async fn advance(
        &self,
        state: &ActiveClipboardState,
    ) -> Result<bool, ActiveClipboardRegisterError> {
        let span = debug_span!(
            "infra.sqlite.active_clipboard_register.advance",
            snapshot_hash = %state.snapshot_hash,
            activated_at_ms = state.activated_at_ms,
            activated_by = %state.activated_by,
        );
        let row = NewRegisterRow {
            id: REGISTER_ROW_ID,
            snapshot_hash: state.snapshot_hash.clone(),
            entry_id: state.entry_id.as_ref().to_string(),
            activated_at_ms: state.activated_at_ms,
            activated_by: state.activated_by.as_str().to_string(),
        };

        span.in_scope(|| {
            self.executor.run(move |conn| {
                // Atomic conditional write: read the current LWW key, then
                // insert/overwrite only when the incoming value supersedes
                // it. Wrapping SELECT-then-UPSERT in a transaction keeps the
                // compare-and-set atomic so an LWW-loser is a true no-op.
                conn.transaction::<bool, diesel::result::Error, _>(|conn| {
                    let current: Option<(i64, String)> = active_clipboard_register::table
                        .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                        .select((
                            active_clipboard_register::activated_at_ms,
                            active_clipboard_register::activated_by,
                        ))
                        .first::<(i64, String)>(conn)
                        .optional()?;

                    // Mirrors `ActiveClipboardState::supersedes`: a strictly
                    // newer timestamp wins; on a tie the lexicographically
                    // greater `activated_by` wins; an exact-key duplicate is
                    // a no-op.
                    let should_advance = match &current {
                        None => true,
                        Some((cur_ts, cur_by)) => {
                            row.activated_at_ms > *cur_ts
                                || (row.activated_at_ms == *cur_ts && row.activated_by > *cur_by)
                        }
                    };
                    if !should_advance {
                        return Ok(false);
                    }

                    diesel::insert_into(active_clipboard_register::table)
                        .values(&row)
                        .on_conflict(active_clipboard_register::id)
                        .do_update()
                        .set((
                            active_clipboard_register::snapshot_hash
                                .eq(excluded(active_clipboard_register::snapshot_hash)),
                            active_clipboard_register::entry_id
                                .eq(excluded(active_clipboard_register::entry_id)),
                            active_clipboard_register::activated_at_ms
                                .eq(excluded(active_clipboard_register::activated_at_ms)),
                            active_clipboard_register::activated_by
                                .eq(excluded(active_clipboard_register::activated_by)),
                        ))
                        .execute(conn)?;
                    Ok(true)
                })
                .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
        })
        .map_err(|e| ActiveClipboardRegisterError::Storage(e.to_string()))
    }
}

#[async_trait]
impl<E: DbExecutor> LoadActiveClipboardPort for DieselActiveClipboardRegisterRepository<E> {
    async fn load(&self) -> Result<Option<ActiveClipboardState>, ActiveClipboardRegisterError> {
        let span = debug_span!("infra.sqlite.active_clipboard_register.load");
        let row: Option<(String, String, i64, String)> = span
            .in_scope(|| {
                self.executor.run(move |conn| {
                    Ok(active_clipboard_register::table
                        .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                        .select((
                            active_clipboard_register::snapshot_hash,
                            active_clipboard_register::entry_id,
                            active_clipboard_register::activated_at_ms,
                            active_clipboard_register::activated_by,
                        ))
                        .first::<(String, String, i64, String)>(conn)
                        .optional()?)
                })
            })
            .map_err(|e| ActiveClipboardRegisterError::Storage(e.to_string()))?;

        Ok(
            row.map(|(snapshot_hash, entry_id, activated_at_ms, activated_by)| {
                ActiveClipboardState::new(
                    snapshot_hash,
                    EntryId::from(entry_id),
                    activated_at_ms,
                    DeviceId::new(activated_by),
                )
            }),
        )
    }
}

#[async_trait]
impl<E: DbExecutor> ResetActiveClipboardPort for DieselActiveClipboardRegisterRepository<E> {
    async fn reset(&self) -> Result<(), ActiveClipboardRegisterError> {
        let span = debug_span!("infra.sqlite.active_clipboard_register.reset");
        span.in_scope(|| {
            self.executor.run(move |conn| {
                // Unconditional clear: delete the single row regardless of its
                // LWW key. Deleting an absent row affects zero rows and still
                // succeeds, so the operation is idempotent.
                diesel::delete(
                    active_clipboard_register::table
                        .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID)),
                )
                .execute(conn)?;
                Ok(())
            })
        })
        .map_err(|e| ActiveClipboardRegisterError::Storage(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::executor::DieselSqliteExecutor;
    use crate::db::pool::init_db_pool;
    use tempfile::{tempdir, TempDir};
    use uc_core::ids::{DeviceId, EntryId};

    type Repo = DieselActiveClipboardRegisterRepository<DieselSqliteExecutor>;

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("active-register.sqlite");
        let path_str = path.to_str().unwrap();
        let pool_for_repo = init_db_pool(path_str).unwrap();
        let pool_for_read = init_db_pool(path_str).unwrap();
        let repo =
            DieselActiveClipboardRegisterRepository::new(DieselSqliteExecutor::new(pool_for_repo));
        (repo, DieselSqliteExecutor::new(pool_for_read), tempdir)
    }

    fn state(hash: &str, ts: i64, by: &str) -> ActiveClipboardState {
        ActiveClipboardState::new(hash, EntryId::new(), ts, DeviceId::new(by))
    }

    /// Read back the stored `(snapshot_hash, activated_at_ms, activated_by)`,
    /// or `None` when the register is still empty.
    fn read_row(executor: &DieselSqliteExecutor) -> Option<(String, i64, String)> {
        executor
            .run(|conn| {
                Ok(active_clipboard_register::table
                    .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                    .select((
                        active_clipboard_register::snapshot_hash,
                        active_clipboard_register::activated_at_ms,
                        active_clipboard_register::activated_by,
                    ))
                    .first::<(String, i64, String)>(conn)
                    .optional()?)
            })
            .unwrap()
    }

    #[tokio::test]
    async fn first_advance_inserts_and_reports_advanced() {
        let (repo, reader, _tmp) = make_repo();
        assert_eq!(read_row(&reader), None);

        let advanced = repo
            .advance(&state("blake3v1:aa", 100, "dev-a"))
            .await
            .unwrap();
        assert!(advanced);
        assert_eq!(
            read_row(&reader),
            Some(("blake3v1:aa".to_string(), 100, "dev-a".to_string()))
        );
    }

    #[tokio::test]
    async fn newer_timestamp_advances_and_overwrites() {
        let (repo, reader, _tmp) = make_repo();
        repo.advance(&state("blake3v1:aa", 100, "dev-a"))
            .await
            .unwrap();

        let advanced = repo
            .advance(&state("blake3v1:bb", 200, "dev-a"))
            .await
            .unwrap();
        assert!(advanced);
        assert_eq!(
            read_row(&reader),
            Some(("blake3v1:bb".to_string(), 200, "dev-a".to_string()))
        );
    }

    #[tokio::test]
    async fn older_timestamp_is_a_noop() {
        let (repo, reader, _tmp) = make_repo();
        repo.advance(&state("blake3v1:bb", 200, "dev-a"))
            .await
            .unwrap();

        let advanced = repo
            .advance(&state("blake3v1:aa", 100, "dev-a"))
            .await
            .unwrap();
        assert!(!advanced, "stale write must not advance");
        assert_eq!(
            read_row(&reader),
            Some(("blake3v1:bb".to_string(), 200, "dev-a".to_string())),
            "register must be unchanged after a stale write"
        );
    }

    #[tokio::test]
    async fn equal_timestamp_breaks_tie_on_activator() {
        let (repo, reader, _tmp) = make_repo();
        repo.advance(&state("blake3v1:aa", 100, "dev-a"))
            .await
            .unwrap();

        // Greater activated_by wins the tie.
        let advanced = repo
            .advance(&state("blake3v1:bb", 100, "dev-b"))
            .await
            .unwrap();
        assert!(advanced);
        assert_eq!(read_row(&reader).unwrap().2, "dev-b");

        // Lesser activated_by at the same ts is a no-op.
        let advanced = repo
            .advance(&state("blake3v1:cc", 100, "dev-a"))
            .await
            .unwrap();
        assert!(!advanced);
        assert_eq!(read_row(&reader).unwrap().2, "dev-b");
    }

    #[tokio::test]
    async fn exact_key_duplicate_is_a_noop() {
        let (repo, reader, _tmp) = make_repo();
        let s = state("blake3v1:aa", 100, "dev-a");
        repo.advance(&s).await.unwrap();

        let advanced = repo
            .advance(&state("blake3v1:aa", 100, "dev-a"))
            .await
            .unwrap();
        assert!(!advanced, "an exact-key duplicate must not advance");
        assert_eq!(read_row(&reader).unwrap().0, "blake3v1:aa");
    }

    #[tokio::test]
    async fn load_returns_none_when_register_empty() {
        let (repo, _reader, _tmp) = make_repo();
        let loaded = repo.load().await.unwrap();
        assert!(loaded.is_none(), "empty register must load as None");
    }

    #[tokio::test]
    async fn load_returns_the_full_advanced_state() {
        let (repo, _reader, _tmp) = make_repo();
        let written = ActiveClipboardState::new(
            "blake3v1:cafe",
            EntryId::from("entry-xyz"),
            4242,
            DeviceId::new("dev-load"),
        );
        repo.advance(&written).await.unwrap();

        let loaded = repo.load().await.unwrap().expect("register has a value");
        assert_eq!(loaded.snapshot_hash, "blake3v1:cafe");
        assert_eq!(loaded.entry_id.as_ref(), "entry-xyz");
        assert_eq!(loaded.activated_at_ms, 4242);
        assert_eq!(loaded.activated_by.as_str(), "dev-load");
    }

    #[tokio::test]
    async fn reset_clears_a_stored_value_unconditionally() {
        let (repo, reader, _tmp) = make_repo();
        // Store a value with a high timestamp — a value that would *win* every
        // LWW comparison, so `advance` could never overwrite it. `reset` must
        // clear it anyway.
        repo.advance(&state("blake3v1:aa", i64::MAX, "dev-z"))
            .await
            .unwrap();
        assert!(read_row(&reader).is_some());

        repo.reset().await.unwrap();
        assert_eq!(read_row(&reader), None, "reset must clear the register row");
        assert!(
            repo.load().await.unwrap().is_none(),
            "register loads as None after reset"
        );
    }

    #[tokio::test]
    async fn reset_on_empty_register_is_a_noop_success() {
        let (repo, reader, _tmp) = make_repo();
        assert_eq!(read_row(&reader), None);

        repo.reset()
            .await
            .expect("reset on empty register succeeds");
        assert_eq!(read_row(&reader), None);
    }

    #[tokio::test]
    async fn advance_after_reset_inserts_a_fresh_value() {
        let (repo, reader, _tmp) = make_repo();
        // A high-timestamp value, then reset, then a *lower*-timestamp advance:
        // with no row present the lower value must win (no stale ts blocks it).
        repo.advance(&state("blake3v1:aa", 9_000, "dev-a"))
            .await
            .unwrap();
        repo.reset().await.unwrap();

        let advanced = repo
            .advance(&state("blake3v1:bb", 100, "dev-b"))
            .await
            .unwrap();
        assert!(
            advanced,
            "after reset, any advance wins against the empty row"
        );
        assert_eq!(
            read_row(&reader),
            Some(("blake3v1:bb".to_string(), 100, "dev-b".to_string()))
        );
    }
}
