//! Adapter-owned Diesel row types for the three search tables.
//!
//! These types carry `profile_id` as a persistence concern, keeping it out of
//! `uc-core` search domain structs (`SearchDocument`, `SearchPosting`, `SearchIndexMeta`).
//!
//! Type-mapping notes:
//! - `field_mask`: domain `u8`, Diesel `Integer` → row uses `i32`; cast on conversion.
//! - `term_freq`: domain `u32`, Diesel `Integer` → row uses `i32`; cast on conversion.
//!   The `term_freq > 0` CHECK constraint guarantees it fits safely in both.
//! - `file_type`: stored as `TEXT` using serde snake_case serialization.
//! - `file_extensions`: stored as JSON array `TEXT` via `serde_json`.

use crate::db::schema::{search_document, search_index_meta, search_posting};
use crate::search::constants::CURRENT_INDEX_VERSION;
use anyhow::Result;
use diesel::prelude::*;
use uc_core::search::document::{ContentType, SearchDocument, SearchIndexMeta, SearchPosting};

// ──────────────────────────────────────────────
// search_document
// ──────────────────────────────────────────────

/// Full queryable row for `search_document`.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = search_document)]
#[diesel(primary_key(profile_id, entry_id))]
pub struct SearchDocumentRow {
    pub profile_id: String,
    pub entry_id: String,
    pub event_id: String,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    /// Stored as snake_case string (e.g. "text", "html").
    pub file_type: String,
    /// Stored as a JSON array of strings (e.g. `["txt","md"]`).
    pub file_extensions: String,
    pub mime_type: String,
    pub indexed_at_ms: i64,
    pub index_version: String,
    pub text_preview: Option<String>,
}

/// Insertable row for `search_document`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = search_document)]
pub struct NewSearchDocumentRow {
    pub profile_id: String,
    pub entry_id: String,
    pub event_id: String,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub file_type: String,
    pub file_extensions: String,
    pub mime_type: String,
    pub indexed_at_ms: i64,
    pub index_version: String,
    pub text_preview: Option<String>,
}

impl NewSearchDocumentRow {
    /// Convert a domain `SearchDocument` into an insertable row, binding it to `profile_id`.
    ///
    /// `file_type` is serialized via serde to produce the stable snake_case string.
    /// `file_extensions` is serialized as a JSON array.
    pub fn from_domain(profile_id: &str, document: &SearchDocument) -> Result<Self> {
        // serde_json::to_string produces `"text"` with surrounding quotes; trim them.
        let content_type_json = serde_json::to_string(&document.content_type)?;
        let file_type = content_type_json.trim_matches('"').to_string();
        let file_extensions = serde_json::to_string(&document.file_extensions)?;

        Ok(Self {
            profile_id: profile_id.to_string(),
            entry_id: document.entry_id.to_string(),
            event_id: document.event_id.to_string(),
            active_time_ms: document.active_time_ms,
            captured_at_ms: document.captured_at_ms,
            file_type,
            file_extensions,
            mime_type: document.mime_type.clone(),
            indexed_at_ms: document.indexed_at_ms,
            index_version: document.index_version.clone(),
            text_preview: document.text_preview.clone(),
        })
    }
}

impl SearchDocumentRow {
    /// Convert a stored row back into a domain `SearchDocument`.
    ///
    /// `file_type` is deserialized from the snake_case string.
    /// `file_extensions` is deserialized from the JSON array.
    pub fn to_domain(&self) -> Result<SearchDocument> {
        // Re-add surrounding quotes so serde_json can deserialize the string enum.
        let content_type_json = format!("\"{}\"", self.file_type);
        let content_type: ContentType = serde_json::from_str(&content_type_json)?;
        let file_extensions: Vec<String> = serde_json::from_str(&self.file_extensions)?;

        Ok(SearchDocument {
            entry_id: self.entry_id.clone().into(),
            event_id: self.event_id.clone().into(),
            active_time_ms: self.active_time_ms,
            captured_at_ms: self.captured_at_ms,
            content_type,
            file_extensions,
            mime_type: self.mime_type.clone(),
            indexed_at_ms: self.indexed_at_ms,
            index_version: self.index_version.clone(),
            text_preview: self.text_preview.clone(),
        })
    }
}

// ──────────────────────────────────────────────
// search_posting
// ──────────────────────────────────────────────

