//! SQLite implementation of `SearchIndexPort`.
//!
//! `SqliteSearchIndex` is the single authoritative adapter for the local encrypted
//! search index. It owns:
//! - Meta-row seeding / loading per profile
//! - Live active-table upsert / hard-delete for `search_document` + `search_posting`
//! - Blocked-state and version-mismatch guards for `search()`
//! - Real SQLite posting-based AND/OR query resolution
//!
//! Phase 92 will wire this adapter into daemon routes. Phase 91 Plan 02 will add
//! the `rebuild()` temp-table flow.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use diesel::prelude::*;
use diesel::RunQueryDsl;
use tokio::sync::mpsc::Sender;
use tracing::{debug, warn};

use uc_core::ids::EntryId;
use uc_core::ports::search::search_index::SearchIndexPort;
use uc_core::ports::search::search_key::SearchKeyDerivationPort;
use uc_core::ports::security::key_scope::KeyScopePort;
use uc_core::search::document::{SearchDocument, SearchIndexMeta, SearchPosting};
use uc_core::search::error::SearchError;
use uc_core::search::query::{QueryOperator, SearchQuery, TimeRangeFilter};
use uc_core::search::result::{RebuildProgress, SearchResult};

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
// Public adapter struct
// ──────────────────────────────────────────────────────────────────────────────

/// SQLite adapter implementing `SearchIndexPort`.
///
/// Holds a connection pool and the two async ports needed for profile-scoped
/// key derivation. `rebuild_state` is an `Arc<RwLock<...>>` owned here so that
/// Plan 02's rebuild flow can mirror live mutations into the temp tables.
pub struct SqliteSearchIndex {
    pool: DbPool,
    key_scope: Arc<dyn KeyScopePort>,
    search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
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

    // ─── Private synchronous helpers (run inside spawn_blocking) ─────────────

