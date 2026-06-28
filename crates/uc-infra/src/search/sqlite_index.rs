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
use uc_core::search::tag::{SearchTagCount, TagId};

use crate::db::pool::DbPool;
use crate::db::schema::{search_document, search_entry_tag, search_index_meta, search_posting};
use crate::search::constants::CURRENT_INDEX_VERSION;
use crate::search::rows::{
    NewSearchDocumentRow, NewSearchEntryTagRow, NewSearchIndexMetaRow, NewSearchPostingRow,
    SearchDocumentRow, SearchIndexMetaRow,
};
use crate::search::search_key_derivation::term_tag;
use crate::search::tokenizer::SearchTokenizer;

/// Owned, query-derived filter inputs shared by both search paths.
///
/// Built once before `spawn_blocking`. `content_types` are pre-encoded to their
/// stored snake_case `file_type` strings; `extensions` are pre-lowercased.
struct FilterParams {
    content_types: Vec<String>,
    tags: Vec<String>,
    extensions: Vec<String>,
    source_devices: Vec<String>,
    time_range: Option<TimeRangeFilter>,
}

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
    pub temp_entry_tag_table: String,
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
            temp_entry_tag_table: format!("tmp_search_entry_tag_rebuild_{safe_suffix}"),
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

            // 4. Replace tag membership rows for this entry (mirror of document.tags).
            diesel::delete(
                search_entry_tag::table
                    .filter(search_entry_tag::profile_id.eq(profile_id))
                    .filter(search_entry_tag::entry_id.eq(&entry_id_str)),
            )
            .execute(tx)?;

            let tag_rows = NewSearchEntryTagRow::rows_for_document(profile_id, document);
            if !tag_rows.is_empty() {
                diesel::insert_into(search_entry_tag::table)
                    .values(&tag_rows)
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

            // Delete tag membership for this entry (hard-delete consistency).
            diesel::delete(
                search_entry_tag::table
                    .filter(search_entry_tag::profile_id.eq(profile_id))
                    .filter(search_entry_tag::entry_id.eq(&entry_id_str)),
            )
            .execute(tx)?;

            Ok(())
        })
        .map_err(|e| SearchError::Internal(format!("delete_active_entry failed: {e}")))
    }

    /// Add or remove only the favorited tag membership row for `entry_id` in the
    /// active table, leaving all rule-derived tags (e.g. `link`) untouched.
    /// Idempotent in both directions.
    fn set_active_favorite_tag(
        conn: &mut SqliteConnection,
        profile_id: &str,
        entry_id: &EntryId,
        favorited: bool,
    ) -> Result<(), SearchError> {
        let entry_id_str = entry_id.to_string();
        let favorited_tag = TagId::favorited().as_str().to_string();
        if favorited {
            diesel::insert_or_ignore_into(search_entry_tag::table)
                .values(NewSearchEntryTagRow {
                    profile_id: profile_id.to_string(),
                    entry_id: entry_id_str,
                    tag_id: favorited_tag,
                })
                .execute(conn)
                .map_err(|e| {
                    SearchError::Internal(format!("set_active_favorite_tag insert failed: {e}"))
                })?;
        } else {
            diesel::delete(
                search_entry_tag::table
                    .filter(search_entry_tag::profile_id.eq(profile_id))
                    .filter(search_entry_tag::entry_id.eq(&entry_id_str))
                    .filter(search_entry_tag::tag_id.eq(&favorited_tag)),
            )
            .execute(conn)
            .map_err(|e| {
                SearchError::Internal(format!("set_active_favorite_tag delete failed: {e}"))
            })?;
        }
        Ok(())
    }

    /// Mirror `set_active_favorite_tag` into the rebuild temp tag table, so a
    /// favorite toggle during a rebuild survives cutover. Only the favorited row
    /// is touched.
    fn set_temp_favorite_tag(
        conn: &mut SqliteConnection,
        state: &ActiveRebuild,
        entry_id: &EntryId,
        favorited: bool,
    ) -> Result<(), SearchError> {
        let profile_id = &state.profile_id;
        let entry_id_str = entry_id.to_string();
        let favorited_tag = TagId::favorited().as_str().to_string();
        if favorited {
            let insert_tag = format!(
                "INSERT OR REPLACE INTO {tag_table} (profile_id, entry_id, tag_id) VALUES (?, ?, ?)",
                tag_table = state.temp_entry_tag_table
            );
            diesel::sql_query(&insert_tag)
                .bind::<diesel::sql_types::Text, _>(profile_id)
                .bind::<diesel::sql_types::Text, _>(&entry_id_str)
                .bind::<diesel::sql_types::Text, _>(&favorited_tag)
                .execute(conn)
                .map_err(|e| {
                    SearchError::Internal(format!("set_temp_favorite_tag insert failed: {e}"))
                })?;
        } else {
            let del_tag = format!(
                "DELETE FROM {tag_table} WHERE profile_id = ? AND entry_id = ? AND tag_id = ?",
                tag_table = state.temp_entry_tag_table
            );
            diesel::sql_query(&del_tag)
                .bind::<diesel::sql_types::Text, _>(profile_id)
                .bind::<diesel::sql_types::Text, _>(&entry_id_str)
                .bind::<diesel::sql_types::Text, _>(&favorited_tag)
                .execute(conn)
                .map_err(|e| {
                    SearchError::Internal(format!("set_temp_favorite_tag delete failed: {e}"))
                })?;
        }
        Ok(())
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

    /// Resolve one page of the filter-only (no keyword) browse path entirely in
    /// SQL: filtering, ordering, and pagination are pushed down so the whole
    /// profile is never loaded into memory.
    ///
    /// Returns `(page rows, authoritative total, has_more)`.
    ///
    /// The same filter predicates feed both the `COUNT(*)` and the page query
    /// (via the `apply_filters!` macro) so the total can never drift from the
    /// rows. Ordering is `active_time_ms DESC`, served by
    /// `idx_search_document_profile_active_time` (leading column), letting SQLite
    /// stop after `offset + limit` rows without a temp-b-tree sort.
    fn filter_only_page(
        conn: &mut SqliteConnection,
        profile_id: &str,
        filters: &FilterParams,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SearchDocumentRow>, u32, bool), SearchError> {
        use diesel::dsl::{count_star, sql};
        use diesel::sql_types::{Bool, Text};
        use diesel::sqlite::Sqlite;

        let now_ms = chrono::Utc::now().timestamp_millis();
        let time_bounds: Option<(i64, i64)> = filters.time_range.as_ref().map(|tr| {
            let (from_ms, to_ms) = resolve_time_range(tr, now_ms);
            (from_ms as i64, to_ms as i64)
        });
        // Encode the (already lowercased) query extensions as one JSON array so
        // the predicate binds a single parameter and expands it with `json_each`.
        let ext_json: Option<String> = if filters.extensions.is_empty() {
            None
        } else {
            Some(
                serde_json::to_string(&filters.extensions)
                    .map_err(|e| SearchError::Internal(format!("extension encode failed: {e}")))?,
            )
        };

        // Single source of truth for the WHERE clause, applied to both the count
        // query and the page query so `total` and the rows can never diverge.
        macro_rules! apply_filters {
            ($q:expr) => {{
                let mut q = $q;
                if !filters.source_devices.is_empty() {
                    q = q.filter(
                        search_document::source_device.eq_any(filters.source_devices.clone()),
                    );
                }
                if !filters.tags.is_empty() {
                    // Tag membership (OR within the group) as an indexed subquery —
                    // served by `idx_entry_tag_by_tag (profile_id, tag_id)`.
                    let members = search_entry_tag::table
                        .filter(search_entry_tag::profile_id.eq(profile_id))
                        .filter(search_entry_tag::tag_id.eq_any(filters.tags.clone()))
                        .select(search_entry_tag::entry_id);
                    q = q.filter(search_document::entry_id.eq_any(members));
                }
                if let Some((from_ms, to_ms)) = time_bounds {
                    q = q.filter(search_document::active_time_ms.between(from_ms, to_ms));
                }
                if !filters.content_types.is_empty() {
                    q = q.filter(search_document::file_type.eq_any(filters.content_types.clone()));
                }
                if let Some(ref json) = ext_json {
                    // Case-insensitive membership between the document's extension
                    // array and the query's, both via `json_each`. `'[]'` (the
                    // default) yields no rows → no match, as intended.
                    q = q.filter(
                        sql::<Bool>(
                            "EXISTS (SELECT 1 \
                               FROM json_each(search_document.file_extensions) AS de \
                               JOIN json_each(",
                        )
                        .bind::<Text, _>(json.clone())
                        .sql(") AS qe ON lower(de.value) = qe.value)"),
                    );
                }
                q
            }};
        }

        // Authoritative total over the full filtered set.
        let total: i64 = apply_filters!(search_document::table
            .filter(search_document::profile_id.eq(profile_id))
            .select(count_star())
            .into_boxed::<Sqlite>())
        .first(conn)
        .map_err(|e| SearchError::Internal(format!("filter-only count failed: {e}")))?;

        // Page window — index-ordered, bounded by LIMIT/OFFSET.
        let page_rows: Vec<SearchDocumentRow> = apply_filters!(search_document::table
            .filter(search_document::profile_id.eq(profile_id))
            .select(SearchDocumentRow::as_select())
            .into_boxed::<Sqlite>())
        .order(search_document::active_time_ms.desc())
        .limit(limit as i64)
        .offset(offset as i64)
        .load(conn)
        .map_err(|e| SearchError::Internal(format!("filter-only page load failed: {e}")))?;

        let total = total as u32;
        let has_more = total > (offset as u32) + (page_rows.len() as u32);
        Ok((page_rows, total, has_more))
    }

    /// Resolve one page of the keyword path: rank the bounded posting-candidate
    /// set in memory (the candidate set is already restricted by term matches, so
    /// this never scans the whole profile). Filters are applied in memory and
    /// ordering keeps the hit-count tiebreak that the SQL path cannot express.
    ///
    /// Returns `(page rows, authoritative total, has_more)`.
    fn term_page(
        conn: &mut SqliteConnection,
        profile_id: &str,
        term_tags: &[Vec<u8>],
        operator: &QueryOperator,
        filters: &FilterParams,
        limit: usize,
        offset: usize,
    ) -> Result<(Vec<SearchDocumentRow>, u32, bool), SearchError> {
        let hits = Self::query_candidate_hits(conn, profile_id, term_tags, operator)?;
        if hits.is_empty() {
            return Ok((vec![], 0, false));
        }

        let candidate_ids: Vec<String> = hits.keys().cloned().collect();
        let docs = Self::load_candidate_documents(conn, profile_id, &candidate_ids)?;

        // Resolve the tag-membership restriction up front (None = unrestricted) so
        // filtering completes before pagination and the total stays authoritative.
        let tag_entry_ids: Option<HashSet<String>> = if filters.tags.is_empty() {
            None
        } else {
            Some(Self::load_entry_ids_for_tags(
                conn,
                profile_id,
                &filters.tags,
            )?)
        };
        let source_set: Option<HashSet<&str>> = if filters.source_devices.is_empty() {
            None
        } else {
            Some(filters.source_devices.iter().map(|s| s.as_str()).collect())
        };
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut filtered: Vec<(SearchDocumentRow, u32)> = docs
            .into_iter()
            .filter_map(|doc| {
                // Source-device filter — read the indexed `source_device` column
                // directly (no clipboard_event JOIN).
                if let Some(ref allowed) = source_set {
                    match doc.source_device.as_deref() {
                        Some(sd) if allowed.contains(sd) => {}
                        _ => return None,
                    }
                }

                // Tag filter (OR within the tag group).
                if let Some(ref allowed) = tag_entry_ids {
                    if !allowed.contains(&doc.entry_id) {
                        return None;
                    }
                }

                // Time range filter.
                if let Some(ref tr) = filters.time_range {
                    let (from_ms, to_ms) = resolve_time_range(tr, now_ms);
                    if doc.active_time_ms < from_ms as i64 || doc.active_time_ms > to_ms as i64 {
                        return None;
                    }
                }

                // File type filter (pre-encoded snake_case strings).
                if !filters.content_types.is_empty()
                    && !filters.content_types.iter().any(|ft| *ft == doc.file_type)
                {
                    return None;
                }

                // Extension filter (case-insensitive).
                if !filters.extensions.is_empty() {
                    let doc_exts: Vec<String> =
                        serde_json::from_str::<Vec<String>>(&doc.file_extensions)
                            .unwrap_or_else(|e| {
                                // A malformed stored row must not silently match
                                // nothing without a trace; the index data needs
                                // repair (a rebuild reserializes it).
                                tracing::warn!(
                                    entry_id = %doc.entry_id,
                                    error = %e,
                                    "search row has unparseable file_extensions; treating as empty"
                                );
                                Vec::new()
                            })
                            .into_iter()
                            .map(|e| e.to_lowercase())
                            .collect();
                    if !filters.extensions.iter().any(|ext| doc_exts.contains(ext)) {
                        return None;
                    }
                }

                let hit_count = *hits.get(&doc.entry_id).unwrap_or(&0);
                Some((doc, hit_count))
            })
            .collect();

        // Sort: active_time_ms DESC, hit_count DESC, captured_at_ms DESC.
        filtered.sort_by(|(a, a_hits), (b, b_hits)| {
            b.active_time_ms
                .cmp(&a.active_time_ms)
                .then(b_hits.cmp(a_hits))
                .then(b.captured_at_ms.cmp(&a.captured_at_ms))
        });

        let total = filtered.len() as u32;
        let page_rows: Vec<SearchDocumentRow> = filtered
            .into_iter()
            .skip(offset)
            .take(limit)
            .map(|(doc, _)| doc)
            .collect();
        let has_more = total > (offset as u32) + (page_rows.len() as u32);
        Ok((page_rows, total, has_more))
    }

    /// Hydrate the page's tag membership from `search_entry_tag` and map each
    /// row to a domain `SearchResult`. Shared tail of both search paths.
    ///
    /// Document rows carry an empty tag set; the membership is fetched here in one
    /// batched query scoped to the page window (at most `limit` entries).
    fn hydrate_results(
        conn: &mut SqliteConnection,
        profile_id: &str,
        page_rows: Vec<SearchDocumentRow>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let page_entry_ids: Vec<String> =
            page_rows.iter().map(|doc| doc.entry_id.clone()).collect();
        let tags_by_entry = Self::load_tags_for_entries(conn, profile_id, &page_entry_ids)?;

        let items = page_rows
            .into_iter()
            .map(|doc| {
                let tags = tags_by_entry
                    .get(&doc.entry_id)
                    .cloned()
                    .unwrap_or_default();
                // Propagate decode failures: silently dropping a row would make
                // `items` shorter than the already-counted `total`/`has_more`.
                let domain = doc.to_domain().map_err(|e| {
                    SearchError::Internal(format!(
                        "failed to decode search row {}: {e}",
                        doc.entry_id
                    ))
                })?;
                Ok(SearchResult {
                    entry_id: domain.entry_id,
                    content_type: domain.content_type,
                    active_time_ms: domain.active_time_ms,
                    tags,
                    text_preview: domain.text_preview,
                    mime_type: domain.mime_type,
                    file_extensions: domain.file_extensions,
                    file_names: domain.file_names,
                    link_urls: domain.link_urls,
                    source_device: domain.source_device,
                    payload_state: domain.payload_state,
                })
            })
            .collect::<Result<Vec<SearchResult>, SearchError>>()?;
        Ok(items)
    }

    /// Load tag memberships for `entry_ids`, grouped by entry id.
    ///
    /// Document rows do not carry tags (membership lives in `search_entry_tag`);
    /// the read side hydrates the current page's tags with this single batched
    /// query. Entries with no tags are absent from the map.
    fn load_tags_for_entries(
        conn: &mut SqliteConnection,
        profile_id: &str,
        entry_ids: &[String],
    ) -> Result<HashMap<String, Vec<TagId>>, SearchError> {
        use crate::db::schema::search_entry_tag::dsl;

        if entry_ids.is_empty() {
            return Ok(HashMap::new());
        }

        let rows: Vec<(String, String)> = dsl::search_entry_tag
            .filter(dsl::profile_id.eq(profile_id))
            .filter(dsl::entry_id.eq_any(entry_ids))
            .select((dsl::entry_id, dsl::tag_id))
            .load::<(String, String)>(conn)
            .map_err(|e| SearchError::Internal(format!("load_tags_for_entries failed: {e}")))?;

        let mut map: HashMap<String, Vec<TagId>> = HashMap::new();
        for (entry_id, tag_id) in rows {
            map.entry(entry_id).or_default().push(TagId::new(tag_id));
        }
        Ok(map)
    }

    /// Load the set of entry ids carrying any of `tag_ids` (OR semantics).
    /// Used to restrict search results by tag membership before pagination.
    fn load_entry_ids_for_tags(
        conn: &mut SqliteConnection,
        profile_id: &str,
        tag_ids: &[String],
    ) -> Result<HashSet<String>, SearchError> {
        use crate::db::schema::search_entry_tag::dsl;

        let rows = dsl::search_entry_tag
            .filter(dsl::profile_id.eq(profile_id))
            .filter(dsl::tag_id.eq_any(tag_ids))
            .select(dsl::entry_id)
            .load::<String>(conn)
            .map_err(|e| SearchError::Internal(format!("load_entry_ids_for_tags failed: {e}")))?;

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
                file_names TEXT NOT NULL DEFAULT '[]',
                link_urls TEXT NOT NULL DEFAULT '[]',
                source_device TEXT,
                payload_state TEXT,
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

        // Temp tag-membership table: same columns as search_entry_tag.
        let create_entry_tag = format!(
            "CREATE TABLE IF NOT EXISTS {tag_table} (
                profile_id TEXT NOT NULL,
                entry_id TEXT NOT NULL,
                tag_id TEXT NOT NULL,
                PRIMARY KEY (profile_id, entry_id, tag_id)
            )",
            tag_table = state.temp_entry_tag_table
        );

        diesel::sql_query(&create_doc)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("create temp doc table failed: {e}")))?;

        diesel::sql_query(&create_posting)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("create temp posting table failed: {e}")))?;

        diesel::sql_query(&create_entry_tag)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("create temp tag table failed: {e}")))?;

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
        let drop_entry_tag = format!("DROP TABLE IF EXISTS {}", state.temp_entry_tag_table);

        if let Err(e) = diesel::sql_query(&drop_doc).execute(conn) {
            warn!(table = %state.temp_document_table, error = %e, "failed to drop temp doc table");
        }
        if let Err(e) = diesel::sql_query(&drop_posting).execute(conn) {
            warn!(table = %state.temp_posting_table, error = %e, "failed to drop temp posting table");
        }
        if let Err(e) = diesel::sql_query(&drop_entry_tag).execute(conn) {
            warn!(table = %state.temp_entry_tag_table, error = %e, "failed to drop temp tag table");
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
              file_type, file_extensions, mime_type, indexed_at_ms, index_version, text_preview,
              file_names, link_urls, source_device, payload_state)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
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
            .bind::<diesel::sql_types::Text, _>(&doc_row.file_names)
            .bind::<diesel::sql_types::Text, _>(&doc_row.link_urls)
            .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(&doc_row.source_device)
            .bind::<diesel::sql_types::Nullable<diesel::sql_types::Text>, _>(&doc_row.payload_state)
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

        // Replace temp tag membership for this entry (mirror of document.tags).
        let del_tags = format!(
            "DELETE FROM {tag_table} WHERE profile_id = ? AND entry_id = ?",
            tag_table = state.temp_entry_tag_table
        );
        diesel::sql_query(&del_tags)
            .bind::<diesel::sql_types::Text, _>(profile_id)
            .bind::<diesel::sql_types::Text, _>(&entry_id_str)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("delete temp tags failed: {e}")))?;

        for tag_row in NewSearchEntryTagRow::rows_for_document(profile_id, document) {
            let insert_tag = format!(
                "INSERT OR REPLACE INTO {tag_table}
                 (profile_id, entry_id, tag_id)
                 VALUES (?, ?, ?)",
                tag_table = state.temp_entry_tag_table
            );
            diesel::sql_query(&insert_tag)
                .bind::<diesel::sql_types::Text, _>(&tag_row.profile_id)
                .bind::<diesel::sql_types::Text, _>(&tag_row.entry_id)
                .bind::<diesel::sql_types::Text, _>(&tag_row.tag_id)
                .execute(conn)
                .map_err(|e| SearchError::Internal(format!("insert temp tag failed: {e}")))?;
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

        let del_tags = format!(
            "DELETE FROM {tag_table} WHERE profile_id = ? AND entry_id = ?",
            tag_table = state.temp_entry_tag_table
        );
        diesel::sql_query(&del_tags)
            .bind::<diesel::sql_types::Text, _>(profile_id)
            .bind::<diesel::sql_types::Text, _>(&entry_id_str)
            .execute(conn)
            .map_err(|e| SearchError::Internal(format!("delete_temp_entry tags failed: {e}")))?;

        Ok(())
    }

    /// Finalize the rebuild by copying temp rows into the active tables in one transaction.
    ///
    /// Transaction sequence (three-table atomic cutover):
    /// 1. Delete active `search_posting` rows for `profile_id`
    /// 2. Delete active `search_document` rows for `profile_id`
    /// 3. Delete active `search_entry_tag` rows for `profile_id`
    /// 4. INSERT ... SELECT from temp posting table
    /// 5. INSERT ... SELECT from temp document table
    /// 6. INSERT ... SELECT from temp tag table
    /// 7. Update `search_index_meta`: version, unblock, completed_at_ms
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

            // 3. Delete active tag membership for profile.
            diesel::delete(
                search_entry_tag::table.filter(search_entry_tag::profile_id.eq(profile_id)),
            )
            .execute(tx)?;

            // 4. Copy temp postings into active table.
            let copy_postings = format!(
                "INSERT INTO search_posting
                 SELECT profile_id, term_tag, entry_id, field_mask, term_freq
                 FROM {post_table}",
                post_table = state.temp_posting_table
            );
            diesel::sql_query(&copy_postings).execute(tx)?;

            // 5. Copy temp documents into active table.
            let copy_docs = format!(
                "INSERT INTO search_document
                 SELECT profile_id, entry_id, event_id, active_time_ms, captured_at_ms,
                        file_type, file_extensions, mime_type, indexed_at_ms,
                        index_version, text_preview,
                        file_names, link_urls, source_device, payload_state
                 FROM {doc_table}",
                doc_table = state.temp_document_table
            );
            diesel::sql_query(&copy_docs).execute(tx)?;

            // 6. Copy temp tag membership into active table.
            let copy_tags = format!(
                "INSERT INTO search_entry_tag
                 SELECT profile_id, entry_id, tag_id
                 FROM {tag_table}",
                tag_table = state.temp_entry_tag_table
            );
            diesel::sql_query(&copy_tags).execute(tx)?;

            // 7. Update meta: unblock and record version + completion timestamp.
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
        // Pre-encode `content_type` to its stored snake_case string form once, so
        // both the SQL push-down and the in-memory term path compare against the
        // same representation as `file_type`.
        let content_types = query
            .content_types
            .iter()
            .map(|ct| serde_json::to_string(ct).map(|s| s.trim_matches('"').to_string()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SearchError::Internal(format!("content_type encode failed: {e}")))?;
        let filters = FilterParams {
            content_types,
            tags: query.tags.iter().map(|t| t.as_str().to_string()).collect(),
            extensions: query.extensions.iter().map(|e| e.to_lowercase()).collect(),
            source_devices: query
                .source_devices
                .iter()
                .map(|d| d.as_str().to_string())
                .collect(),
            time_range: query.time_range.clone(),
        };
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

            // 4. Resolve the current page of document rows + authoritative total.
            //    Filter-only browse pushes filtering, ordering, and pagination
            //    down to SQL so it never loads the whole profile into memory; the
            //    keyword path ranks the bounded posting-candidate set in memory.
            let (page_rows, total, has_more) = if is_filter_only {
                debug!("filter-only search — SQL push-down");
                Self::filter_only_page(&mut conn, &profile_id, &filters, limit, offset)?
            } else {
                Self::term_page(
                    &mut conn,
                    &profile_id,
                    &term_tags,
                    &operator,
                    &filters,
                    limit,
                    offset,
                )?
            };

            // 5. Hydrate the page's tags from `search_entry_tag` and map to
            //    domain results (shared by both paths).
            let items = Self::hydrate_results(&mut conn, &profile_id, page_rows)?;

            debug!(total, returned = items.len(), has_more, "search completed");

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

    #[instrument(
        name = "search_index.set_entry_favorite_tag",
        level = "debug",
        skip(self),
        fields(entry_id = %entry_id, favorited)
    )]
    async fn set_entry_favorite_tag(
        &self,
        entry_id: &EntryId,
        favorited: bool,
    ) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();
        let entry_id = entry_id.clone();
        let maybe_rebuild = self.active_rebuild_for_profile(&profile_id).await;

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;

            // 1. Always update the active table first. Only the favorited row is
            //    touched, leaving rule-derived tags (e.g. `link`) intact.
            Self::set_active_favorite_tag(&mut conn, &profile_id, &entry_id, favorited)?;

            // 2. Mirror into the rebuild temp table when a rebuild is active so
            //    the toggle survives cutover (best-effort, like index/remove).
            if let Some(rebuild_state) = maybe_rebuild {
                if let Err(e) =
                    Self::set_temp_favorite_tag(&mut conn, &rebuild_state, &entry_id, favorited)
                {
                    warn!(error = %e, "failed to mirror favorite tag into rebuild temp tables (best-effort)");
                }
            }

            Ok(())
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    #[instrument(name = "search_index.list_tags", level = "debug", skip(self))]
    async fn list_tags(&self) -> Result<Vec<SearchTagCount>, SearchError> {
        let profile_id = self.current_profile_id().await?.into_inner();
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            use crate::db::schema::search_entry_tag::dsl;
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            let rows: Vec<(String, i64)> = dsl::search_entry_tag
                .filter(dsl::profile_id.eq(&profile_id))
                .group_by(dsl::tag_id)
                .select((dsl::tag_id, diesel::dsl::count_star()))
                .load::<(String, i64)>(&mut conn)
                .map_err(|e| SearchError::Internal(format!("list_tags failed: {e}")))?;

            Ok(rows
                .into_iter()
                .map(|(tag_id, count)| SearchTagCount {
                    tag_id: TagId::new(tag_id),
                    count: count.max(0) as u32,
                })
                .collect())
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
    use crate::db::pool::init_db_pool;
    use tempfile::{tempdir, TempDir};
    use uc_core::ports::security::current_profile::CurrentProfileError;
    use uc_core::search::document::ContentType;
    use uc_core::search::key::SearchKey;
    use uc_core::search::tag::TagId;

    const TEST_PROFILE: &str = "default";

    struct FixedProfile;
    #[async_trait]
    impl CurrentProfilePort for FixedProfile {
        async fn current_profile(&self) -> Result<ProfileId, CurrentProfileError> {
            Ok(ProfileId::from(TEST_PROFILE))
        }
    }

    struct FixedKey;
    #[async_trait]
    impl SearchKeyDerivationPort for FixedKey {
        async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
            Ok(SearchKey([7u8; 32]))
        }
    }

    /// Search-key derivation that always reports a locked encryption session,
    /// modelling the runtime lock state for lock-contract assertions (§4.6).
    struct LockedKey;
    #[async_trait]
    impl SearchKeyDerivationPort for LockedKey {
        async fn derive_search_key(&self) -> Result<SearchKey, SearchError> {
            Err(SearchError::SessionLocked)
        }
    }

    /// Build an index over a fresh migrated SQLite file. The returned pool shares
    /// the same database so assertions can read the active tables directly.
    fn make_index() -> (SqliteSearchIndex, DbPool, TempDir) {
        make_index_with_key(Arc::new(FixedKey))
    }

    fn make_index_with_key(
        key: Arc<dyn SearchKeyDerivationPort>,
    ) -> (SqliteSearchIndex, DbPool, TempDir) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("search.sqlite");
        let pool = init_db_pool(path.to_str().unwrap()).unwrap();
        let index = SqliteSearchIndex::new(pool.clone(), Arc::new(FixedProfile), key);
        (index, pool, dir)
    }

    fn filter_only_query() -> SearchQuery {
        SearchQuery {
            query_string: String::new(),
            operator: QueryOperator::And,
            time_range: None,
            content_types: vec![],
            tags: vec![],
            extensions: vec![],
            source_devices: vec![],
            limit: 50,
            offset: 0,
        }
    }

    fn keyword_query(term: &str) -> SearchQuery {
        SearchQuery {
            query_string: term.to_string(),
            ..filter_only_query()
        }
    }

    fn make_doc(entry_id: &str, tags: Vec<TagId>) -> SearchDocument {
        SearchDocument {
            entry_id: entry_id.into(),
            event_id: format!("ev-{entry_id}").into(),
            active_time_ms: 1,
            captured_at_ms: 1,
            content_type: ContentType::Text,
            tags,
            file_extensions: vec![],
            mime_type: "text/plain".into(),
            indexed_at_ms: 1,
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: None,
            file_names: vec![],
            link_urls: vec![],
            source_device: None,
            payload_state: None,
        }
    }

    /// Load all `(entry_id, tag_id)` membership rows for the test profile, sorted.
    fn tag_rows(pool: &DbPool) -> Vec<(String, String)> {
        use crate::db::schema::search_entry_tag::dsl;
        let mut conn = pool.get().unwrap();
        let mut rows = dsl::search_entry_tag
            .filter(dsl::profile_id.eq(TEST_PROFILE))
            .select((dsl::entry_id, dsl::tag_id))
            .load::<(String, String)>(&mut conn)
            .unwrap();
        rows.sort();
        rows
    }

    fn doc_ids(pool: &DbPool) -> Vec<String> {
        let mut conn = pool.get().unwrap();
        let mut ids = search_document::table
            .filter(search_document::profile_id.eq(TEST_PROFILE))
            .select(search_document::entry_id)
            .load::<String>(&mut conn)
            .unwrap();
        ids.sort();
        ids
    }

    #[tokio::test]
    async fn index_entry_persists_tag_membership() {
        let (index, pool, _dir) = make_index();
        index
            .index_entry(make_doc("e1", vec![TagId::link()]), vec![])
            .await
            .unwrap();
        assert_eq!(tag_rows(&pool), vec![("e1".into(), "link".into())]);
    }

    #[tokio::test]
    async fn search_hydrates_tags_and_render_metadata() {
        let (index, _pool, _dir) = make_index();
        let mut doc = make_doc("e1", vec![TagId::link(), TagId::favorited()]);
        doc.file_names = vec!["a.txt".to_string()];
        doc.link_urls = vec!["https://example.com".to_string()];
        doc.source_device = Some("dev-1".to_string());
        doc.payload_state = Some("Lost".to_string());
        index.index_entry(doc, vec![]).await.unwrap();

        let page = index.search(filter_only_query()).await.unwrap();
        assert_eq!(page.total, 1);
        let result = &page.items[0];
        assert_eq!(result.entry_id.to_string(), "e1");

        // Tags are hydrated from `search_entry_tag`, not the document row.
        let mut tags: Vec<String> = result.tags.iter().map(|t| t.to_string()).collect();
        tags.sort();
        assert_eq!(tags, vec!["favorited".to_string(), "link".to_string()]);

        // Render metadata carried through from the document row.
        assert_eq!(result.file_names, vec!["a.txt".to_string()]);
        assert_eq!(result.link_urls, vec!["https://example.com".to_string()]);
        assert_eq!(result.source_device.as_deref(), Some("dev-1"));
        assert_eq!(result.payload_state.as_deref(), Some("Lost"));
    }

    #[tokio::test]
    async fn search_filters_by_tag_membership() {
        let (index, _pool, _dir) = make_index();
        index
            .index_entry(make_doc("e1", vec![TagId::link()]), vec![])
            .await
            .unwrap();
        index
            .index_entry(make_doc("e2", vec![]), vec![])
            .await
            .unwrap();

        // No tag filter → both entries.
        let all = index.search(filter_only_query()).await.unwrap();
        assert_eq!(all.total, 2);

        // Filter by the link tag → only the entry that carries it.
        let linked = index
            .search(SearchQuery {
                tags: vec![TagId::link()],
                ..filter_only_query()
            })
            .await
            .unwrap();
        assert_eq!(linked.total, 1);
        assert_eq!(linked.items[0].entry_id.to_string(), "e1");

        // Filter by a tag no entry carries → empty.
        let none = index
            .search(SearchQuery {
                tags: vec![TagId::favorited()],
                ..filter_only_query()
            })
            .await
            .unwrap();
        assert_eq!(none.total, 0);
    }

    #[tokio::test]
    async fn set_favorite_tag_adds_and_removes_only_favorited_row() {
        let (index, pool, _dir) = make_index();
        // The entry already carries a rule-derived `link` tag.
        index
            .index_entry(make_doc("e1", vec![TagId::link()]), vec![])
            .await
            .unwrap();

        // Favoriting adds the favorited row, leaving `link` intact.
        index
            .set_entry_favorite_tag(&EntryId::from("e1"), true)
            .await
            .unwrap();
        assert_eq!(
            tag_rows(&pool),
            vec![
                ("e1".into(), "favorited".into()),
                ("e1".into(), "link".into()),
            ]
        );

        // Idempotent: favoriting again does not duplicate the row.
        index
            .set_entry_favorite_tag(&EntryId::from("e1"), true)
            .await
            .unwrap();
        assert_eq!(
            tag_rows(&pool),
            vec![
                ("e1".into(), "favorited".into()),
                ("e1".into(), "link".into()),
            ]
        );

        // Unfavoriting removes only the favorited row; `link` survives.
        index
            .set_entry_favorite_tag(&EntryId::from("e1"), false)
            .await
            .unwrap();
        assert_eq!(tag_rows(&pool), vec![("e1".into(), "link".into())]);
    }

    #[tokio::test]
    async fn list_tags_reports_distinct_entry_counts_per_tag() {
        let (index, _pool, _dir) = make_index();
        index
            .index_entry(make_doc("e1", vec![TagId::link()]), vec![])
            .await
            .unwrap();
        index
            .index_entry(make_doc("e2", vec![TagId::link()]), vec![])
            .await
            .unwrap();
        index
            .index_entry(make_doc("e3", vec![]), vec![])
            .await
            .unwrap();
        index
            .set_entry_favorite_tag(&EntryId::from("e1"), true)
            .await
            .unwrap();

        let mut tags = index.list_tags().await.unwrap();
        tags.sort_by(|a, b| a.tag_id.as_str().cmp(b.tag_id.as_str()));
        assert_eq!(tags.len(), 2);
        assert_eq!(tags[0].tag_id, TagId::favorited());
        assert_eq!(tags[0].count, 1);
        assert_eq!(tags[1].tag_id, TagId::link());
        assert_eq!(tags[1].count, 2);
    }

    #[tokio::test]
    async fn rebuild_cutover_repopulates_tags_and_clears_stale() {
        let (index, pool, _dir) = make_index();

        // Pre-existing active entry simulates a prior-version index. The rebuild
        // must drop it and repopulate from the supplied entries.
        index
            .index_entry(make_doc("stale", vec![TagId::link()]), vec![])
            .await
            .unwrap();

        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        index
            .rebuild(
                vec![
                    (make_doc("e1", vec![TagId::link()]), vec![]),
                    (make_doc("e2", vec![]), vec![]),
                ],
                tx,
            )
            .await
            .unwrap();

        // Documents cut over: stale gone, e1/e2 present.
        assert_eq!(doc_ids(&pool), vec!["e1".to_string(), "e2".to_string()]);
        // Tags cut over alongside documents: only e1 carries `link`.
        assert_eq!(tag_rows(&pool), vec![("e1".into(), "link".into())]);

        // Meta unblocked and bumped to the current version.
        let meta = index.get_index_meta().await.unwrap();
        assert!(!meta.search_blocked);
        assert_eq!(meta.index_version, CURRENT_INDEX_VERSION);
    }

    #[tokio::test]
    async fn remove_entry_deletes_tag_membership() {
        let (index, pool, _dir) = make_index();
        index
            .index_entry(make_doc("e1", vec![TagId::link()]), vec![])
            .await
            .unwrap();
        index.remove_entry(&EntryId::from("e1")).await.unwrap();
        assert!(tag_rows(&pool).is_empty());
    }

    /// Lock contract (§4.6): a filter-only / empty browse never derives the
    /// search key, so it returns results even while the encryption session is
    /// locked. This is what lets the daemon serve browse without unlocking.
    #[tokio::test]
    async fn filter_only_browse_succeeds_when_session_locked() {
        let (index, _pool, _dir) = make_index_with_key(Arc::new(LockedKey));
        index
            .index_entry(make_doc("e1", vec![]), vec![])
            .await
            .unwrap();

        let page = index.search(filter_only_query()).await.unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].entry_id.to_string(), "e1");
    }

    /// Lock contract (§4.6): a keyword search must derive the search key, which
    /// is unavailable while locked, so it surfaces `SessionLocked` rather than
    /// silently degrading to unkeyed or empty results.
    #[tokio::test]
    async fn keyword_search_rejected_when_session_locked() {
        let (index, _pool, _dir) = make_index_with_key(Arc::new(LockedKey));
        index
            .index_entry(make_doc("e1", vec![]), vec![])
            .await
            .unwrap();

        let err = index.search(keyword_query("hello")).await.unwrap_err();

        assert!(matches!(err, SearchError::SessionLocked));
    }

    /// Read the render columns of one document row as `(file_names, link_urls,
    /// source_device, payload_state)`. `file_names`/`link_urls` come back as the
    /// stored JSON-array strings.
    fn render_cols(
        pool: &DbPool,
        entry_id: &str,
    ) -> (String, String, Option<String>, Option<String>) {
        let mut conn = pool.get().unwrap();
        search_document::table
            .filter(search_document::profile_id.eq(TEST_PROFILE))
            .filter(search_document::entry_id.eq(entry_id))
            .select((
                search_document::file_names,
                search_document::link_urls,
                search_document::source_device,
                search_document::payload_state,
            ))
            .first::<(String, String, Option<String>, Option<String>)>(&mut conn)
            .unwrap()
    }

    fn doc_with_render(entry_id: &str) -> SearchDocument {
        let mut doc = make_doc(entry_id, vec![TagId::link()]);
        doc.file_names = vec!["a.txt".to_string()];
        doc.link_urls = vec!["https://example.com".to_string()];
        doc.source_device = Some("dev-1".to_string());
        doc.payload_state = Some("Lost".to_string());
        doc
    }

    /// The live index path (`upsert_active_entry`, diesel `replace_into`) writes
    /// every render column.
    #[tokio::test]
    async fn index_entry_persists_render_columns() {
        let (index, pool, _dir) = make_index();
        index
            .index_entry(doc_with_render("e1"), vec![])
            .await
            .unwrap();

        let (file_names, link_urls, source_device, payload_state) = render_cols(&pool, "e1");
        assert_eq!(file_names, r#"["a.txt"]"#);
        assert_eq!(link_urls, r#"["https://example.com"]"#);
        assert_eq!(source_device.as_deref(), Some("dev-1"));
        assert_eq!(payload_state.as_deref(), Some("Lost"));
    }

    /// The rebuild path (temp table DDL + `insert_temp_entry` binds + the
    /// `INSERT … SELECT` cutover) must carry the render columns through unchanged
    /// — the v5 "fields survive rebuild" gate.
    #[tokio::test]
    async fn rebuild_preserves_render_columns() {
        let (index, pool, _dir) = make_index();

        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        index
            .rebuild(vec![(doc_with_render("e1"), vec![])], tx)
            .await
            .unwrap();

        let (file_names, link_urls, source_device, payload_state) = render_cols(&pool, "e1");
        assert_eq!(file_names, r#"["a.txt"]"#);
        assert_eq!(link_urls, r#"["https://example.com"]"#);
        assert_eq!(source_device.as_deref(), Some("dev-1"));
        assert_eq!(payload_state.as_deref(), Some("Lost"));
    }

    /// End-to-end upgrade from a pre-v5 on-disk database (not a fresh one): an
    /// old row written before the render columns existed must survive the
    /// `ALTER TABLE ADD COLUMN` migration and then get its render columns
    /// backfilled by the version-mismatch rebuild. Reverting and re-applying the
    /// real migration also exercises its down/up reversibility.
    #[tokio::test]
    async fn upgrade_from_pre_render_schema_backfills_render_columns() {
        use crate::db::pool::MIGRATIONS;
        use diesel_migrations::MigrationHarness;

        let (index, pool, _dir) = make_index();

        // 1. Roll back the render-columns migration so `search_document` looks
        //    like the v4-era schema (original 11 columns, no render columns).
        {
            let mut conn = pool.get().unwrap();
            conn.revert_last_migration(MIGRATIONS)
                .expect("down.sql drops the render columns");
        }

        // 2. Seed a row the pre-v5 code path would have written: only the
        //    original columns are set, and the index is at the old version.
        {
            let mut conn = pool.get().unwrap();
            diesel::sql_query(
                "INSERT INTO search_document
                 (profile_id, entry_id, event_id, active_time_ms, captured_at_ms,
                  file_type, file_extensions, mime_type, indexed_at_ms, index_version, text_preview)
                 VALUES (?, 'old1', 'ev-old1', 1, 1, 'text', '[]', 'text/plain', 1, 'search-v4', 'old')",
            )
            .bind::<diesel::sql_types::Text, _>(TEST_PROFILE)
            .execute(&mut conn)
            .unwrap();
            diesel::sql_query(
                "UPDATE search_index_meta SET index_version = 'search-v4' WHERE profile_id = ?",
            )
            .bind::<diesel::sql_types::Text, _>(TEST_PROFILE)
            .execute(&mut conn)
            .unwrap();
        }

        // 3. Upgrade: re-apply the migration. ADD COLUMN runs against a populated
        //    table — the old row must survive and pick up column defaults.
        {
            let mut conn = pool.get().unwrap();
            conn.run_pending_migrations(MIGRATIONS)
                .expect("up.sql re-adds the render columns");
        }
        assert_eq!(
            render_cols(&pool, "old1"),
            ("[]".to_string(), "[]".to_string(), None, None),
            "old row survives ADD COLUMN with defaults"
        );

        // 4. The version-mismatch rebuild reprojects from the main store with the
        //    v5 code, backfilling render columns onto what were old rows.
        let (tx, _rx) = tokio::sync::mpsc::channel(64);
        index
            .rebuild(vec![(doc_with_render("old1"), vec![])], tx)
            .await
            .unwrap();

        let (file_names, link_urls, source_device, payload_state) = render_cols(&pool, "old1");
        assert_eq!(file_names, r#"["a.txt"]"#);
        assert_eq!(link_urls, r#"["https://example.com"]"#);
        assert_eq!(source_device.as_deref(), Some("dev-1"));
        assert_eq!(payload_state.as_deref(), Some("Lost"));

        let meta = index.get_index_meta().await.unwrap();
        assert_eq!(meta.index_version, CURRENT_INDEX_VERSION);
    }

    // ── Phase 2: filter-only SQL push-down acceptance ──────────────────────

    /// Bulk-insert `n` text documents straight into `search_document`, bypassing
    /// the live index path, so push-down tests can reach large ordered datasets
    /// cheaply. `active_time_ms` ascends with `i`, so higher `i` sorts first.
    fn bulk_insert_text_docs(pool: &DbPool, n: usize) {
        let mut conn = pool.get().unwrap();
        let rows: Vec<NewSearchDocumentRow> = (0..n)
            .map(|i| NewSearchDocumentRow {
                profile_id: TEST_PROFILE.to_string(),
                entry_id: format!("e{i:06}"),
                event_id: format!("ev{i:06}"),
                active_time_ms: i as i64,
                captured_at_ms: i as i64,
                file_type: "text".to_string(),
                file_extensions: "[]".to_string(),
                mime_type: "text/plain".to_string(),
                indexed_at_ms: 0,
                index_version: CURRENT_INDEX_VERSION.to_string(),
                text_preview: None,
                file_names: "[]".to_string(),
                link_urls: "[]".to_string(),
                source_device: None,
                payload_state: None,
            })
            .collect();
        // Chunk to stay well within SQLite's bound-variable limit.
        for chunk in rows.chunks(400) {
            diesel::insert_into(search_document::table)
                .values(chunk)
                .execute(&mut conn)
                .unwrap();
        }
    }

    /// Run `EXPLAIN QUERY PLAN` and return the joined `detail` column.
    fn explain_query_plan(pool: &DbPool, sql: &str) -> String {
        #[derive(QueryableByName)]
        struct PlanRow {
            #[diesel(sql_type = diesel::sql_types::Text)]
            detail: String,
        }
        let mut conn = pool.get().unwrap();
        diesel::sql_query(format!("EXPLAIN QUERY PLAN {sql}"))
            .load::<PlanRow>(&mut conn)
            .unwrap()
            .into_iter()
            .map(|r| r.detail)
            .collect::<Vec<_>>()
            .join(" | ")
    }

    /// Structural gate: the dominant browse path (no filter) must be served by
    /// `idx_search_document_profile_active_time` with no temp-b-tree sort, proving
    /// the page is index-ordered and bounded — not a full-table load + sort.
    #[tokio::test]
    async fn filter_only_browse_is_index_served_not_full_scan() {
        let (_index, pool, _dir) = make_index();
        bulk_insert_text_docs(&pool, 500);

        let plan = explain_query_plan(
            &pool,
            "SELECT * FROM search_document WHERE profile_id = 'default' \
             ORDER BY active_time_ms DESC LIMIT 50 OFFSET 0",
        );

        assert!(
            plan.contains("idx_search_document_profile_active_time"),
            "browse should be served by the active-time index; plan: {plan}"
        );
        assert!(
            !plan.contains("USE TEMP B-TREE"),
            "browse ordering must be index-covered (no temp sort); plan: {plan}"
        );
    }

    /// Structural gate: the tag-membership subquery is served by an index (not a
    /// full `SCAN`) — the "tag JOIN index" the design calls for. With the
    /// covering `(profile_id, tag_id, entry_id)` index in place, the planner
    /// seeks matching tag rows without touching the table.
    #[tokio::test]
    async fn tag_filter_is_served_by_entry_tag_index() {
        let (_index, pool, _dir) = make_index();
        bulk_insert_text_docs(&pool, 500);
        // Populate membership so the planner has a reason to seek selectively.
        {
            let mut conn = pool.get().unwrap();
            let tags: Vec<NewSearchEntryTagRow> = (0..500)
                .step_by(5)
                .map(|i| NewSearchEntryTagRow {
                    profile_id: TEST_PROFILE.to_string(),
                    entry_id: format!("e{i:06}"),
                    tag_id: "link".to_string(),
                })
                .collect();
            diesel::insert_into(search_entry_tag::table)
                .values(&tags)
                .execute(&mut conn)
                .unwrap();
        }

        let plan = explain_query_plan(
            &pool,
            "SELECT * FROM search_document WHERE profile_id = 'default' \
             AND entry_id IN (SELECT entry_id FROM search_entry_tag \
               WHERE profile_id = 'default' AND tag_id IN ('link')) \
             ORDER BY active_time_ms DESC LIMIT 50",
        );

        // The membership lookup must be index-served, never a full table scan.
        assert!(
            !plan.contains("SCAN search_entry_tag"),
            "tag membership must not full-scan search_entry_tag; plan: {plan}"
        );
        // The widened `(profile_id, tag_id, entry_id)` index lets the planner seek
        // matching tag rows AND read entry_id straight from the index (covering).
        assert!(
            plan.contains("search_entry_tag USING COVERING INDEX"),
            "tag membership should use a covering index seek; plan: {plan}"
        );
        // The outer browse order stays index-served by the active-time index.
        assert!(
            plan.contains("idx_search_document_profile_active_time"),
            "outer query should use the active-time index; plan: {plan}"
        );
    }

    /// The push-down returns the same ordered, paginated, authoritative-total
    /// result the in-memory path used to — across page boundaries.
    #[tokio::test]
    async fn filter_only_pushdown_orders_and_paginates() {
        let (index, pool, _dir) = make_index();
        bulk_insert_text_docs(&pool, 120);

        // Page 1: newest first (active_time desc → e000119 … e000070).
        let p1 = {
            let mut q = filter_only_query();
            q.limit = 50;
            q.offset = 0;
            index.search(q).await.unwrap()
        };
        assert_eq!(p1.total, 120);
        assert_eq!(p1.items.len(), 50);
        assert!(p1.has_more);
        assert_eq!(p1.items[0].entry_id.to_string(), "e000119");
        assert_eq!(p1.items[49].entry_id.to_string(), "e000070");

        // Page 2 continues without overlap, total unchanged.
        let p2 = {
            let mut q = filter_only_query();
            q.limit = 50;
            q.offset = 50;
            index.search(q).await.unwrap()
        };
        assert_eq!(p2.total, 120);
        assert_eq!(p2.items[0].entry_id.to_string(), "e000069");
        assert!(p2.has_more);

        // Final partial page: no more entries afterwards.
        let p3 = {
            let mut q = filter_only_query();
            q.limit = 50;
            q.offset = 100;
            index.search(q).await.unwrap()
        };
        assert_eq!(p3.items.len(), 20);
        assert!(!p3.has_more);
        assert_eq!(p3.items[19].entry_id.to_string(), "e000000");
    }

    /// Source-device filtering reads the indexed `search_document.source_device`
    /// column (no clipboard_event JOIN) and runs before the total/pagination.
    #[tokio::test]
    async fn filter_only_filters_by_source_device_column() {
        let (index, _pool, _dir) = make_index();
        let mut a = make_doc("e1", vec![]);
        a.source_device = Some("dev-a".to_string());
        let mut b = make_doc("e2", vec![]);
        b.source_device = Some("dev-b".to_string());
        index.index_entry(a, vec![]).await.unwrap();
        index.index_entry(b, vec![]).await.unwrap();

        let mut q = filter_only_query();
        q.source_devices = vec![uc_core::ids::DeviceId::new("dev-a")];
        let page = index.search(q).await.unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].entry_id.to_string(), "e1");
    }

    /// Content-type filtering matches the stored snake_case `file_type`.
    #[tokio::test]
    async fn filter_only_filters_by_content_type() {
        let (index, _pool, _dir) = make_index();
        let mut img = make_doc("img", vec![]);
        img.content_type = ContentType::Image;
        index
            .index_entry(make_doc("txt", vec![]), vec![])
            .await
            .unwrap();
        index.index_entry(img, vec![]).await.unwrap();

        let mut q = filter_only_query();
        q.content_types = vec![ContentType::Image];
        let page = index.search(q).await.unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].entry_id.to_string(), "img");
    }

    /// Extension filtering is case-insensitive via `json_each` membership: a
    /// stored upper-case extension matches a lower-case query extension.
    #[tokio::test]
    async fn filter_only_filters_by_extension_case_insensitively() {
        let (index, _pool, _dir) = make_index();
        let mut pdf = make_doc("pdf", vec![]);
        pdf.file_extensions = vec!["PDF".to_string()];
        let mut txt = make_doc("txt", vec![]);
        txt.file_extensions = vec!["txt".to_string()];
        index.index_entry(pdf, vec![]).await.unwrap();
        index.index_entry(txt, vec![]).await.unwrap();

        let mut q = filter_only_query();
        q.extensions = vec!["pdf".to_string()];
        let page = index.search(q).await.unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].entry_id.to_string(), "pdf");
    }

    /// Absolute time-range filtering is pushed down as a `BETWEEN` predicate.
    #[tokio::test]
    async fn filter_only_filters_by_absolute_time_range() {
        let (index, _pool, _dir) = make_index();
        let mut old = make_doc("old", vec![]);
        old.active_time_ms = 1_000;
        let mut recent = make_doc("recent", vec![]);
        recent.active_time_ms = 100_000;
        index.index_entry(old, vec![]).await.unwrap();
        index.index_entry(recent, vec![]).await.unwrap();

        let mut q = filter_only_query();
        q.time_range = Some(TimeRangeFilter::Absolute {
            from_ms: 50_000,
            to_ms: 200_000,
        });
        let page = index.search(q).await.unwrap();

        assert_eq!(page.total, 1);
        assert_eq!(page.items[0].entry_id.to_string(), "recent");
    }

    /// Manual P95 harness (excluded from CI). Seeds N=100k and prints browse +
    /// tag-filtered latency percentiles. Targets: browse P95 ≤ 100ms, filtered
    /// P95 ≤ 200ms. Run with:
    ///   `cargo test -p uc-infra --lib filter_only_pushdown_p95 -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "perf harness; run manually with --ignored --nocapture"]
    async fn filter_only_pushdown_p95_harness() {
        use std::time::{Duration, Instant};

        let (index, pool, _dir) = make_index();
        let n = 100_000usize;
        bulk_insert_text_docs(&pool, n);

        // Tag every 10th entry `link` so the filtered measurement exercises the
        // tag subquery against a realistic membership table.
        {
            let mut conn = pool.get().unwrap();
            let tags: Vec<NewSearchEntryTagRow> = (0..n)
                .step_by(10)
                .map(|i| NewSearchEntryTagRow {
                    profile_id: TEST_PROFILE.to_string(),
                    entry_id: format!("e{i:06}"),
                    tag_id: TagId::link().as_str().to_string(),
                })
                .collect();
            for chunk in tags.chunks(400) {
                diesel::insert_into(search_entry_tag::table)
                    .values(chunk)
                    .execute(&mut conn)
                    .unwrap();
            }
        }

        let p95 = |mut samples: Vec<Duration>| -> Duration {
            samples.sort();
            let idx = ((samples.len() as f64 * 0.95).ceil() as usize)
                .saturating_sub(1)
                .min(samples.len() - 1);
            samples[idx]
        };

        let runs = 50;
        let mut browse = Vec::with_capacity(runs);
        for _ in 0..runs {
            let q = filter_only_query();
            let start = Instant::now();
            let page = index.search(q).await.unwrap();
            browse.push(start.elapsed());
            assert_eq!(page.total, n as u32);
        }

        let mut filtered = Vec::with_capacity(runs);
        for _ in 0..runs {
            let mut q = filter_only_query();
            q.tags = vec![TagId::link()];
            let start = Instant::now();
            let page = index.search(q).await.unwrap();
            filtered.push(start.elapsed());
            assert_eq!(page.total, (n / 10) as u32);
        }

        let browse_p95 = p95(browse);
        let filtered_p95 = p95(filtered);
        println!("N={n} browse P95 = {browse_p95:?} | filtered(link) P95 = {filtered_p95:?}");

        // Generous ceilings so the harness flags only gross regressions; the
        // documented targets are browse ≤ 100ms / filtered ≤ 200ms.
        assert!(
            browse_p95 <= Duration::from_millis(500),
            "browse P95 {browse_p95:?} far over target (≤100ms)"
        );
        assert!(
            filtered_p95 <= Duration::from_millis(1000),
            "filtered P95 {filtered_p95:?} far over target (≤200ms)"
        );
    }
}
