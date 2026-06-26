//! SQLite implementation of `SearchIndexPort`.
//!
//! `SqliteSearchIndex` is the single authoritative adapter for the local encrypted
//! search index. It owns:
//! - Meta-row seeding / loading per profile
//! - Live active-table upsert / hard-delete for `search_document` + `search_posting`
//! - Blocked-state and version-mismatch guards for `search()`
//! - Real SQLite posting-based AND/OR query resolution
//! - Rebuild lifecycle: temp-table workspace, blocked state, finalize cutover, failure handling
//!
//! Phase 92 will wire this adapter into daemon routes.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::RunQueryDsl;
use tokio::sync::mpsc::Sender;
use tracing::{debug, instrument, warn};

use uc_core::ids::{EntryId, ProfileId};
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::security::current_profile::CurrentProfilePort;
use uc_core::search::document::{SearchDocument, SearchIndexMeta, SearchPosting};
use uc_core::search::error::SearchError;
use uc_core::search::query::{QueryOperator, SearchQuery, TimeRangeFilter};
use uc_core::search::result::{RebuildProgress, RebuildStage, SearchResult, SearchResultsPage};

use crate::db::pool::DbPool;
use crate::db::schema::{search_document, search_index_meta, search_posting};
use crate::search::constants::CURRENT_INDEX_VERSION;
use crate::search::rows::{
    NewSearchDocumentRow, NewSearchIndexMetaRow, NewSearchPostingRow, SearchDocumentRow,
    SearchIndexMetaRow,
};
use crate::search::search_key_derivation::term_tag;
use crate::search::tokenizer::SearchTokenizer;

// ──────────────────────────────────────────────────────────────────────────────
// Rebuild workspace state
// ──────────────────────────────────────────────────────────────────────────────

/// In-memory state for an in-progress index rebuild.
///
/// Held in `SqliteSearchIndex::rebuild_state` behind a `std::sync::RwLock`.
/// Cloned by `active_rebuild_for_profile` to allow live mutations to mirror
/// into the temp tables without holding the lock during the DB write.
#[derive(Debug, Clone)]
pub struct ActiveRebuild {
    pub profile_id: String,
    pub temp_document_table: String,
    pub temp_posting_table: String,
    pub target_version: String,
}

impl ActiveRebuild {
    /// Construct an `ActiveRebuild` for `profile_id`.
    ///
    /// Temp table names are deterministic from the profile ID (hex-encoded bytes so
    /// the names are safe SQL identifiers regardless of profile ID content).
    pub fn new(profile_id: &str) -> Self {
        // Hex-encode profile_id bytes for a safe SQL identifier suffix.
        let safe_suffix: String = profile_id
            .bytes()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join("");
        // Cap at 40 chars to keep table names short.
        let safe_suffix = &safe_suffix[..safe_suffix.len().min(40)];

        Self {
            profile_id: profile_id.to_string(),
            temp_document_table: format!("tmp_search_document_rebuild_{safe_suffix}"),
            temp_posting_table: format!("tmp_search_posting_rebuild_{safe_suffix}"),
            target_version: CURRENT_INDEX_VERSION.to_string(),
        }
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Public adapter struct
// ──────────────────────────────────────────────────────────────────────────────

/// SQLite adapter implementing `SearchIndexPort`.
///
/// Holds a connection pool and the two async ports needed for profile-scoped
/// key derivation. `rebuild_state` is a `std::sync::RwLock` so that live write
/// helpers inside `spawn_blocking` can check for an active rebuild without
/// crossing the async/blocking boundary.
pub struct SqliteSearchIndex {
    pool: DbPool,
    current_profile: Arc<dyn CurrentProfilePort>,
    search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    /// Active rebuild state, shared between the rebuild coordinator and the
    /// live write/delete helpers that must mirror into temp tables.
    rebuild_state: Arc<std::sync::RwLock<Option<ActiveRebuild>>>,

    /// Test-only: inject a fault after this many entries have been written to temp tables.
    #[cfg(test)]
    pub fail_after_n_entries: Option<usize>,

    /// Test-only: pause just before finalize so tests can inject mutations mid-rebuild.
    /// The rebuild task waits for a permit on this semaphore before calling finalize.
    #[cfg(test)]
    pub pause_before_finalize: Option<Arc<tokio::sync::Semaphore>>,
}

impl SqliteSearchIndex {
    /// Create a new `SqliteSearchIndex`.
    pub fn new(
        pool: DbPool,
        current_profile: Arc<dyn CurrentProfilePort>,
        search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    ) -> Self {
        Self {
            pool,
            current_profile,
            search_key_derivation,
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            #[cfg(test)]
            fail_after_n_entries: None,
            #[cfg(test)]
            pause_before_finalize: None,
        }
    }

    // ─── Private async helpers ────────────────────────────────────────────────

    /// Resolve the current `ProfileId` from the `CurrentProfilePort`.
    ///
    /// 返回值对象而非 String:call site 利用 `ProfileId: Deref<Target=String>`
    /// 直接在 `&str` / `bytes()` / SQL bind 场景透明解引用;仅在需要拥有
    /// 所有权的 `String`(move 进 spawn_blocking、HashMap key)时显式
    /// `into_inner()` 或 `clone`。
    async fn current_profile_id(&self) -> Result<ProfileId, SearchError> {
        self.current_profile
            .current_profile()
            .await
            .map_err(|e| SearchError::Internal(format!("failed to get current profile: {e}")))
    }

    /// Return a clone of the active rebuild state only when `profile_id` matches.
    ///
    /// Returns `None` if there is no active rebuild or the rebuild is for a different profile.
    ///
    /// Declared `async` so callers in async context can use `await` directly;
    /// the implementation is synchronous (uses `std::sync::RwLock`) and does
    /// not actually suspend.
    async fn active_rebuild_for_profile(&self, profile_id: &str) -> Option<ActiveRebuild> {
        let guard = self.rebuild_state.read().expect("rebuild_state poisoned");
        guard.as_ref().and_then(|r| {
            if r.profile_id == profile_id {
                Some(r.clone())
            } else {
                None
            }
        })
    }

    // ─── Private synchronous helpers (run inside spawn_blocking) ─────────────

    /// Ensure a `search_index_meta` row exists for `profile_id`.
    ///
    /// If the row is missing, inserts a fresh seed row via `NewSearchIndexMetaRow::seed`.
    fn ensure_meta_row(conn: &mut SqliteConnection, profile_id: &str) -> Result<(), SearchError> {
        let seed = NewSearchIndexMetaRow::seed(profile_id);
        let inserted = diesel::insert_into(search_index_meta::table)
            .values(&seed)
            .on_conflict(search_index_meta::profile_id)
            .do_nothing()
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("meta row seed failed: {e}")))?;

        if inserted > 0 {
            debug!(profile_id, "search_index_meta row seeded");
        }

        Ok(())
    }

