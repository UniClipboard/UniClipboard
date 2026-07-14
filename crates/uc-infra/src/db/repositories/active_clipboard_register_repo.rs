//! SQLite adapter for the single-row active-clipboard LWW register.

use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use tracing::{debug_span, warn};

use super::active_clipboard_register_cipher::{
    ActiveClipboardRegisterCipher, CONSUMABLE_HKDF_INFO,
};
use crate::db::ports::DbExecutor;
use crate::db::schema::active_clipboard_register;
use uc_core::clipboard::{ActiveClipboardState, MobileConsumableRef};
use uc_core::ids::{DeviceId, EntryId};
use uc_core::ports::clipboard::{
    ActiveClipboardRegisterError, AdvanceActiveClipboardPort,
    BackfillMobileConsumableClipboardPort, LoadActiveClipboardPort,
    LoadMobileConsumableClipboardPort, ResetActiveClipboardPort,
};
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::ports::space::{DeriveSpaceSubkeyPort, SpaceAccessError};

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
    consumable_ref_ciphertext: Option<Vec<u8>>,
}

/// SQLite adapter implementing the active-clipboard register port.
pub struct DieselActiveClipboardRegisterRepository<E> {
    executor: E,
    derive_subkey: Arc<dyn DeriveSpaceSubkeyPort>,
    current_profile: Arc<dyn CurrentProfilePort>,
}

impl<E> DieselActiveClipboardRegisterRepository<E> {
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

    async fn cipher(&self) -> Result<ActiveClipboardRegisterCipher, ActiveClipboardRegisterError> {
        let profile = self
            .current_profile
            .current_profile()
            .await
            .map_err(|err| {
                ActiveClipboardRegisterError::Storage(format!("current profile unavailable: {err}"))
            })?;
        let key = self
            .derive_subkey
            .derive_subkey(profile.as_ref().as_bytes(), CONSUMABLE_HKDF_INFO)
            .await
            .map_err(|err| match err {
                SpaceAccessError::NotUnlocked => ActiveClipboardRegisterError::NotUnlocked,
                other => ActiveClipboardRegisterError::Storage(format!(
                    "derive active-register consumable key: {other}"
                )),
            })?;
        Ok(ActiveClipboardRegisterCipher::new(key))
    }
}

#[async_trait]
impl<E: DbExecutor> AdvanceActiveClipboardPort for DieselActiveClipboardRegisterRepository<E> {
    async fn advance(
        &self,
        state: &ActiveClipboardState,
        mobile_consumable: bool,
    ) -> Result<bool, ActiveClipboardRegisterError> {
        let span = debug_span!(
            "infra.sqlite.active_clipboard_register.advance",
            snapshot_hash = %state.snapshot_hash,
            activated_at_ms = state.activated_at_ms,
            activated_by = %state.activated_by,
        );
        let consumable_ref_ciphertext = if mobile_consumable {
            Some(
                self.cipher()
                    .await?
                    .seal(&MobileConsumableRef::new(
                        state.snapshot_hash.clone(),
                        state.entry_id.clone(),
                    ))
                    .map_err(|err| ActiveClipboardRegisterError::Storage(err.to_string()))?,
            )
        } else {
            None
        };
        let row = NewRegisterRow {
            id: REGISTER_ROW_ID,
            snapshot_hash: state.snapshot_hash.clone(),
            entry_id: state.entry_id.as_ref().to_string(),
            activated_at_ms: state.activated_at_ms,
            activated_by: state.activated_by.as_str().to_string(),
            consumable_ref_ciphertext,
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

                    if current.is_none() {
                        diesel::insert_into(active_clipboard_register::table)
                            .values(&row)
                            .execute(conn)?;
                    } else if let Some(ciphertext) = &row.consumable_ref_ciphertext {
                        diesel::update(
                            active_clipboard_register::table
                                .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID)),
                        )
                        .set((
                            active_clipboard_register::snapshot_hash.eq(&row.snapshot_hash),
                            active_clipboard_register::entry_id.eq(&row.entry_id),
                            active_clipboard_register::activated_at_ms.eq(row.activated_at_ms),
                            active_clipboard_register::activated_by.eq(&row.activated_by),
                            active_clipboard_register::consumable_ref_ciphertext.eq(ciphertext),
                        ))
                        .execute(conn)?;
                    } else {
                        diesel::update(
                            active_clipboard_register::table
                                .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID)),
                        )
                        .set((
                            active_clipboard_register::snapshot_hash.eq(&row.snapshot_hash),
                            active_clipboard_register::entry_id.eq(&row.entry_id),
                            active_clipboard_register::activated_at_ms.eq(row.activated_at_ms),
                            active_clipboard_register::activated_by.eq(&row.activated_by),
                        ))
                        .execute(conn)?;
                    }
                    Ok(true)
                })
                .map_err(|e| anyhow::anyhow!(e.to_string()))
            })
        })
        .map_err(|e| ActiveClipboardRegisterError::Storage(e.to_string()))
    }
}