/// Full queryable row for `search_posting`.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = search_posting)]
#[diesel(primary_key(profile_id, term_tag, entry_id))]
pub struct SearchPostingRow {
    pub profile_id: String,
    /// 32-byte HMAC-SHA256 tag (stored as BLOB, mapped to Vec<u8>).
    pub term_tag: Vec<u8>,
    pub entry_id: String,
    /// Stored as `i32`; domain uses `u8`. Cast on conversion.
    pub field_mask: i32,
    /// Stored as `i32`; domain uses `u32`. Cast on conversion.
    /// CHECK (term_freq > 0) guarantees a safe upcast.
    pub term_freq: i32,
}

/// Insertable row for `search_posting`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = search_posting)]
pub struct NewSearchPostingRow {
    pub profile_id: String,
    pub term_tag: Vec<u8>,
    pub entry_id: String,
    pub field_mask: i32,
    pub term_freq: i32,
}

impl NewSearchPostingRow {
    /// Convert a domain `SearchPosting` into an insertable row, binding it to `profile_id`.
    pub fn from_domain(profile_id: &str, posting: &SearchPosting) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            term_tag: posting.term_tag.clone(),
            entry_id: posting.entry_id.to_string(),
            field_mask: posting.field_mask as i32,
            term_freq: posting.term_freq as i32,
        }
    }
}

// ──────────────────────────────────────────────
// search_index_meta
// ──────────────────────────────────────────────

/// Full queryable row for `search_index_meta`.
#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Identifiable)]
#[diesel(table_name = search_index_meta)]
#[diesel(primary_key(profile_id))]
pub struct SearchIndexMetaRow {
    pub profile_id: String,
    pub index_version: String,
    pub search_blocked: bool,
    pub last_rebuild_started_at_ms: Option<i64>,
    pub last_rebuild_completed_at_ms: Option<i64>,
}

/// Insertable row for `search_index_meta`.
#[derive(Debug, Clone, Insertable)]
#[diesel(table_name = search_index_meta)]
pub struct NewSearchIndexMetaRow {
    pub profile_id: String,
    pub index_version: String,
    pub search_blocked: bool,
    pub last_rebuild_started_at_ms: Option<i64>,
    pub last_rebuild_completed_at_ms: Option<i64>,
}

impl SearchIndexMetaRow {
    /// Convert a stored row into the domain `SearchIndexMeta` view.
    ///
    /// Note: `profile_id` is a persistence concern and is not exposed in the domain type.
    pub fn to_domain(&self) -> SearchIndexMeta {
        SearchIndexMeta {
            index_version: self.index_version.clone(),
            search_blocked: self.search_blocked,
            last_rebuild_started_at_ms: self.last_rebuild_started_at_ms,
            last_rebuild_completed_at_ms: self.last_rebuild_completed_at_ms,
        }
    }
}

impl NewSearchIndexMetaRow {
    /// Seed a fresh meta row for `profile_id` using the current index version.
    ///
    /// `search_blocked = false` and timestamps are `None` for a brand-new profile.
    pub fn seed(profile_id: &str) -> Self {
        Self {
            profile_id: profile_id.to_string(),
            index_version: CURRENT_INDEX_VERSION.to_string(),
            search_blocked: false,
            last_rebuild_started_at_ms: None,
            last_rebuild_completed_at_ms: None,
        }
    }
}