    /// Load `SearchIndexMeta` for `profile_id`.
    ///
    /// Callers should call `ensure_meta_row` first so this never returns `NotFound`.
    fn load_meta(
        conn: &mut SqliteConnection,
        profile_id: &str,
    ) -> Result<SearchIndexMeta, SearchError> {
        use crate::db::schema::search_index_meta::dsl;

        let row = dsl::search_index_meta
            .filter(dsl::profile_id.eq(profile_id))
            .first::<SearchIndexMetaRow>(conn)
            .map_err(|e| SearchError::Internal(format!("load_meta query failed: {e}")))?;

        Ok(row.to_domain())
    }

    /// Upsert a `search_document` row and replace all `search_posting` rows for the entry.
    ///
    /// Runs inside a single transaction:
    /// 1. Delete existing `search_posting` rows for `(profile_id, entry_id)`.
    /// 2. Upsert (insert or replace) the `search_document` row.
    /// 3. Insert new posting rows.
    fn upsert_active_entry(
        conn: &mut SqliteConnection,
        profile_id: &str,
        document: &SearchDocument,
        postings: &[SearchPosting],
    ) -> Result<(), SearchError> {
        conn.transaction::<(), diesel::result::Error, _>(|tx| {
            let entry_id_str = document.entry_id.to_string();

            // 1. Delete existing postings for this entry.
            diesel::delete(
                search_posting::table
                    .filter(search_posting::profile_id.eq(profile_id))
                    .filter(search_posting::entry_id.eq(&entry_id_str)),
            )
            .execute(tx)?;

            // 2. Upsert (insert or replace) the document row.
            let doc_row = NewSearchDocumentRow::from_domain(profile_id, document)
                .map_err(|_e| diesel::result::Error::RollbackTransaction)?;

            diesel::replace_into(search_document::table)
                .values(&doc_row)
                .execute(tx)?;

            // 3. Insert new postings.
            let posting_rows: Vec<NewSearchPostingRow> = postings
                .iter()
                .map(|p| NewSearchPostingRow::from_domain(profile_id, p))
                .collect();

            if !posting_rows.is_empty() {
                diesel::insert_into(search_posting::table)
                    .values(&posting_rows)
                    .execute(tx)?;
            }

            Ok(())
        })
        .map_err(|e| SearchError::Internal(format!("upsert_active_entry failed: {e}")))
    }

    /// Hard-delete `search_document` and all `search_posting` rows for `entry_id`.
    ///
    /// Runs inside a single transaction: postings first, then document.
    fn delete_active_entry(
        conn: &mut SqliteConnection,
        profile_id: &str,
        entry_id: &EntryId,
    ) -> Result<(), SearchError> {
        let entry_id_str = entry_id.to_string();

        conn.transaction::<(), diesel::result::Error, _>(|tx| {
            // Delete postings first (foreign-key ordering not strictly required here
            // since we're not using FK cascades on search tables, but ordering is
            // the safe convention).
            diesel::delete(
                search_posting::table
                    .filter(search_posting::profile_id.eq(profile_id))
                    .filter(search_posting::entry_id.eq(&entry_id_str)),
            )
            .execute(tx)?;

            diesel::delete(
                search_document::table
                    .filter(search_document::profile_id.eq(profile_id))
                    .filter(search_document::entry_id.eq(&entry_id_str)),
            )
            .execute(tx)?;

            Ok(())
        })
        .map_err(|e| SearchError::Internal(format!("delete_active_entry failed: {e}")))
    }

    // ─── Search helpers ───────────────────────────────────────────────────────