#[async_trait]
impl<E: DbExecutor> LoadMobileConsumableClipboardPort
    for DieselActiveClipboardRegisterRepository<E>
{
    async fn load_mobile_consumable(
        &self,
    ) -> Result<Option<MobileConsumableRef>, ActiveClipboardRegisterError> {
        let ciphertext: Option<Vec<u8>> = self
            .executor
            .run(move |conn| {
                Ok(active_clipboard_register::table
                    .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                    .select(active_clipboard_register::consumable_ref_ciphertext)
                    .first::<Option<Vec<u8>>>(conn)
                    .optional()?
                    .flatten())
            })
            .map_err(|err| ActiveClipboardRegisterError::Storage(err.to_string()))?;
        let Some(ciphertext) = ciphertext else {
            return Ok(None);
        };
        match self.cipher().await?.open(&ciphertext) {
            Ok(reference) => Ok(Some(reference)),
            Err(err) => {
                // The reference is a rebuildable shadow of the register: an
                // unreadable envelope must degrade the mobile snapshot to
                // "nothing consumable", never break the read path. Discard it
                // so the warning does not repeat on every poll; the next
                // consumable advance or unlock backfill rewrites the column.
                warn!(
                    error = %err,
                    "mobile-consumable reference ciphertext is unreadable; discarding it"
                );
                // CAS on the exact unreadable bytes: a concurrent advance or
                // backfill may have replaced the column with a valid envelope
                // between the read above and this clear, and that value must
                // survive.
                if let Err(clear_err) = self.executor.run(move |conn| {
                    diesel::update(
                        active_clipboard_register::table
                            .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                            .filter(
                                active_clipboard_register::consumable_ref_ciphertext
                                    .eq(&ciphertext),
                            ),
                    )
                    .set(active_clipboard_register::consumable_ref_ciphertext.eq(None::<Vec<u8>>))
                    .execute(conn)?;
                    Ok(())
                }) {
                    warn!(
                        error = %clear_err,
                        "failed to discard unreadable mobile-consumable ciphertext"
                    );
                }
                Ok(None)
            }
        }
    }
}