// ──────────────────────────────────────────────
// Row-level unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::{EntryId, EventId};
    use uc_core::search::document::{ContentType, SearchDocument, SearchPosting};

    fn sample_document() -> SearchDocument {
        SearchDocument {
            entry_id: EntryId::from("entry-01"),
            event_id: EventId::from("event-01"),
            active_time_ms: 1_000_000,
            captured_at_ms: 999_000,
            content_type: ContentType::Text,
            file_extensions: vec!["txt".to_string(), "md".to_string()],
            mime_type: "text/plain".to_string(),
            indexed_at_ms: 1_100_000,
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: Some("Hello world".to_string()),
        }
    }

    #[test]
    fn document_row_round_trips_file_extensions() {
        let doc = sample_document();
        let row = NewSearchDocumentRow::from_domain("profile-abc", &doc).expect("from_domain");

        // Simulate what we'd get back from the DB by building a SearchDocumentRow.
        let queried = SearchDocumentRow {
            profile_id: row.profile_id.clone(),
            entry_id: row.entry_id.clone(),
            event_id: row.event_id.clone(),
            active_time_ms: row.active_time_ms,
            captured_at_ms: row.captured_at_ms,
            file_type: row.file_type.clone(),
            file_extensions: row.file_extensions.clone(),
            mime_type: row.mime_type.clone(),
            indexed_at_ms: row.indexed_at_ms,
            index_version: row.index_version.clone(),
            text_preview: row.text_preview.clone(),
        };

        let restored = queried.to_domain().expect("to_domain");
        assert_eq!(restored.file_extensions, doc.file_extensions);
        assert_eq!(restored.entry_id, doc.entry_id);
    }

    #[test]
    fn document_row_content_type_round_trips_all_variants() {
        let variants = [
            ContentType::Text,
            ContentType::Html,
            ContentType::Link,
            ContentType::File,
            ContentType::Image,
            ContentType::Other,
        ];
        for ft in variants {
            let doc = SearchDocument {
                content_type: ft.clone(),
                file_extensions: vec![],
                ..sample_document()
            };
            let row = NewSearchDocumentRow::from_domain("p", &doc).expect("from_domain");
            let queried = SearchDocumentRow {
                profile_id: row.profile_id.clone(),
                entry_id: row.entry_id.clone(),
                event_id: row.event_id.clone(),
                active_time_ms: row.active_time_ms,
                captured_at_ms: row.captured_at_ms,
                file_type: row.file_type.clone(),
                file_extensions: row.file_extensions.clone(),
                mime_type: row.mime_type.clone(),
                indexed_at_ms: row.indexed_at_ms,
                index_version: row.index_version.clone(),
                text_preview: row.text_preview.clone(),
            };
            let restored = queried.to_domain().expect("to_domain");
            assert_eq!(
                restored.content_type, ft,
                "content_type round-trip failed for {ft:?}"
            );
        }
    }

    #[test]
    fn document_row_from_domain_does_not_require_profile_id_on_search_document() {
        // SearchDocument must NOT have a profile_id field.
        // This test passes at compile time — if profile_id were added to SearchDocument,
        // the field would appear here, and the from_domain signature would change.
        let doc = sample_document();
        let result = NewSearchDocumentRow::from_domain("profile-xyz", &doc);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().profile_id, "profile-xyz");
    }

    #[test]
    fn posting_row_from_domain_does_not_require_profile_id_on_search_posting() {
        let posting = SearchPosting {
            term_tag: vec![0xABu8; 32],
            entry_id: EntryId::from("entry-02"),
            field_mask: 0b0000_0001,
            term_freq: 3,
        };
        let row = NewSearchPostingRow::from_domain("profile-abc", &posting);
        assert_eq!(row.profile_id, "profile-abc");
        assert_eq!(row.term_tag, posting.term_tag);
        assert_eq!(row.field_mask, 1i32);
        assert_eq!(row.term_freq, 3i32);
    }

    #[test]
    fn posting_row_casts_field_mask_and_term_freq_correctly() {
        let posting = SearchPosting {
            term_tag: vec![0x01u8; 32],
            entry_id: EntryId::from("entry-03"),
            field_mask: 0b0001_1111, // all 5 fields
            term_freq: 255,
        };
        let row = NewSearchPostingRow::from_domain("p", &posting);
        assert_eq!(row.field_mask, 0b0001_1111i32);
        assert_eq!(row.term_freq, 255i32);
    }

    #[test]
    fn meta_row_to_domain_maps_index_version_and_timestamps() {
        let row = SearchIndexMetaRow {
            profile_id: "profile-abc".to_string(),
            index_version: "search-v1".to_string(),
            search_blocked: false,
            last_rebuild_started_at_ms: Some(1_000),
            last_rebuild_completed_at_ms: Some(2_000),
        };
        let meta = row.to_domain();
        assert_eq!(meta.index_version, "search-v1");
        assert_eq!(meta.search_blocked, false);
        assert_eq!(meta.last_rebuild_started_at_ms, Some(1_000));
        assert_eq!(meta.last_rebuild_completed_at_ms, Some(2_000));
    }

    #[test]
    fn meta_row_seed_uses_current_index_version_and_defaults() {
        let row = NewSearchIndexMetaRow::seed("profile-fresh");
        assert_eq!(row.profile_id, "profile-fresh");
        assert_eq!(row.index_version, CURRENT_INDEX_VERSION);
        assert_eq!(row.search_blocked, false);
        assert!(row.last_rebuild_started_at_ms.is_none());
        assert!(row.last_rebuild_completed_at_ms.is_none());
    }

    #[test]
    fn meta_row_to_domain_with_none_timestamps() {
        let row = SearchIndexMetaRow {
            profile_id: "p".to_string(),
            index_version: CURRENT_INDEX_VERSION.to_string(),
            search_blocked: true,
            last_rebuild_started_at_ms: None,
            last_rebuild_completed_at_ms: None,
        };
        let meta = row.to_domain();
        assert!(meta.last_rebuild_started_at_ms.is_none());
        assert!(meta.last_rebuild_completed_at_ms.is_none());
        assert!(meta.search_blocked);
    }
}