    /// Update `search_index_meta.search_blocked = true` for `profile_id`.
    fn mark_blocked(conn: &mut SqliteConnection, profile_id: &str) -> Result<(), SearchError> {
        use crate::db::schema::search_index_meta::dsl;

        diesel::update(dsl::search_index_meta.filter(dsl::profile_id.eq(profile_id)))
            .set(dsl::search_blocked.eq(true))
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("mark_blocked failed: {e}")))?;

        Ok(())
    }

    /// Normalize and tokenize a query string into distinct search terms.
    ///
    /// The query string is split on whitespace first, then each word-level token
    /// is individually tokenized and de-duplicated. This avoids the tokenizer
    /// treating the full query string as an identifier (e.g., "alpha beta" would
    /// produce a spurious "alpha beta" whole-segment token in addition to the
    /// individual "alpha" and "beta" tokens if the whole string were passed as
    /// a single `tokenize_segment` call).
    ///
    /// Returns an empty `Vec` when the query string is blank — this is valid for
    /// filter-only searches (e.g. `contentTypes=text` with no keywords).
    /// Returns `SearchError::InvalidQuery` only when the query string is non-empty
    /// but produces no searchable terms after tokenization.
    fn normalize_query_terms(query: &SearchQuery) -> Result<Vec<String>, SearchError> {
        let trimmed = query.query_string.trim();
        if trimmed.is_empty() {
            return Ok(vec![]);
        }

        let tokenizer = SearchTokenizer;

        // Split on whitespace to get individual query words, then tokenize each.
        // This prevents multi-word query strings from generating whole-segment tokens.
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let segments: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        // No prefix expansion at query time: the user's partial term (e.g. "uniclip")
        // is searched as an exact token, matching the prefix tokens stored at index time.
        let raw_tokens = tokenizer.tokenize_all_no_prefix(&segments);

        // De-duplicate while preserving first-occurrence order.
        let mut seen = std::collections::HashSet::new();
        let mut unique: Vec<String> = Vec::new();
        for tok in raw_tokens {
            if seen.insert(tok.clone()) {
                unique.push(tok);
            }
        }

        if unique.is_empty() {
            return Err(SearchError::InvalidQuery(
                "query produced no searchable terms".to_string(),
            ));
        }

        Ok(unique)
    }

    /// Query `search_posting` for candidate entries and their hit counts.
    ///
    /// Returns `HashMap<entry_id, hit_count>`.
    ///
    /// - AND mode: entry must match all `term_tags` — enforced by requiring
    ///   `HAVING COUNT(DISTINCT term_tag) = len(term_tags)`
    /// - OR mode:  entry must match at least one tag
    ///
    /// Implementation: load all matching postings via Diesel's `eq_any`, then
    /// aggregate in Rust. This avoids dynamic SQL parameter building while still
    /// implementing the correct AND/OR semantics.
    fn query_candidate_hits(
        conn: &mut SqliteConnection,
        profile_id: &str,
        term_tags: &[Vec<u8>],
        operator: &QueryOperator,
    ) -> Result<HashMap<String, u32>, SearchError> {
        if term_tags.is_empty() {
            return Ok(HashMap::new());
        }

        use crate::db::schema::search_posting::dsl as sp;

        // Load all posting rows where profile_id matches and term_tag is one of the query tags.
        let matching_rows = sp::search_posting
            .filter(sp::profile_id.eq(profile_id))
            .filter(sp::term_tag.eq_any(term_tags))
            .select((sp::entry_id, sp::term_tag))
            .load::<(String, Vec<u8>)>(conn)
            .map_err(|e| SearchError::Internal(format!("posting query failed: {e}")))?;

        if matching_rows.is_empty() {
            return Ok(HashMap::new());
        }

        // Aggregate: per entry_id, collect the set of distinct matched term_tags
        // and total hit count (number of tag matches).
        let mut per_entry: HashMap<String, std::collections::HashSet<Vec<u8>>> = HashMap::new();
        for (entry_id, tag) in matching_rows {
            per_entry.entry(entry_id).or_default().insert(tag);
        }

        // AND semantics mirror SQL: HAVING COUNT(DISTINCT term_tag) = term_count
        // OR  semantics mirror SQL: HAVING COUNT(DISTINCT term_tag) >= 1
        let term_count = term_tags.len();
        let mut result: HashMap<String, u32> = HashMap::new();

        for (entry_id, matched_tags) in per_entry {
            let distinct_hit_count = matched_tags.len();
            let include = match operator {
                // AND: entry must contain all queried terms.
                QueryOperator::And => distinct_hit_count == term_count,
                // OR: entry must contain at least one term.
                QueryOperator::Or => distinct_hit_count >= 1,
            };
            if include {
                result.insert(entry_id, distinct_hit_count as u32);
            }
        }

        Ok(result)
    }

    /// Load `search_document` rows for the given entry IDs.
    fn load_candidate_documents(
        conn: &mut SqliteConnection,
        profile_id: &str,
        entry_ids: &[String],
    ) -> Result<Vec<SearchDocumentRow>, SearchError> {
        if entry_ids.is_empty() {
            return Ok(vec![]);
        }

        use crate::db::schema::search_document::dsl;

        let rows = dsl::search_document
            .filter(dsl::profile_id.eq(profile_id))
            .filter(dsl::entry_id.eq_any(entry_ids))
            .load::<SearchDocumentRow>(conn)
            .map_err(|e| SearchError::Internal(format!("load_candidate_documents failed: {e}")))?;

        Ok(rows)
    }

    /// Load all `search_document` rows for a profile (filter-only search path).
    fn load_all_documents(
        conn: &mut SqliteConnection,
        profile_id: &str,
    ) -> Result<Vec<SearchDocumentRow>, SearchError> {
        use crate::db::schema::search_document::dsl;

        let rows = dsl::search_document
            .filter(dsl::profile_id.eq(profile_id))
            .load::<SearchDocumentRow>(conn)
            .map_err(|e| SearchError::Internal(format!("load_all_documents failed: {e}")))?;

        Ok(rows)
    }

    /// Load the set of `event_id`s whose originating `source_device` is one of
    /// `source_devices`. Used to restrict search results by provenance.
    fn load_event_ids_for_sources(
        conn: &mut SqliteConnection,
        source_devices: &[String],
    ) -> Result<HashSet<String>, SearchError> {
        use crate::db::schema::clipboard_event::dsl;

        let rows = dsl::clipboard_event
            .filter(dsl::source_device.eq_any(source_devices))
            .select(dsl::event_id)
            .load::<String>(conn)
            .map_err(|e| {
                SearchError::Internal(format!("load_event_ids_for_sources failed: {e}"))
            })?;

        Ok(rows.into_iter().collect())
    }

    // ─── Rebuild helpers ──────────────────────────────────────────────────────

    /// Create the two temp tables (`tmp_search_document_rebuild_*` and
    /// `tmp_search_posting_rebuild_*`) with the same columns as the active tables.
    fn create_rebuild_tables(
        conn: &mut SqliteConnection,
        state: &ActiveRebuild,
    ) -> Result<(), SearchError> {
        // Temp document table: same columns as search_document.
        let create_doc = format!(
            "CREATE TABLE IF NOT EXISTS {doc_table} (
                profile_id TEXT NOT NULL,
                entry_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                active_time_ms INTEGER NOT NULL,
                captured_at_ms INTEGER NOT NULL,
                file_type TEXT NOT NULL,
                file_extensions TEXT NOT NULL DEFAULT '[]',
                mime_type TEXT NOT NULL DEFAULT '',
                indexed_at_ms INTEGER NOT NULL,
                index_version TEXT NOT NULL,
                text_preview TEXT,
                PRIMARY KEY (profile_id, entry_id)
            )",
            doc_table = state.temp_document_table
        );

        // Temp posting table: same columns as search_posting.
        let create_posting = format!(
            "CREATE TABLE IF NOT EXISTS {post_table} (
                profile_id TEXT NOT NULL,
                term_tag BLOB NOT NULL,
                entry_id TEXT NOT NULL,
                field_mask INTEGER NOT NULL DEFAULT 0,
                term_freq INTEGER NOT NULL DEFAULT 1 CHECK (term_freq > 0),
                PRIMARY KEY (profile_id, term_tag, entry_id)
            )",
            post_table = state.temp_posting_table
        );

        diesel::sql_query(&create_doc)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("create temp doc table failed: {e}")))?;

        diesel::sql_query(&create_posting)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("create temp posting table failed: {e}")))?;

        debug!(
            profile_id = %state.profile_id,
            doc_table = %state.temp_document_table,
            posting_table = %state.temp_posting_table,
            "rebuild temp tables created"
        );

        Ok(())
    }

    /// Drop the two temp tables best-effort. Errors are logged but not returned.
    fn drop_rebuild_tables(conn: &mut SqliteConnection, state: &ActiveRebuild) {
        let drop_doc = format!("DROP TABLE IF EXISTS {}", state.temp_document_table);
        let drop_posting = format!("DROP TABLE IF EXISTS {}", state.temp_posting_table);

        if let Err(e) = diesel::sql_query(&drop_doc).execute(conn) {
            warn!(table = %state.temp_document_table, error = %e, "failed to drop temp doc table");
        }
        if let Err(e) = diesel::sql_query(&drop_posting).execute(conn) {
            warn!(table = %state.temp_posting_table, error = %e, "failed to drop temp posting table");
        }
    }

    /// Write one `(SearchDocument, Vec<SearchPosting>)` pair into the rebuild temp tables.
    ///
    /// Deletes any existing staged postings for `(profile_id, entry_id)` first,
    /// then inserts the document row and new postings. This makes the function
    /// idempotent and ensures mid-rebuild `index_entry()` mirrors replace correctly.
    fn insert_temp_entry(
        conn: &mut SqliteConnection,
        state: &ActiveRebuild,
        document: &SearchDocument,
        postings: &[SearchPosting],
    ) -> Result<(), SearchError> {
        let profile_id = &state.profile_id;
        let entry_id_str = document.entry_id.to_string();

        // Delete existing temp postings for this entry (idempotent upsert).
        let del_postings = format!(
            "DELETE FROM {post_table} WHERE profile_id = ? AND entry_id = ?",
            post_table = state.temp_posting_table
        );
        diesel::sql_query(&del_postings)
            .bind::<diesel::sql_types::Text, _>(profile_id)
            .bind::<diesel::sql_types::Text, _>(&entry_id_str)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("delete temp postings failed: {e}")))?;

        // Build document values for INSERT OR REPLACE.
        let doc_row = NewSearchDocumentRow::from_domain(profile_id, document)
            .map_err(|e| SearchError::Internal(format!("from_domain failed: {e}")))?;

        let insert_doc = format!(
            "INSERT OR REPLACE INTO {doc_table}
             (profile_id, entry_id, event_id, active_time_ms, captured_at_ms,
              file_type, file_extensions, mime_type, indexed_at_ms, index_version, text_preview)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            doc_table = state.temp_document_table
        );
        diesel::sql_query(&insert_doc)
            .bind::<diesel::sql_types::Text, _>(&doc_row.profile_id)
            .bind::<diesel::sql_types::Text, _>(&doc_row.entry_id)
            .bind::<diesel::sql_types::Text, _>(&doc_row.event_id)
            .bind::<diesel::sql_types::BigInt, _>(doc_row.active_time_ms)
            .bind::<diesel::sql_types::BigInt, _>(doc_row.captured_at_ms)
            .bind::<diesel::sql_types::Text, _>(&doc_row.file_type)
            .bind::<diesel::sql_types::Text, _>(&doc_row.file_extensions)
            .bind::<diesel::sql_types::Text, _>(&doc_row.mime_type)
            .bind::<diesel::sql_types::BigInt, _>(doc_row.indexed_at_ms)
            .bind::<diesel::sql_types::Text, _>(&doc_row.index_version)
            .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(&doc_row.text_preview)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("insert temp doc failed: {e}")))?;

        // Insert postings.
        for posting in postings {
            let post_row = NewSearchPostingRow::from_domain(profile_id, posting);
            let insert_posting = format!(
                "INSERT OR REPLACE INTO {post_table}
                 (profile_id, term_tag, entry_id, field_mask, term_freq)
                 VALUES (?, ?, ?, ?, ?)",
                post_table = state.temp_posting_table
            );
            diesel::sql_query(&insert_posting)
                .bind::<diesel::sql_types::Text, _>(&post_row.profile_id)
                .bind::<diesel::sql_types::Binary, _>(&post_row.term_tag)
                .bind::<diesel::sql_types::Text, _>(&post_row.entry_id)
                .bind::<diesel::sql_types::Integer, _>(post_row.field_mask)
                .bind::<diesel::sql_types::Integer, _>(post_row.term_freq)
                .execute(conn)
                .map_err(|e| SearchError::Internal(format!("insert temp posting failed: {e}")))?;
        }

        Ok(())
    }

    /// Delete an entry from the rebuild temp tables.
    ///
    /// Called when `remove_entry()` is called while a rebuild is in progress,
    /// ensuring deleted entries are not resurrected after cutover.
    fn delete_temp_entry(
        conn: &mut SqliteConnection,
        state: &ActiveRebuild,
        entry_id: &EntryId,
    ) -> Result<(), SearchError> {
        let profile_id = &state.profile_id;
        let entry_id_str = entry_id.to_string();

        let del_postings = format!(
            "DELETE FROM {post_table} WHERE profile_id = ? AND entry_id = ?",
            post_table = state.temp_posting_table
        );
        diesel::sql_query(&del_postings)
            .bind::<diesel::sql_types::Text, _>(profile_id)
            .bind::<diesel::sql_types::Text, _>(&entry_id_str)
            .execute(conn)
            .map_err(|e| {
                SearchError::Internal(format!("delete_temp_entry postings failed: {e}"))
            })?;

        let del_doc = format!(
            "DELETE FROM {doc_table} WHERE profile_id = ? AND entry_id = ?",
            doc_table = state.temp_document_table
        );
        diesel::sql_query(&del_doc)
            .bind::<diesel::sql_types::Text, _>(profile_id)
            .bind::<diesel::sql_types::Text, _>(&entry_id_str)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("delete_temp_entry doc failed: {e}")))?;

        Ok(())
    }

    /// Finalize the rebuild by copying temp rows into the active tables in one transaction.
    ///
    /// Transaction sequence:
    /// 1. Delete active `search_posting` rows for `profile_id`
    /// 2. Delete active `search_document` rows for `profile_id`
    /// 3. INSERT ... SELECT from temp posting table
    /// 4. INSERT ... SELECT from temp document table
    /// 5. Update `search_index_meta`: version, unblock, completed_at_ms
    fn finalize_rebuild(
        conn: &mut SqliteConnection,
        state: &ActiveRebuild,
        completed_at_ms: i64,
    ) -> Result<(), SearchError> {
        let profile_id = &state.profile_id;
        let target_version = &state.target_version;

        conn.transaction::<(), diesel::result::Error, _>(|tx| {
            // 1. Delete active postings for profile.
            diesel::delete(search_posting::table.filter(search_posting::profile_id.eq(profile_id)))
                .execute(tx)?;

            // 2. Delete active documents for profile.
            diesel::delete(
                search_document::table.filter(search_document::profile_id.eq(profile_id)),
            )
            .execute(tx)?;

            // 3. Copy temp postings into active table.
            let copy_postings = format!(
                "INSERT INTO search_posting
                 SELECT profile_id, term_tag, entry_id, field_mask, term_freq
                 FROM {post_table}",
                post_table = state.temp_posting_table
            );
            diesel::sql_query(&copy_postings).execute(tx)?;

            // 4. Copy temp documents into active table.
            let copy_docs = format!(
                "INSERT INTO search_document
                 SELECT profile_id, entry_id, event_id, active_time_ms, captured_at_ms,
                        file_type, file_extensions, mime_type, indexed_at_ms,
                        index_version, text_preview
                 FROM {doc_table}",
                doc_table = state.temp_document_table
            );
            diesel::sql_query(&copy_docs).execute(tx)?;

            // 5. Update meta: unblock and record version + completion timestamp.
            use crate::db::schema::search_index_meta::dsl;
            diesel::update(dsl::search_index_meta.filter(dsl::profile_id.eq(profile_id)))
                .set((
                    dsl::index_version.eq(target_version),
                    dsl::search_blocked.eq(false),
                    dsl::last_rebuild_completed_at_ms.eq(completed_at_ms),
                ))
                .execute(tx)?;

            Ok(())
        })
        .map_err(|e| SearchError::Internal(format!("finalize_rebuild transaction failed: {e}")))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// SearchIndexPort implementation
// ──────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl SearchIndexPort for SqliteSearchIndex {
    #[instrument(
        name = "search_index.index_entry",
        level = "debug",
        skip(self, document, postings),
        fields(entry_id = %document.entry_id, posting_count = postings.len())
    )]
    async fn index_entry(
        &self,
        document: SearchDocument,
        postings: Vec<SearchPosting>,
    ) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();

        // Check for active rebuild before entering spawn_blocking. A clone is cheap;
        // TOCTOU is acceptable here — if rebuild finishes between check and temp write,
        // the temp table will be gone, and the sql_query will return an error that we
        // swallow as best-effort (per the plan: new entries survive cutover via the
        // active-table write, which always happens first).
        let maybe_rebuild = self.active_rebuild_for_profile(&profile_id).await;

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;

            // 1. Always write to the active tables first.
            Self::upsert_active_entry(&mut conn, &profile_id, &document, &postings)?;

            // 2. If a rebuild is active for this profile, mirror into temp tables.
            if let Some(rebuild_state) = maybe_rebuild {
                // Best-effort: if temp table was already dropped (rebuild completed
                // between our check and this write), log and continue.
                if let Err(e) =
                    Self::insert_temp_entry(&mut conn, &rebuild_state, &document, &postings)
                {
                    warn!(error = %e, "failed to mirror index_entry into rebuild temp tables (best-effort)");
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    #[instrument(name = "search_index.remove_entry", level = "debug", skip(self), fields(entry_id = %entry_id))]
    async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();
        let entry_id = entry_id.clone();

        let maybe_rebuild = self.active_rebuild_for_profile(&profile_id).await;

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;

            // 1. Always delete from active tables first.
            Self::delete_active_entry(&mut conn, &profile_id, &entry_id)?;

            // 2. If a rebuild is active, mirror the delete into temp tables.
            if let Some(rebuild_state) = maybe_rebuild {
                if let Err(e) = Self::delete_temp_entry(&mut conn, &rebuild_state, &entry_id) {
                    warn!(error = %e, "failed to mirror remove_entry into rebuild temp tables (best-effort)");
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    #[instrument(
        name = "search_index.search",
        level = "debug",
        skip(self, query),
        fields(operator = ?query.operator, limit = query.limit, offset = query.offset)
    )]
    async fn search(&self, query: SearchQuery) -> Result<SearchResultsPage, SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();

        // Normalize query terms before entering spawn_blocking.
        let terms = Self::normalize_query_terms(&query)?;
        let is_filter_only = terms.is_empty();

        // Derive search key and compute HMAC tags only when there are terms.
        let term_tags: Vec<Vec<u8>> = if !is_filter_only {
            let search_key = self.search_key_derivation.derive_search_key().await?;
            terms
                .iter()
                .map(|t| term_tag(&search_key, t))
                .collect::<Result<_, _>>()
                .map_err(|e| SearchError::Internal(format!("term_tag computation failed: {e}")))?
        } else {
            vec![]
        };

        let operator = query.operator.clone();
        let time_range = query.time_range.clone();
        let content_types = query.content_types.clone();
        let extensions = query
            .extensions
            .iter()
            .map(|e| e.to_lowercase())
            .collect::<Vec<_>>();
        let source_devices = query
            .source_devices
            .iter()
            .map(|d| d.as_str().to_string())
            .collect::<Vec<_>>();
        let limit = query.limit as usize;
        let offset = query.offset as usize;

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            // 1. Ensure/load meta.
            Self::ensure_meta_row(&mut conn, &profile_id)?;
            let meta = Self::load_meta(&mut conn, &profile_id)?;

            // 2. Blocked guard.
            if meta.search_blocked {
                return Err(SearchError::IndexNotReady);
            }

            // 3. Version mismatch guard.
            if meta.index_version != CURRENT_INDEX_VERSION {
                warn!(
                    profile_id = %profile_id,
                    stored_version = %meta.index_version,
                    current_version = CURRENT_INDEX_VERSION,
                    "index version mismatch — blocking search"
                );
                Self::mark_blocked(&mut conn, &profile_id)?;
                return Err(SearchError::IndexNotReady);
            }

            // 4. Load candidate documents.
            // Filter-only path: load all documents (no term matching).
            // Term path: resolve postings first, then load matching documents.
            let (docs, hit_map): (Vec<SearchDocumentRow>, HashMap<String, u32>) = if is_filter_only
            {
                debug!("filter-only search — loading all documents");
                let all_docs = Self::load_all_documents(&mut conn, &profile_id)?;
                (all_docs, HashMap::new())
            } else {
                let hits =
                    Self::query_candidate_hits(&mut conn, &profile_id, &term_tags, &operator)?;

                if hits.is_empty() {
                    debug!("search produced no candidate hits");
                    return Ok(SearchResultsPage {
                        items: vec![],
                        total: 0,
                        has_more: false,
                    });
                }

                let candidate_ids: Vec<String> = hits.keys().cloned().collect();
                let candidate_docs =
                    Self::load_candidate_documents(&mut conn, &profile_id, &candidate_ids)?;
                (candidate_docs, hits)
            };

            // 5. Apply filters: source device, time range, file type, extension.
            // Resolve the allowed event_id set up front (None = no source
            // restriction) so source filtering runs before pagination and the
            // authoritative total count, keeping both correct.
            let source_event_ids: Option<HashSet<String>> = if source_devices.is_empty() {
                None
            } else {
                Some(Self::load_event_ids_for_sources(
                    &mut conn,
                    &source_devices,
                )?)
            };
            let now_ms = chrono::Utc::now().timestamp_millis();

            let filtered: Vec<(SearchDocumentRow, u32)> = docs
                .into_iter()
                .filter_map(|doc| {
                    // Source-device filter.
                    if let Some(ref allowed) = source_event_ids {
                        if !allowed.contains(&doc.event_id) {
                            return None;
                        }
                    }

                    // Time range filter.
                    if let Some(ref tr) = time_range {
                        let (from_ms, to_ms) = resolve_time_range(tr, now_ms);
                        if doc.active_time_ms < from_ms as i64 || doc.active_time_ms > to_ms as i64
                        {
                            return None;
                        }
                    }

                    // File type filter.
                    if !content_types.is_empty() {
                        let stored = &doc.file_type;
                        let matches = content_types.iter().any(|ft| {
                            let ft_str = serde_json::to_string(ft)
                                .unwrap_or_default()
                                .trim_matches('"')
                                .to_string();
                            ft_str == *stored
                        });
                        if !matches {
                            return None;
                        }
                    }

                    // Extension filter (case-insensitive).
                    if !extensions.is_empty() {
                        let doc_exts: Vec<String> =
                            serde_json::from_str::<Vec<String>>(&doc.file_extensions)
                                .unwrap_or_default()
                                .into_iter()
                                .map(|e| e.to_lowercase())
                                .collect();

                        let matches = extensions.iter().any(|ext| doc_exts.contains(ext));
                        if !matches {
                            return None;
                        }
                    }

                    let hit_count = *hit_map.get(&doc.entry_id).unwrap_or(&0);
                    Some((doc, hit_count))
                })
                .collect();

            // 7. Sort: active_time_ms DESC, hit_count DESC, captured_at_ms DESC.
            let mut sorted = filtered;
            sorted.sort_by(|(a, a_hits), (b, b_hits)| {
                b.active_time_ms
                    .cmp(&a.active_time_ms)
                    .then(b_hits.cmp(a_hits))
                    .then(b.captured_at_ms.cmp(&a.captured_at_ms))
            });

            // 8. Compute total before pagination — authoritative count for all matches.
            let total = sorted.len() as u32;

            // 9. Pagination.
            let paginated: Vec<(SearchDocumentRow, u32)> =
                sorted.into_iter().skip(offset).take(limit).collect();

            // 10. has_more: true when remaining entries exist after the current page.
            let has_more = total > (offset as u32) + (paginated.len() as u32);

            // 11. Map to SearchResult.
            let items: Vec<SearchResult> = paginated
                .into_iter()
                .filter_map(|(doc, _)| {
                    let domain = doc.to_domain().ok()?;
                    Some(SearchResult {
                        entry_id: domain.entry_id,
                        content_type: domain.content_type,
                        active_time_ms: domain.active_time_ms,
                        text_preview: domain.text_preview,
                        mime_type: domain.mime_type,
                        file_extensions: domain.file_extensions,
                    })
                })
                .collect();

            debug!(
                candidates = hit_map.len(),
                total,
                returned = items.len(),
                has_more,
                "search completed"
            );

            Ok(SearchResultsPage {
                items,
                total,
                has_more,
            })
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    /// Full index rebuild using temp-table workspace.
    ///
    /// Sequence:
    /// 1. Resolve profile_id, ensure meta row, set search_blocked = true.
    /// 2. Emit `RebuildStage::Started`.
    /// 3. Create temp tables and store `ActiveRebuild` in `rebuild_state`.
    /// 4. Write all entries into temp tables, emitting `RebuildStage::Indexing` every 100.
    /// 5. Call `finalize_rebuild()` in one transaction, then emit `RebuildStage::Complete`.
    /// 6. Drop temp tables, clear `rebuild_state`.
    ///
    /// On any error: emit `RebuildStage::Failed`, clear state, drop tables best-effort,
    /// leave `search_blocked = true`, return `SearchError::Internal`.
    #[instrument(
        name = "search_index.rebuild",
        level = "info",
        skip(self, entries, progress_tx),
        fields(entry_count = entries.len())
    )]
    async fn rebuild(
        &self,
        entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
        progress_tx: Sender<RebuildProgress>,
    ) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();
        let rebuild_state_arc = self.rebuild_state.clone();
        let total = entries.len() as u32;

        // ─── Step 1: set blocked and record start time ────────────────────────
        {
            let pid = profile_id.clone();
            let p = pool.clone();
            let now_ms = chrono::Utc::now().timestamp_millis();
            tokio::task::spawn_blocking(move || {
                let mut conn = p
                    .get()
                    .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;
                Self::ensure_meta_row(&mut conn, &pid)?;
                use crate::db::schema::search_index_meta::dsl;
                diesel::update(dsl::search_index_meta.filter(dsl::profile_id.eq(&pid)))
                    .set((
                        dsl::search_blocked.eq(true),
                        dsl::last_rebuild_started_at_ms.eq(now_ms),
                    ))
                    .execute(&mut conn)
                    .map_err(|e| SearchError::Internal(format!("set blocked failed: {e}")))?;
                Ok(())
            })
            .await
            .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))??;
        }

        // ─── Step 2: emit Started ─────────────────────────────────────────────
        let _ = progress_tx
            .send(RebuildProgress {
                stage: RebuildStage::Started,
                indexed: 0,
                total,
            })
            .await;

        // ─── Step 3: create temp tables and register active rebuild ───────────
        let rebuild_info = ActiveRebuild::new(&profile_id);
        {
            let rid = rebuild_info.clone();
            let p = pool.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                let mut conn = p
                    .get()
                    .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;
                Self::create_rebuild_tables(&mut conn, &rid)
            })
            .await
            .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))
            .and_then(|r| r)
            {
                let _ = progress_tx
                    .send(RebuildProgress {
                        stage: RebuildStage::Failed,
                        indexed: 0,
                        total,
                    })
                    .await;
                return Err(e);
            }
        }

        // Register active rebuild state so live mutations can mirror.
        {
            let mut guard = rebuild_state_arc.write().expect("rebuild_state poisoned");
            *guard = Some(rebuild_info.clone());
        }

        // ─── Step 4: batch-write entries into temp tables ─────────────────────
        let mut indexed: u32 = 0;

        // Test-only fault injection limit.
        #[cfg(test)]
        let fault_limit = self.fail_after_n_entries;

        for (document, postings) in &entries {
            let rid = rebuild_info.clone();
            let p = pool.clone();
            let doc = document.clone();
            let post = postings.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                let mut conn = p
                    .get()
                    .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;
                Self::insert_temp_entry(&mut conn, &rid, &doc, &post)
            })
            .await
            .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))
            .and_then(|r| r)
            {
                // Failure path: emit Failed, clear state, drop tables, leave blocked.
                {
                    let mut guard = rebuild_state_arc.write().expect("rebuild_state poisoned");
                    *guard = None;
                }
                let rid2 = rebuild_info.clone();
                let p2 = pool.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(mut conn) = p2.get() {
                        Self::drop_rebuild_tables(&mut conn, &rid2);
                    }
                })
                .await;
                let _ = progress_tx
                    .send(RebuildProgress {
                        stage: RebuildStage::Failed,
                        indexed,
                        total,
                    })
                    .await;
                return Err(e);
            }

            indexed += 1;

            // Test-only: trigger failure after N entries.
            #[cfg(test)]
            if let Some(limit) = fault_limit {
                if indexed as usize >= limit {
                    // Simulate a failure by injecting an Internal error.
                    {
                        let mut guard = rebuild_state_arc.write().expect("rebuild_state poisoned");
                        *guard = None;
                    }
                    let rid2 = rebuild_info.clone();
                    let p2 = pool.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(mut conn) = p2.get() {
                            Self::drop_rebuild_tables(&mut conn, &rid2);
                        }
                    })
                    .await;
                    let _ = progress_tx
                        .send(RebuildProgress {
                            stage: RebuildStage::Failed,
                            indexed,
                            total,
                        })
                        .await;
                    return Err(SearchError::Internal(
                        "fault injection: rebuild failed after N entries".to_string(),
                    ));
                }
            }

            // Emit Indexing progress after every 100 entries or at the final batch.
            if indexed % 100 == 0 || indexed == total {
                let _ = progress_tx
                    .send(RebuildProgress {
                        stage: RebuildStage::Indexing,
                        indexed,
                        total,
                    })
                    .await;
            }
        }

        // ─── Test-only pause: wait before finalize so tests can inject mutations ─
        #[cfg(test)]
        if let Some(sem) = &self.pause_before_finalize {
            // Acquire one permit — the test will add a permit when ready to proceed.
            let _ = sem.acquire().await;
        }

        // ─── Step 5: finalize rebuild ─────────────────────────────────────────
        let completed_at_ms = chrono::Utc::now().timestamp_millis();
        {
            let rid = rebuild_info.clone();
            let p = pool.clone();
            if let Err(e) = tokio::task::spawn_blocking(move || {
                let mut conn = p
                    .get()
                    .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;
                Self::finalize_rebuild(&mut conn, &rid, completed_at_ms)
            })
            .await
            .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))
            .and_then(|r| r)
            {
                // Finalize failed: clear state, drop tables, leave blocked.
                {
                    let mut guard = rebuild_state_arc.write().expect("rebuild_state poisoned");
                    *guard = None;
                }
                let rid2 = rebuild_info.clone();
                let p2 = pool.clone();
                let _ = tokio::task::spawn_blocking(move || {
                    if let Ok(mut conn) = p2.get() {
                        Self::drop_rebuild_tables(&mut conn, &rid2);
                    }
                })
                .await;
                let _ = progress_tx
                    .send(RebuildProgress {
                        stage: RebuildStage::Failed,
                        indexed,
                        total,
                    })
                    .await;
                return Err(e);
            }
        }

        // ─── Step 6: drop temp tables, clear state ────────────────────────────
        {
            let mut guard = rebuild_state_arc.write().expect("rebuild_state poisoned");
            *guard = None;
        }
        {
            let rid = rebuild_info.clone();
            let p = pool.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Ok(mut conn) = p.get() {
                    Self::drop_rebuild_tables(&mut conn, &rid);
                }
            })
            .await;
        }

        // ─── Emit Complete ────────────────────────────────────────────────────
        let _ = progress_tx
            .send(RebuildProgress {
                stage: RebuildStage::Complete,
                indexed,
                total,
            })
            .await;

        Ok(())
    }

    #[instrument(name = "search_index.get_index_meta", level = "debug", skip(self))]
    async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;
            Self::load_meta(&mut conn, &profile_id)
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Convert a `TimeRangeFilter` variant to an inclusive `(from_ms, to_ms)` pair.
///
/// Preset ranges are resolved relative to `now_ms` (UTC milliseconds).
fn resolve_time_range(filter: &TimeRangeFilter, now_ms: i64) -> (u64, u64) {
    const MS_PER_DAY: i64 = 86_400_000;

    // Snap to midnight of today in UTC.
    let today_start_ms = {
        let secs = now_ms / 1000;
        let day_secs = secs - (secs % 86_400);
        day_secs * 1000
    };

    match filter {
        TimeRangeFilter::Today => (today_start_ms as u64, now_ms as u64),
        TimeRangeFilter::Yesterday => {
            let start = today_start_ms - MS_PER_DAY;
            (start as u64, (today_start_ms - 1) as u64)
        }
        TimeRangeFilter::Last24h => {
            let start = now_ms - MS_PER_DAY;
            (start as u64, now_ms as u64)
        }
        TimeRangeFilter::Last7d => {
            let start = today_start_ms - 7 * MS_PER_DAY;
            (start as u64, now_ms as u64)
        }
        TimeRangeFilter::Last30d => {
            let start = today_start_ms - 30 * MS_PER_DAY;
            (start as u64, now_ms as u64)
        }
        TimeRangeFilter::ThisWeek => {
            // ISO: week starts Monday. Approximate using day-of-week from epoch.
            // Epoch (1970-01-01) was a Thursday. Days since Thursday = (days % 7).
            // Monday offset from Thursday = -3 mod 7 = 4.
            let days_since_epoch = today_start_ms / (MS_PER_DAY);
            let day_of_week = ((days_since_epoch + 4) % 7) as i64; // 0=Mon
            let start = today_start_ms - day_of_week * MS_PER_DAY;
            (start as u64, now_ms as u64)
        }
        TimeRangeFilter::ThisMonth => {
            // Approximate: 30 days from first of month is complex; use calendar.
            // Simpler: subtract days-in-current-month approximation.
            // For V1 correctness, use chrono to find the first of the month.
            let dt = chrono::DateTime::from_timestamp_millis(now_ms)
                .unwrap_or_else(|| chrono::DateTime::from_timestamp(0, 0).unwrap());
            use chrono::{Datelike, TimeZone, Utc};
            let first_of_month = Utc
                .with_ymd_and_hms(dt.year(), dt.month(), 1, 0, 0, 0)
                .single()
                .map(|d| d.timestamp_millis())
                .unwrap_or(today_start_ms);
            (first_of_month as u64, now_ms as u64)
        }
        TimeRangeFilter::Absolute { from_ms, to_ms } => (*from_ms, *to_ms),
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────
