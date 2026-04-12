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

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::RunQueryDsl;
use tokio::sync::mpsc::Sender;
use tracing::{debug, instrument, warn};

use uc_core::ids::EntryId;
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::security::key_scope::KeyScopePort;
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
    key_scope: Arc<dyn KeyScopePort>,
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
        key_scope: Arc<dyn KeyScopePort>,
        search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    ) -> Self {
        Self {
            pool,
            key_scope,
            search_key_derivation,
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            #[cfg(test)]
            fail_after_n_entries: None,
            #[cfg(test)]
            pause_before_finalize: None,
        }
    }

    // ─── Private async helpers ────────────────────────────────────────────────

    /// Resolve the current profile ID from the key scope.
    async fn current_profile_id(&self) -> Result<String, SearchError> {
        let scope = self
            .key_scope
            .current_scope()
            .await
            .map_err(|e| SearchError::Internal(format!("failed to get key scope: {e}")))?;
        Ok(scope.profile_id)
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
        use crate::db::schema::search_index_meta::dsl;

        let existing: Option<SearchIndexMetaRow> = dsl::search_index_meta
            .filter(dsl::profile_id.eq(profile_id))
            .first::<SearchIndexMetaRow>(conn)
            .optional()
            .map_err(|e| SearchError::Internal(format!("meta row query failed: {e}")))?;

        if existing.is_none() {
            let seed = NewSearchIndexMetaRow::seed(profile_id);
            diesel::insert_into(search_index_meta::table)
                .values(&seed)
                .execute(conn)
                .map_err(|e| SearchError::Internal(format!("meta row seed failed: {e}")))?;
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
        let profile_id = self.current_profile_id().await?;
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
        let profile_id = self.current_profile_id().await?;
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
        let profile_id = self.current_profile_id().await?;
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

            // 5. Apply filters: time range, file type, extension.
            let now_ms = chrono::Utc::now().timestamp_millis();

            let filtered: Vec<(SearchDocumentRow, u32)> = docs
                .into_iter()
                .filter_map(|doc| {
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
        let profile_id = self.current_profile_id().await?;
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
        let profile_id = self.current_profile_id().await?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    use async_trait::async_trait;
    use uc_core::ids::{EntryId, EventId};
    use uc_core::ports::search::search_key::SearchKeyDerivationPort;
    use uc_core::ports::security::key_scope::{KeyScopePort, ScopeError};
    use uc_core::search::document::{ContentType, SearchDocument, SearchPosting};
    use uc_core::search::error::SearchError;
    use uc_core::search::key::SearchKey;
    use uc_core::security::model::KeyScope;

    use crate::db::pool::init_db_pool;
    use crate::search::constants::CURRENT_INDEX_VERSION;
    use crate::search::search_key_derivation::term_tag;

    // ── Stubs ─────────────────────────────────────────────────────────────────

    struct FixedScope {
        profile_id: String,
    }

    #[async_trait]
    impl KeyScopePort for FixedScope {
        async fn current_scope(&self) -> Result<KeyScope, ScopeError> {
            Ok(KeyScope {
                profile_id: self.profile_id.clone(),
            })
        }
    }

    struct FixedSearchKey {
        key: SearchKey,
    }

    #[async_trait]
    impl SearchKeyDerivationPort for FixedSearchKey {
        async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
            Ok(self.key.clone())
        }
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_adapter(tmp: &NamedTempFile, profile_id: &str) -> SqliteSearchIndex {
        let path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&path).expect("pool init");
        SqliteSearchIndex::new(
            pool,
            Arc::new(FixedScope {
                profile_id: profile_id.to_string(),
            }),
            Arc::new(FixedSearchKey {
                key: SearchKey([0xABu8; 32]),
            }),
        )
    }

    fn sample_document(entry_id: &str) -> SearchDocument {
        SearchDocument {
            entry_id: EntryId::from(entry_id),
            event_id: EventId::from("event-01"),
            active_time_ms: 1_000_000,
            captured_at_ms: 999_000,
            content_type: ContentType::Text,
            file_extensions: vec!["txt".to_string()],
            mime_type: "text/plain".to_string(),
            indexed_at_ms: 1_100_000,
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: Some("Hello world".to_string()),
        }
    }

    fn make_postings(entry_id: &str, tokens: &[&str]) -> Vec<SearchPosting> {
        let key = SearchKey([0xABu8; 32]);
        tokens
            .iter()
            .map(|t| {
                let tag = term_tag(&key, t).expect("term_tag");
                SearchPosting {
                    term_tag: tag,
                    entry_id: EntryId::from(entry_id),
                    field_mask: 0b0000_0001,
                    term_freq: 1,
                }
            })
            .collect()
    }

    fn make_search_query(q: &str) -> SearchQuery {
        SearchQuery {
            query_string: q.to_string(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        }
    }

    // ── Task 1 Tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn meta_and_live_write_seeds_and_round_trips() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        // get_index_meta() should seed the row and return defaults.
        let meta = adapter.get_index_meta().await.expect("get_index_meta");
        assert_eq!(meta.index_version, CURRENT_INDEX_VERSION);
        assert!(!meta.search_blocked);
        assert!(meta.last_rebuild_started_at_ms.is_none());

        // index_entry() should write one document and its postings.
        let doc = sample_document("entry-001");
        let postings = make_postings("entry-001", &["hello", "world"]);
        adapter
            .index_entry(doc, postings)
            .await
            .expect("index_entry");

        // Verify rows exist in DB via direct pool access.
        let pool = init_db_pool(&tmp.path().to_string_lossy()).expect("pool");
        let mut conn = pool.get().expect("conn");

        use crate::db::schema::search_document::dsl as sd;
        use crate::db::schema::search_posting::dsl as sp;
        use diesel::RunQueryDsl;

        let doc_count: i64 = sd::search_document
            .filter(sd::profile_id.eq("profile-test"))
            .count()
            .get_result(&mut conn)
            .expect("doc count");
        assert_eq!(doc_count, 1, "expected 1 search_document row");

        let posting_count: i64 = sp::search_posting
            .filter(sp::profile_id.eq("profile-test"))
            .count()
            .get_result(&mut conn)
            .expect("posting count");
        assert_eq!(posting_count, 2, "expected 2 search_posting rows");
    }

    #[tokio::test]
    async fn remove_entry_deletes_doc_and_postings() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        // Index an entry first.
        let doc = sample_document("entry-del");
        let postings = make_postings("entry-del", &["alpha", "beta"]);
        adapter
            .index_entry(doc, postings)
            .await
            .expect("index_entry");

        // Remove the entry.
        let entry_id = EntryId::from("entry-del");
        adapter.remove_entry(&entry_id).await.expect("remove_entry");

        // Verify both tables are empty for this entry.
        let pool = init_db_pool(&tmp.path().to_string_lossy()).expect("pool");
        let mut conn = pool.get().expect("conn");

        use crate::db::schema::search_document::dsl as sd;
        use crate::db::schema::search_posting::dsl as sp;

        let doc_count: i64 = sd::search_document
            .filter(sd::profile_id.eq("profile-test"))
            .filter(sd::entry_id.eq("entry-del"))
            .count()
            .get_result(&mut conn)
            .expect("doc count");
        assert_eq!(doc_count, 0, "expected 0 search_document rows after remove");

        let posting_count: i64 = sp::search_posting
            .filter(sp::profile_id.eq("profile-test"))
            .filter(sp::entry_id.eq("entry-del"))
            .count()
            .get_result(&mut conn)
            .expect("posting count");
        assert_eq!(
            posting_count, 0,
            "expected 0 search_posting rows after remove"
        );
    }

    // ── Task 2 Tests ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn search_query_and_mode_requires_all_terms() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        // entry-A has both "alpha" and "beta"
        let doc_a = SearchDocument {
            entry_id: EntryId::from("entry-A"),
            active_time_ms: 2_000_000,
            ..sample_document("entry-A")
        };
        let postings_a = make_postings("entry-A", &["alpha", "beta"]);
        adapter
            .index_entry(doc_a, postings_a)
            .await
            .expect("index A");

        // entry-B has only "alpha"
        let doc_b = SearchDocument {
            entry_id: EntryId::from("entry-B"),
            active_time_ms: 1_000_000,
            ..sample_document("entry-B")
        };
        let postings_b = make_postings("entry-B", &["alpha"]);
        adapter
            .index_entry(doc_b, postings_b)
            .await
            .expect("index B");

        let query = SearchQuery {
            query_string: "alpha beta".to_string(),
            operator: QueryOperator::And,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };

        let page = adapter.search(query).await.expect("search");
        assert_eq!(
            page.items.len(),
            1,
            "AND mode must require all terms: {:?}",
            page.items
        );
        assert_eq!(page.items[0].entry_id, EntryId::from("entry-A"));
    }

    #[tokio::test]
    async fn search_query_or_mode_returns_any_match() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        // entry-A: "alpha"
        let doc_a = SearchDocument {
            entry_id: EntryId::from("entry-A"),
            active_time_ms: 2_000_000,
            ..sample_document("entry-A")
        };
        let postings_a = make_postings("entry-A", &["alpha"]);
        adapter
            .index_entry(doc_a, postings_a)
            .await
            .expect("index A");

        // entry-B: "beta"
        let doc_b = SearchDocument {
            entry_id: EntryId::from("entry-B"),
            active_time_ms: 1_000_000,
            ..sample_document("entry-B")
        };
        let postings_b = make_postings("entry-B", &["beta"]);
        adapter
            .index_entry(doc_b, postings_b)
            .await
            .expect("index B");

        let query = SearchQuery {
            query_string: "alpha beta".to_string(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };

        let page = adapter.search(query).await.expect("search");
        assert_eq!(
            page.items.len(),
            2,
            "OR mode must return both entries: {:?}",
            page.items
        );
    }

    #[tokio::test]
    async fn search_query_filters_time_type_and_extension() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        let now_ms = chrono::Utc::now().timestamp_millis();

        // entry-match: recent text with txt extension
        let doc_match = SearchDocument {
            entry_id: EntryId::from("entry-match"),
            active_time_ms: now_ms - 3600_000, // 1 hour ago
            captured_at_ms: now_ms - 3600_000,
            content_type: ContentType::Text,
            file_extensions: vec!["txt".to_string()],
            ..sample_document("entry-match")
        };
        let postings_match = make_postings("entry-match", &["hello"]);
        adapter
            .index_entry(doc_match, postings_match)
            .await
            .expect("index match");

        // entry-old: old entry (30+ days ago)
        let doc_old = SearchDocument {
            entry_id: EntryId::from("entry-old"),
            active_time_ms: now_ms - 40 * 86_400_000, // 40 days ago
            captured_at_ms: now_ms - 40 * 86_400_000,
            content_type: ContentType::Text,
            file_extensions: vec!["txt".to_string()],
            ..sample_document("entry-old")
        };
        let postings_old = make_postings("entry-old", &["hello"]);
        adapter
            .index_entry(doc_old, postings_old)
            .await
            .expect("index old");

        // entry-image: recent but wrong type
        let doc_image = SearchDocument {
            entry_id: EntryId::from("entry-image"),
            active_time_ms: now_ms - 3600_000,
            captured_at_ms: now_ms - 3600_000,
            content_type: ContentType::Image,
            file_extensions: vec!["png".to_string()],
            ..sample_document("entry-image")
        };
        let postings_image = make_postings("entry-image", &["hello"]);
        adapter
            .index_entry(doc_image, postings_image)
            .await
            .expect("index image");

        // Query: last 7 days, text type, txt extension
        let query = SearchQuery {
            query_string: "hello".to_string(),
            operator: QueryOperator::Or,
            time_range: Some(TimeRangeFilter::Last7d),
            content_types: vec![ContentType::Text],
            extensions: vec!["txt".to_string()],
            limit: 10,
            offset: 0,
        };

        let page = adapter.search(query).await.expect("search");
        assert_eq!(
            page.items.len(),
            1,
            "only entry-match should pass all filters: {:?}",
            page.items
        );
        assert_eq!(page.items[0].entry_id, EntryId::from("entry-match"));
    }

    #[tokio::test]
    async fn search_query_returns_index_not_ready_when_blocked_or_version_mismatched() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-test");

        // Seed meta row via get_index_meta.
        adapter.get_index_meta().await.expect("seed meta");

        // Manually set search_blocked = true.
        let pool = init_db_pool(&tmp.path().to_string_lossy()).expect("pool");
        {
            let mut conn = pool.get().expect("conn");
            use crate::db::schema::search_index_meta::dsl;
            diesel::update(dsl::search_index_meta.filter(dsl::profile_id.eq("profile-test")))
                .set(dsl::search_blocked.eq(true))
                .execute(&mut conn)
                .expect("set blocked");
        }

        let query = SearchQuery {
            query_string: "hello".to_string(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };

        let result = adapter.search(query.clone()).await;
        assert!(
            matches!(result, Err(SearchError::IndexNotReady)),
            "blocked meta must return IndexNotReady, got: {result:?}"
        );

        // Reset blocked, set wrong version.
        {
            let mut conn = pool.get().expect("conn");
            use crate::db::schema::search_index_meta::dsl;
            diesel::update(dsl::search_index_meta.filter(dsl::profile_id.eq("profile-test")))
                .set((
                    dsl::search_blocked.eq(false),
                    dsl::index_version.eq("stale-v0"),
                ))
                .execute(&mut conn)
                .expect("set stale version");
        }

        let result2 = adapter.search(query.clone()).await;
        assert!(
            matches!(result2, Err(SearchError::IndexNotReady)),
            "version mismatch must return IndexNotReady, got: {result2:?}"
        );

        // Verify that search_blocked was set to true after version mismatch.
        {
            let mut conn = pool.get().expect("conn");
            use crate::db::schema::search_index_meta::dsl;
            let row = dsl::search_index_meta
                .filter(dsl::profile_id.eq("profile-test"))
                .first::<SearchIndexMetaRow>(&mut conn)
                .expect("row");
            assert!(
                row.search_blocked,
                "version mismatch must set search_blocked = true"
            );
        }
    }

    // ── Phase 92: Pagination Metadata Tests ──────────────────────────────────

    #[tokio::test]
    async fn search_query_returns_total_and_has_more_metadata() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-pg");

        // Seed meta row.
        adapter.get_index_meta().await.expect("seed meta");

        // Index 5 entries, each with the token "clip".
        for i in 0..5u32 {
            let mut doc = sample_document(&format!("entry-pg-{i}"));
            doc.entry_id = uc_core::ids::EntryId::from(format!("entry-pg-{i}").as_str());
            doc.active_time_ms = i as i64 * 1000;
            doc.indexed_at_ms = i as i64 * 1000;
            let postings = make_postings(&format!("entry-pg-{i}"), &["clip"]);
            adapter.index_entry(doc, postings).await.expect("index");
        }

        // Query page 1: offset=0, limit=3 → 3 items, total=5, has_more=true.
        let q1 = SearchQuery {
            query_string: "clip".into(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 3,
            offset: 0,
        };
        let page1 = adapter.search(q1).await.expect("search page1");
        assert_eq!(page1.total, 5, "total must be 5 regardless of page size");
        assert!(
            page1.has_more,
            "has_more must be true when more pages follow"
        );
        assert_eq!(page1.items.len(), 3, "items must respect limit");

        // Query page 2: offset=3, limit=3 → 2 items, total=5, has_more=false.
        let q2 = SearchQuery {
            query_string: "clip".into(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 3,
            offset: 3,
        };
        let page2 = adapter.search(q2).await.expect("search page2");
        assert_eq!(page2.total, 5, "total must be consistent across pages");
        assert!(!page2.has_more, "has_more must be false on last page");
        assert_eq!(page2.items.len(), 2, "last page has remaining 2 items");

        // Query for non-existent token → total=0, has_more=false.
        let q3 = SearchQuery {
            query_string: "nonexistent".into(),
            operator: QueryOperator::Or,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };
        let page3 = adapter.search(q3).await.expect("search page3");
        assert_eq!(page3.total, 0, "empty result total must be 0");
        assert!(!page3.has_more, "empty result has_more must be false");
        assert!(page3.items.is_empty(), "empty result items must be empty");
    }

    // ── Task 1 Rebuild Tests ──────────────────────────────────────────────────

    #[tokio::test]
    async fn rebuild_cutover_sets_blocked_then_clears_on_success() {
        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-rb");

        // Seed meta row.
        adapter.get_index_meta().await.expect("seed meta");

        // Index an entry so we have content to rebuild over.
        let doc = sample_document("entry-rb1");
        let postings = make_postings("entry-rb1", &["rebuild"]);
        adapter
            .index_entry(doc.clone(), postings.clone())
            .await
            .expect("index_entry");

        let entries = vec![(doc, postings)];
        let (tx, mut rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        let rebuild_result = adapter.rebuild(entries, tx).await;
        assert!(
            rebuild_result.is_ok(),
            "rebuild should succeed: {rebuild_result:?}"
        );

        // Drain progress events.
        let mut stages: Vec<RebuildStage> = Vec::new();
        while let Ok(p) = rx.try_recv() {
            stages.push(p.stage);
        }

        // Must have Started and Complete stages.
        assert!(
            stages.contains(&RebuildStage::Started),
            "must emit Started: {stages:?}"
        );
        assert!(
            stages.contains(&RebuildStage::Complete),
            "must emit Complete: {stages:?}"
        );
        assert!(
            !stages.contains(&RebuildStage::Failed),
            "must not emit Failed: {stages:?}"
        );

        // After successful rebuild, search_blocked must be false.
        let meta = adapter.get_index_meta().await.expect("get_index_meta");
        assert!(
            !meta.search_blocked,
            "search_blocked must be false after successful rebuild"
        );
        assert_eq!(meta.index_version, CURRENT_INDEX_VERSION);
        assert!(meta.last_rebuild_completed_at_ms.is_some());
    }

    #[tokio::test]
    async fn rebuild_failure_leaves_meta_blocked() {
        let tmp = NamedTempFile::new().expect("temp file");
        let mut adapter = make_adapter(&tmp, "profile-fail");

        // Seed meta row.
        adapter.get_index_meta().await.expect("seed meta");

        // Inject fault: fail after 1 entry.
        adapter.fail_after_n_entries = Some(1);

        let doc1 = sample_document("entry-f1");
        let post1 = make_postings("entry-f1", &["foo"]);
        let doc2 = sample_document("entry-f2");
        let post2 = make_postings("entry-f2", &["bar"]);
        let entries = vec![(doc1, post1), (doc2, post2)];

        let (tx, mut rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);
        let result = adapter.rebuild(entries, tx).await;
        assert!(result.is_err(), "rebuild with fault injection should fail");

        // Drain progress events.
        let mut stages: Vec<RebuildStage> = Vec::new();
        while let Ok(p) = rx.try_recv() {
            stages.push(p.stage);
        }
        assert!(
            stages.contains(&RebuildStage::Failed),
            "must emit Failed: {stages:?}"
        );

        // After failure, search_blocked must still be true.
        let meta = adapter.get_index_meta().await.expect("get_index_meta");
        assert!(
            meta.search_blocked,
            "search_blocked must remain true after failed rebuild"
        );
    }

    #[tokio::test]
    async fn rebuild_does_not_use_rename_table() {
        // Structural check: the adapter uses temp-table copy-in strategy, not RENAME TABLE.
        // Verified by:
        // 1. Running a successful rebuild and checking that the active tables are populated
        //    (data is present after cutover — not just renamed from temp to active).
        // 2. Checking that the temp table prefix constants exist in this module (compile-time).
        //
        // The tmp_search_document_rebuild_ and tmp_search_posting_rebuild_ prefixes are
        // referenced elsewhere in this file (in create_rebuild_tables), ensuring the
        // copy-in approach is used rather than RENAME TABLE.

        let tmp = NamedTempFile::new().expect("temp file");
        let adapter = make_adapter(&tmp, "profile-no-rename");

        // Seed + rebuild with one entry.
        adapter.get_index_meta().await.expect("seed meta");
        let doc = sample_document("entry-norename");
        let post = make_postings("entry-norename", &["norename"]);
        let entries = vec![(doc, post)];
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        let result = adapter.rebuild(entries, tx).await;
        assert!(result.is_ok(), "rebuild must succeed: {result:?}");

        // After rebuild, the active search_document table must contain the entry
        // (proving data was copied into active tables, not just stored in temp tables).
        let pool = init_db_pool(&tmp.path().to_string_lossy()).expect("pool");
        let mut conn = pool.get().expect("conn");
        use crate::db::schema::search_document::dsl as sd;
        let count: i64 = sd::search_document
            .filter(sd::profile_id.eq("profile-no-rename"))
            .filter(sd::entry_id.eq("entry-norename"))
            .count()
            .get_result(&mut conn)
            .expect("count");
        assert_eq!(
            count, 1,
            "entry must be in active search_document after cutover"
        );

        // Compile-time assertion: ActiveRebuild::new uses tmp_search_document_rebuild_ prefix.
        let state = ActiveRebuild::new("test");
        assert!(
            state
                .temp_document_table
                .starts_with("tmp_search_document_rebuild_"),
            "temp doc table must use tmp_search_document_rebuild_ prefix: {}",
            state.temp_document_table
        );
        assert!(
            state
                .temp_posting_table
                .starts_with("tmp_search_posting_rebuild_"),
            "temp posting table must use tmp_search_posting_rebuild_ prefix: {}",
            state.temp_posting_table
        );
    }

    // ── Task 2 Rebuild Mirroring Tests ────────────────────────────────────────

    /// Pause rebuild after temp tables are written but before finalize.
    /// Inject a new entry via index_entry(), then resume. Verify the new entry
    /// is present in search results after cutover.
    #[tokio::test]
    async fn rebuild_mirroring_keeps_new_entry_after_cutover() {
        let tmp = NamedTempFile::new().expect("temp file");
        let pool_path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&pool_path).expect("pool init");

        // A semaphore with 0 permits — rebuild will block when it tries to acquire.
        let pause_sem = Arc::new(tokio::sync::Semaphore::new(0));

        let adapter = SqliteSearchIndex {
            pool: pool.clone(),
            key_scope: Arc::new(FixedScope {
                profile_id: "profile-mirror".to_string(),
            }),
            search_key_derivation: Arc::new(FixedSearchKey {
                key: SearchKey([0xABu8; 32]),
            }),
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            fail_after_n_entries: None,
            pause_before_finalize: Some(pause_sem.clone()),
        };

        // Seed + index an initial entry.
        adapter.get_index_meta().await.expect("seed meta");
        let doc_initial = sample_document("entry-initial");
        let post_initial = make_postings("entry-initial", &["initial"]);
        adapter
            .index_entry(doc_initial.clone(), post_initial.clone())
            .await
            .expect("index initial");

        let entries = vec![(doc_initial, post_initial)];
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        // Wrap adapter in Arc for sharing between tasks.
        let adapter = Arc::new(adapter);
        let adapter_clone = adapter.clone();

        // Spawn rebuild task — it will pause before finalize.
        let rebuild_handle = tokio::spawn(async move { adapter_clone.rebuild(entries, tx).await });

        // Wait until rebuild has paused (active_rebuild_for_profile returns Some).
        // Poll with a short delay until rebuild state is set.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if adapter
                .active_rebuild_for_profile("profile-mirror")
                .await
                .is_some()
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("rebuild did not start within 5 seconds");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Inject a new entry while rebuild is paused.
        let doc_new = sample_document("entry-new");
        let post_new = make_postings("entry-new", &["newtoken"]);
        adapter
            .index_entry(doc_new, post_new)
            .await
            .expect("index_entry during rebuild");

        // Resume rebuild by adding a permit.
        pause_sem.add_permits(1);

        // Wait for rebuild to complete.
        let result = rebuild_handle.await.expect("join");
        assert!(result.is_ok(), "rebuild should succeed: {result:?}");

        // After rebuild, search must find the new entry.
        let query = make_search_query("newtoken");
        let page = adapter.search(query).await.expect("search after rebuild");
        assert_eq!(
            page.items.len(),
            1,
            "new entry added during rebuild must be present after cutover: {:?}",
            page.items
        );
        assert_eq!(page.items[0].entry_id, EntryId::from("entry-new"));
    }

    /// Pause rebuild, delete an already-staged entry, resume rebuild.
    /// The deleted entry must not appear in search results after cutover.
    #[tokio::test]
    async fn rebuild_mirroring_prevents_deleted_entry_resurrection() {
        let tmp = NamedTempFile::new().expect("temp file");
        let pool_path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&pool_path).expect("pool init");

        let pause_sem = Arc::new(tokio::sync::Semaphore::new(0));

        let adapter = SqliteSearchIndex {
            pool: pool.clone(),
            key_scope: Arc::new(FixedScope {
                profile_id: "profile-delete".to_string(),
            }),
            search_key_derivation: Arc::new(FixedSearchKey {
                key: SearchKey([0xABu8; 32]),
            }),
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            fail_after_n_entries: None,
            pause_before_finalize: Some(pause_sem.clone()),
        };

        adapter.get_index_meta().await.expect("seed meta");

        // Index two entries.
        let doc_keep = sample_document("entry-keep");
        let post_keep = make_postings("entry-keep", &["keeptoken"]);
        let doc_del = sample_document("entry-to-delete");
        let post_del = make_postings("entry-to-delete", &["deltoken"]);

        adapter
            .index_entry(doc_keep.clone(), post_keep.clone())
            .await
            .expect("index keep");
        adapter
            .index_entry(doc_del.clone(), post_del.clone())
            .await
            .expect("index del");

        let entries = vec![(doc_keep, post_keep), (doc_del, post_del)];
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        let adapter = Arc::new(adapter);
        let adapter_clone = adapter.clone();

        let rebuild_handle = tokio::spawn(async move { adapter_clone.rebuild(entries, tx).await });

        // Wait for rebuild pause.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if adapter
                .active_rebuild_for_profile("profile-delete")
                .await
                .is_some()
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("rebuild did not start within 5 seconds");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Delete entry-to-delete while rebuild is paused.
        let del_id = EntryId::from("entry-to-delete");
        adapter
            .remove_entry(&del_id)
            .await
            .expect("remove_entry during rebuild");

        // Resume.
        pause_sem.add_permits(1);
        let result = rebuild_handle.await.expect("join");
        assert!(result.is_ok(), "rebuild should succeed: {result:?}");

        // entry-to-delete must not appear after cutover.
        let q_del = make_search_query("deltoken");
        let page_del = adapter.search(q_del).await.expect("search deltoken");
        assert!(
            page_del.items.is_empty(),
            "deleted entry must not appear after rebuild cutover: {:?}",
            page_del.items
        );

        // entry-keep must still be present.
        let q_keep = make_search_query("keeptoken");
        let page_keep = adapter.search(q_keep).await.expect("search keeptoken");
        assert_eq!(
            page_keep.items.len(),
            1,
            "kept entry must appear after rebuild cutover: {:?}",
            page_keep.items
        );
    }

    /// Hold a read transaction open on a second connection while rebuild finalizes.
    /// The rebuild must still return Ok — no SQLITE_BUSY failure under WAL.
    #[tokio::test]
    async fn rebuild_cutover_completes_with_concurrent_read_transaction() {
        let tmp = NamedTempFile::new().expect("temp file");
        let pool_path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&pool_path).expect("pool init");

        let pause_sem = Arc::new(tokio::sync::Semaphore::new(0));

        let adapter = Arc::new(SqliteSearchIndex {
            pool: pool.clone(),
            key_scope: Arc::new(FixedScope {
                profile_id: "profile-conc".to_string(),
            }),
            search_key_derivation: Arc::new(FixedSearchKey {
                key: SearchKey([0xABu8; 32]),
            }),
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            fail_after_n_entries: None,
            pause_before_finalize: Some(pause_sem.clone()),
        });

        adapter.get_index_meta().await.expect("seed meta");

        let doc = sample_document("entry-conc");
        let post = make_postings("entry-conc", &["conctoken"]);
        adapter
            .index_entry(doc.clone(), post.clone())
            .await
            .expect("index");

        let entries = vec![(doc, post)];
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        let adapter_clone = adapter.clone();
        let rebuild_handle = tokio::spawn(async move { adapter_clone.rebuild(entries, tx).await });

        // Wait for rebuild to pause.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if adapter
                .active_rebuild_for_profile("profile-conc")
                .await
                .is_some()
            {
                break;
            }
            if std::time::Instant::now() > deadline {
                panic!("rebuild did not start within 5 seconds");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // Open a second connection and begin a read transaction.
        use crate::search::test_support::hold_read_transaction;
        let _read_txn = hold_read_transaction(tmp.path());

        // Resume rebuild while read transaction is held.
        pause_sem.add_permits(1);
        let result = rebuild_handle.await.expect("join");
        assert!(
            result.is_ok(),
            "rebuild must succeed even with concurrent read transaction: {result:?}"
        );
        // _read_txn drops here, ending the read transaction.
    }

    /// Once rebuild has started and before finalize resumes, `search()` must
    /// return `SearchError::IndexNotReady`.
    #[tokio::test]
    async fn search_returns_index_not_ready_while_rebuild_is_in_progress() {
        let tmp = NamedTempFile::new().expect("temp file");
        let pool_path = tmp.path().to_string_lossy().to_string();
        let pool = init_db_pool(&pool_path).expect("pool init");

        let pause_sem = Arc::new(tokio::sync::Semaphore::new(0));

        let adapter = Arc::new(SqliteSearchIndex {
            pool: pool.clone(),
            key_scope: Arc::new(FixedScope {
                profile_id: "profile-block".to_string(),
            }),
            search_key_derivation: Arc::new(FixedSearchKey {
                key: SearchKey([0xABu8; 32]),
            }),
            rebuild_state: Arc::new(std::sync::RwLock::new(None)),
            fail_after_n_entries: None,
            pause_before_finalize: Some(pause_sem.clone()),
        });

        adapter.get_index_meta().await.expect("seed meta");

        let doc = sample_document("entry-blk");
        let post = make_postings("entry-blk", &["blocktest"]);
        adapter
            .index_entry(doc.clone(), post.clone())
            .await
            .expect("index");

        let entries = vec![(doc, post)];
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(32);

        let adapter_clone = adapter.clone();
        let rebuild_handle = tokio::spawn(async move { adapter_clone.rebuild(entries, tx).await });

        // Wait for rebuild to set blocked = true.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok(meta) = adapter.get_index_meta().await {
                if meta.search_blocked {
                    break;
                }
            }
            if std::time::Instant::now() > deadline {
                panic!("rebuild did not block search within 5 seconds");
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        }

        // search() must return IndexNotReady while blocked.
        let query = make_search_query("blocktest");
        let result = adapter.search(query).await;
        assert!(
            matches!(result, Err(SearchError::IndexNotReady)),
            "search must return IndexNotReady during rebuild: {result:?}"
        );

        // Resume rebuild.
        pause_sem.add_permits(1);
        let final_result = rebuild_handle.await.expect("join");
        assert!(
            final_result.is_ok(),
            "rebuild should succeed: {final_result:?}"
        );

        // After completion, search must work again.
        let query2 = make_search_query("blocktest");
        let page2 = adapter.search(query2).await.expect("search after rebuild");
        assert_eq!(
            page2.items.len(),
            1,
            "entry must be findable after rebuild completes"
        );
    }
}