#[async_trait]
impl<E: DbExecutor> BackfillMobileConsumableClipboardPort
    for DieselActiveClipboardRegisterRepository<E>
{
    async fn backfill_mobile_consumable_if_current(
        &self,
        reference: &MobileConsumableRef,
    ) -> Result<bool, ActiveClipboardRegisterError> {
        let ciphertext = self
            .cipher()
            .await?
            .seal(reference)
            .map_err(|err| ActiveClipboardRegisterError::Storage(err.to_string()))?;
        let snapshot_hash = reference.snapshot_hash.clone();
        let entry_id = reference.entry_id.as_ref().to_string();
        self.executor
            .run(move |conn| {
                let changed = diesel::update(
                    active_clipboard_register::table
                        .filter(active_clipboard_register::id.eq(REGISTER_ROW_ID))
                        .filter(active_clipboard_register::consumable_ref_ciphertext.is_null())
                        .filter(active_clipboard_register::snapshot_hash.eq(snapshot_hash))
                        .filter(active_clipboard_register::entry_id.eq(entry_id)),
                )
                .set(active_clipboard_register::consumable_ref_ciphertext.eq(ciphertext))
                .execute(conn)?;
                Ok(changed == 1)
            })
            .map_err(|err| ActiveClipboardRegisterError::Storage(err.to_string()))
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
    use uc_core::ids::{DeviceId, EntryId, ProfileId};
    use uc_core::ports::security::current_profile::CurrentProfileError;

    struct FixedSubkey;

    #[async_trait]
    impl DeriveSpaceSubkeyPort for FixedSubkey {
        async fn derive_subkey(
            &self,
            _salt: &[u8],
            _info: &[u8],
        ) -> Result<[u8; 32], SpaceAccessError> {
            Ok([0x5A; 32])
        }
    }

    struct FixedProfile;

    #[async_trait]
    impl CurrentProfilePort for FixedProfile {
        async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
            Ok(ProfileId::from("default"))
        }
    }

    type Repo = DieselActiveClipboardRegisterRepository<DieselSqliteExecutor>;

    fn make_repo() -> (Repo, DieselSqliteExecutor, TempDir) {
        let tempdir = tempdir().unwrap();
        let path = tempdir.path().join("active-register.sqlite");
        let path_str = path.to_str().unwrap();
        let pool_for_repo = init_db_pool(path_str).unwrap();
        let pool_for_read = init_db_pool(path_str).unwrap();
        let repo = DieselActiveClipboardRegisterRepository::new(
            DieselSqliteExecutor::new(pool_for_repo),
            Arc::new(FixedSubkey),
            Arc::new(FixedProfile),
        );
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
            .advance(&state("blake3v1:aa", 100, "dev-a"), true)
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
        repo.advance(&state("blake3v1:aa", 100, "dev-a"), true)
            .await
            .unwrap();

        let advanced = repo
            .advance(&state("blake3v1:bb", 200, "dev-a"), true)
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
        repo.advance(&state("blake3v1:bb", 200, "dev-a"), true)
            .await
            .unwrap();

        let advanced = repo
            .advance(&state("blake3v1:aa", 100, "dev-a"), true)
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
        repo.advance(&state("blake3v1:aa", 100, "dev-a"), true)
            .await
            .unwrap();

        // Greater activated_by wins the tie.
        let advanced = repo
            .advance(&state("blake3v1:bb", 100, "dev-b"), true)
            .await
            .unwrap();
        assert!(advanced);
        assert_eq!(read_row(&reader).unwrap().2, "dev-b");

        // Lesser activated_by at the same ts is a no-op.
        let advanced = repo
            .advance(&state("blake3v1:cc", 100, "dev-a"), true)
            .await
            .unwrap();
        assert!(!advanced);
        assert_eq!(read_row(&reader).unwrap().2, "dev-b");
    }

    #[tokio::test]
    async fn exact_key_duplicate_is_a_noop() {
        let (repo, reader, _tmp) = make_repo();
        let s = state("blake3v1:aa", 100, "dev-a");
        repo.advance(&s, true).await.unwrap();

        let advanced = repo
            .advance(&state("blake3v1:aa", 100, "dev-a"), true)
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
        repo.advance(&written, true).await.unwrap();

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
        repo.advance(&state("blake3v1:aa", i64::MAX, "dev-z"), true)
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
        repo.advance(&state("blake3v1:aa", 9_000, "dev-a"), true)
            .await
            .unwrap();
        repo.reset().await.unwrap();

        let advanced = repo
            .advance(&state("blake3v1:bb", 100, "dev-b"), true)
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

    fn exact_state(hash: &str, entry_id: &str, ts: i64) -> ActiveClipboardState {
        ActiveClipboardState::new(hash, EntryId::from(entry_id), ts, DeviceId::new("dev-a"))
    }

    #[tokio::test]
    async fn consumable_advances_replace_shadow_while_directory_advances_preserve_it() {
        let (repo, _reader, _tmp) = make_repo();
        let first = exact_state("blake3v1:first", "entry-first", 1);
        repo.advance(&first, true).await.unwrap();
        assert_eq!(
            repo.load_mobile_consumable().await.unwrap(),
            Some(MobileConsumableRef::new(
                "blake3v1:first",
                EntryId::from("entry-first")
            ))
        );

        repo.advance(
            &exact_state("blake3v1:directory-a", "entry-directory-a", 2),
            false,
        )
        .await
        .unwrap();
        repo.advance(
            &exact_state("blake3v1:directory-b", "entry-directory-b", 3),
            false,
        )
        .await
        .unwrap();
        assert_eq!(
            repo.load_mobile_consumable().await.unwrap(),
            Some(MobileConsumableRef::new(
                "blake3v1:first",
                EntryId::from("entry-first")
            ))
        );

        let next = exact_state("blake3v1:next", "entry-next", 4);
        repo.advance(&next, true).await.unwrap();
        assert_eq!(
            repo.load_mobile_consumable().await.unwrap(),
            Some(MobileConsumableRef::new(
                "blake3v1:next",
                EntryId::from("entry-next")
            ))
        );
    }

    #[tokio::test]
    async fn first_non_consumable_advance_leaves_shadow_empty() {
        let (repo, _reader, _tmp) = make_repo();
        repo.advance(
            &exact_state("blake3v1:directory", "entry-directory", 1),
            false,
        )
        .await
        .unwrap();
        assert_eq!(repo.load_mobile_consumable().await.unwrap(), None);
    }

    #[tokio::test]
    async fn stored_shadow_ciphertext_contains_neither_hash_nor_entry_id() {
        let (repo, reader, _tmp) = make_repo();
        let state = exact_state(
            "blake3v1:plaintext-must-not-survive",
            "entry-plaintext-must-not-survive",
            1,
        );
        repo.advance(&state, true).await.unwrap();
        let ciphertext: Vec<u8> = reader
            .run(|conn| {
                Ok(active_clipboard_register::table
                    .select(active_clipboard_register::consumable_ref_ciphertext)
                    .first::<Option<Vec<u8>>>(conn)?
                    .expect("ciphertext"))
            })
            .unwrap();
        assert!(!ciphertext
            .windows(state.snapshot_hash.len())
            .any(|window| window == state.snapshot_hash.as_bytes()));
        assert!(!ciphertext
            .windows(state.entry_id.as_ref().len())
            .any(|window| window == state.entry_id.as_ref().as_bytes()));
    }

    #[tokio::test]
    async fn backfill_writes_only_when_hash_and_entry_still_match() {
        let (repo, _reader, _tmp) = make_repo();
        let old = exact_state("blake3v1:same", "entry-old", 1);
        repo.advance(&old, false).await.unwrap();

        let new = exact_state("blake3v1:same", "entry-new", 2);
        repo.advance(&new, false).await.unwrap();
        assert!(!repo
            .backfill_mobile_consumable_if_current(&MobileConsumableRef::new(
                old.snapshot_hash,
                old.entry_id,
            ))
            .await
            .unwrap());
        assert_eq!(repo.load_mobile_consumable().await.unwrap(), None);

        assert!(repo
            .backfill_mobile_consumable_if_current(&MobileConsumableRef::new(
                new.snapshot_hash.clone(),
                new.entry_id.clone(),
            ))
            .await
            .unwrap());
        assert_eq!(
            repo.load_mobile_consumable().await.unwrap(),
            Some(MobileConsumableRef::new(new.snapshot_hash, new.entry_id))
        );
    }

    #[tokio::test]
    async fn backfill_does_not_replace_an_existing_shadow() {
        let (repo, _reader, _tmp) = make_repo();
        let original = exact_state("blake3v1:original", "entry-original", 1);
        repo.advance(&original, true).await.unwrap();
        let current = exact_state("blake3v1:current", "entry-current", 2);
        repo.advance(&current, false).await.unwrap();

        assert!(!repo
            .backfill_mobile_consumable_if_current(&MobileConsumableRef::new(
                current.snapshot_hash,
                current.entry_id,
            ))
            .await
            .unwrap());
        assert_eq!(
            repo.load_mobile_consumable().await.unwrap(),
            Some(MobileConsumableRef::new(
                original.snapshot_hash,
                original.entry_id
            ))
        );
    }

    #[tokio::test]
    async fn unreadable_shadow_ciphertext_degrades_to_none_and_is_discarded() {
        let (repo, reader, _tmp) = make_repo();
        repo.advance(&exact_state("blake3v1:aa", "entry-aa", 1), true)
            .await
            .unwrap();

        // Corrupt the stored envelope out-of-band.
        reader
            .run(|conn| {
                diesel::update(active_clipboard_register::table)
                    .set(active_clipboard_register::consumable_ref_ciphertext.eq(vec![0u8; 8]))
                    .execute(conn)?;
                Ok(())
            })
            .unwrap();

        assert_eq!(repo.load_mobile_consumable().await.unwrap(), None);

        // The unreadable envelope must be discarded, not re-decrypted (and
        // re-warned about) on every subsequent read.
        let remaining: Option<Vec<u8>> = reader
            .run(|conn| {
                Ok(active_clipboard_register::table
                    .select(active_clipboard_register::consumable_ref_ciphertext)
                    .first::<Option<Vec<u8>>>(conn)?)
            })
            .unwrap();
        assert_eq!(remaining, None);
    }
}