    /// Ensure a `search_index_meta` row exists for `profile_id`.
    ///
    /// If the row is missing, inserts a fresh seed row via `NewSearchIndexMetaRow::seed`.
    fn ensure_meta_row(
        conn: &mut SqliteConnection,
        profile_id: &str,
    ) -> Result<(), SearchError> {
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
    fn mark_blocked(
        conn: &mut SqliteConnection,
        profile_id: &str,
    ) -> Result<(), SearchError> {
        use crate::db::schema::search_index_meta::dsl;

        diesel::update(
            dsl::search_index_meta.filter(dsl::profile_id.eq(profile_id)),
        )
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
    /// Returns `SearchError::InvalidQuery` when the query produces no searchable terms.
    fn normalize_query_terms(query: &SearchQuery) -> Result<Vec<String>, SearchError> {
        let trimmed = query.query_string.trim();
        if trimmed.is_empty() {
            return Err(SearchError::InvalidQuery(
                "query produced no searchable terms".to_string(),
            ));
        }

        let tokenizer = SearchTokenizer;

        // Split on whitespace to get individual query words, then tokenize each.
        // This prevents multi-word query strings from generating whole-segment tokens.
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        let segments: Vec<String> = words.iter().map(|w| w.to_string()).collect();
        let raw_tokens = tokenizer.tokenize_all(&segments);

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
}

// ──────────────────────────────────────────────────────────────────────────────
// SearchIndexPort implementation
// ──────────────────────────────────────────────────────────────────────────────

#[async_trait]
impl SearchIndexPort for SqliteSearchIndex {
    async fn index_entry(
        &self,
        document: SearchDocument,
        postings: Vec<SearchPosting>,
    ) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?;
        let pool = self.pool.clone();

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;
            Self::upsert_active_entry(&mut conn, &profile_id, &document, &postings)
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError> {
        let profile_id = self.current_profile_id().await?;
        let pool = self.pool.clone();
        let entry_id = entry_id.clone();

        tokio::task::spawn_blocking(move || {
            let mut conn = pool
                .get()
                .map_err(|e| SearchError::Internal(format!("pool error: {e}")))?;

            Self::ensure_meta_row(&mut conn, &profile_id)?;
            Self::delete_active_entry(&mut conn, &profile_id, &entry_id)
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    async fn search(&self, query: SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        let profile_id = self.current_profile_id().await?;
        let pool = self.pool.clone();

        // Normalize query terms before entering spawn_blocking.
        let terms = Self::normalize_query_terms(&query)?;

        // Derive search key (async, must happen before spawn_blocking).
        let search_key = self.search_key_derivation.derive_search_key().await?;

        // Compute HMAC term tags for all normalized terms.
        let term_tags: Vec<Vec<u8>> = terms
            .iter()
            .map(|t| term_tag(&search_key, t))
            .collect::<Result<_, _>>()
            .map_err(|e| SearchError::Internal(format!("term_tag computation failed: {e}")))?;

        let operator = query.operator.clone();
        let time_range = query.time_range.clone();
        let file_types = query.file_types.clone();
        let extensions = query.extensions.iter().map(|e| e.to_lowercase()).collect::<Vec<_>>();
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

            // 4. Candidate posting resolution.
            let hit_map =
                Self::query_candidate_hits(&mut conn, &profile_id, &term_tags, &operator)?;

            if hit_map.is_empty() {
                return Ok(vec![]);
            }

            let candidate_ids: Vec<String> = hit_map.keys().cloned().collect();

            // 5. Load candidate documents.
            let docs = Self::load_candidate_documents(&mut conn, &profile_id, &candidate_ids)?;

            // 6. Apply filters: time range, file type, extension.
            let now_ms = chrono::Utc::now().timestamp_millis();

            let filtered: Vec<(SearchDocumentRow, u32)> = docs
                .into_iter()
                .filter_map(|doc| {
                    // Time range filter.
                    if let Some(ref tr) = time_range {
                        let (from_ms, to_ms) = resolve_time_range(tr, now_ms);
                        if doc.active_time_ms < from_ms as i64
                            || doc.active_time_ms > to_ms as i64
                        {
                            return None;
                        }
                    }

                    // File type filter.
                    if !file_types.is_empty() {
                        let stored = &doc.file_type;
                        let matches = file_types.iter().any(|ft| {
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

            // 8. Pagination.
            let paginated: Vec<(SearchDocumentRow, u32)> =
                sorted.into_iter().skip(offset).take(limit).collect();

            // 9. Map to SearchResult.
            let results: Vec<SearchResult> = paginated
                .into_iter()
                .filter_map(|(doc, _)| {
                    let domain = doc.to_domain().ok()?;
                    Some(SearchResult {
                        entry_id: domain.entry_id,
                        file_type: domain.file_type,
                        active_time_ms: domain.active_time_ms,
                        text_preview: domain.text_preview,
                        mime_type: domain.mime_type,
                        file_extensions: domain.file_extensions,
                    })
                })
                .collect();

            Ok(results)
        })
        .await
        .map_err(|e| SearchError::Internal(format!("spawn_blocking error: {e}")))?
    }

    /// Rebuild stub — full implementation in Plan 02.
    async fn rebuild(
        &self,
        _entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
        _progress_tx: Sender<RebuildProgress>,
    ) -> Result<(), SearchError> {
        Err(SearchError::Internal(
            "rebuild not yet implemented (Plan 02)".to_string(),
        ))
    }

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
    use uc_core::search::document::{FileType, SearchDocument, SearchPosting};
    use uc_core::search::error::SearchError;
    use uc_core::search::key::SearchKey;
    use uc_core::security::model::KeyScope;

    use crate::db::pool::init_db_pool;
    use crate::search::search_key_derivation::term_tag;
    use crate::search::constants::CURRENT_INDEX_VERSION;

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
            file_type: FileType::Text,
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
        assert_eq!(posting_count, 0, "expected 0 search_posting rows after remove");
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
        adapter.index_entry(doc_a, postings_a).await.expect("index A");

        // entry-B has only "alpha"
        let doc_b = SearchDocument {
            entry_id: EntryId::from("entry-B"),
            active_time_ms: 1_000_000,
            ..sample_document("entry-B")
        };
        let postings_b = make_postings("entry-B", &["alpha"]);
        adapter.index_entry(doc_b, postings_b).await.expect("index B");

        let query = SearchQuery {
            query_string: "alpha beta".to_string(),
            operator: QueryOperator::And,
            time_range: None,
            file_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };

        let results = adapter.search(query).await.expect("search");
        assert_eq!(results.len(), 1, "AND mode must require all terms: {results:?}");
        assert_eq!(results[0].entry_id, EntryId::from("entry-A"));
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
        adapter.index_entry(doc_a, postings_a).await.expect("index A");

        // entry-B: "beta"
        let doc_b = SearchDocument {
            entry_id: EntryId::from("entry-B"),
            active_time_ms: 1_000_000,
            ..sample_document("entry-B")
        };
        let postings_b = make_postings("entry-B", &["beta"]);
        adapter.index_entry(doc_b, postings_b).await.expect("index B");

        let query = SearchQuery {
            query_string: "alpha beta".to_string(),
            operator: QueryOperator::Or,
            time_range: None,
            file_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };

        let results = adapter.search(query).await.expect("search");
        assert_eq!(results.len(), 2, "OR mode must return both entries: {results:?}");
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
            file_type: FileType::Text,
            file_extensions: vec!["txt".to_string()],
            ..sample_document("entry-match")
        };
        let postings_match = make_postings("entry-match", &["hello"]);
        adapter.index_entry(doc_match, postings_match).await.expect("index match");

        // entry-old: old entry (30+ days ago)
        let doc_old = SearchDocument {
            entry_id: EntryId::from("entry-old"),
            active_time_ms: now_ms - 40 * 86_400_000, // 40 days ago
            captured_at_ms: now_ms - 40 * 86_400_000,
            file_type: FileType::Text,
            file_extensions: vec!["txt".to_string()],
            ..sample_document("entry-old")
        };
        let postings_old = make_postings("entry-old", &["hello"]);
        adapter.index_entry(doc_old, postings_old).await.expect("index old");

        // entry-image: recent but wrong type
        let doc_image = SearchDocument {
            entry_id: EntryId::from("entry-image"),
            active_time_ms: now_ms - 3600_000,
            captured_at_ms: now_ms - 3600_000,
            file_type: FileType::Image,
            file_extensions: vec!["png".to_string()],
            ..sample_document("entry-image")
        };
        let postings_image = make_postings("entry-image", &["hello"]);
        adapter.index_entry(doc_image, postings_image).await.expect("index image");

        // Query: last 7 days, text type, txt extension
        let query = SearchQuery {
            query_string: "hello".to_string(),
            operator: QueryOperator::Or,
            time_range: Some(TimeRangeFilter::Last7d),
            file_types: vec![FileType::Text],
            extensions: vec!["txt".to_string()],
            limit: 10,
            offset: 0,
        };

        let results = adapter.search(query).await.expect("search");
        assert_eq!(results.len(), 1, "only entry-match should pass all filters: {results:?}");
        assert_eq!(results[0].entry_id, EntryId::from("entry-match"));
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
            file_types: vec![],
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

        let result2 = adapter.search(query).await;
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
            assert!(row.search_blocked, "version mismatch must set search_blocked = true");
        }
    }

}
