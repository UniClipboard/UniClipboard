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

use crate::db::schema::{search_document, search_entry_tag, search_index_meta, search_posting};
use crate::search::constants::CURRENT_INDEX_VERSION;
use crate::search::render_payload::{RenderFields, RenderPayloadCodec};
use anyhow::Result;
use diesel::prelude::*;
use tracing::warn;
use uc_core::ids::EntryId;
use uc_core::search::document::{ContentType, SearchDocument, SearchIndexMeta, SearchPosting};

// ──────────────────────────────────────────────
// search_document
// ──────────────────────────────────────────────

/// Full queryable row for `search_document`.
///
/// The five content-derived render fields (`text_preview`, `file_names`,
/// `link_urls`, `file_paths`, `char_count`) are no longer stored as plaintext
/// columns; they are sealed together into `render_payload` (a UCSR AEAD
/// envelope). `file_type`, `file_extensions`, `mime_type`, `source_device` and
/// `payload_state` remain plaintext (SQL-filtered metadata / availability flag).
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
    /// Originating device id, or `NULL` when the source is unknown.
    pub source_device: Option<String>,
    /// `"Lost"` when the paste payload is unrecoverable, else `NULL`.
    pub payload_state: Option<String>,
    /// AEAD-sealed UCSR envelope carrying the content-derived render fields.
    /// `NULL` only appears transiently during migration; a `NULL` on the current
    /// index version is treated as a corrupted row (see [`SearchDocumentRow::to_domain`]).
    pub render_payload: Option<Vec<u8>>,
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
    pub source_device: Option<String>,
    pub payload_state: Option<String>,
    pub render_payload: Option<Vec<u8>>,
}

/// A stored row decoded back into the domain, tracking whether its render payload
/// failed to decode (in which case the render fields are blanked).
pub struct DecodedDocument {
    pub document: SearchDocument,
    /// True when `render_payload` could not be decoded and the render fields were
    /// blanked; the caller schedules a re-projection repair for this entry.
    pub render_corrupted: bool,
}

impl NewSearchDocumentRow {
    /// Convert a domain `SearchDocument` into an insertable row, binding it to
    /// `profile_id` and sealing the render fields with `codec`.
    ///
    /// `file_type` is serialized via serde to produce the stable snake_case string.
    /// `file_extensions` is serialized as a JSON array. The render fields are
    /// packed and encrypted into `render_payload`.
    pub fn from_domain(
        codec: &RenderPayloadCodec,
        profile_id: &str,
        document: &SearchDocument,
    ) -> Result<Self> {
        // serde_json::to_string produces `"text"` with surrounding quotes; trim them.
        let content_type_json = serde_json::to_string(&document.content_type)?;
        let file_type = content_type_json.trim_matches('"').to_string();
        let file_extensions = serde_json::to_string(&document.file_extensions)?;

        let fields = RenderFields::new(
            document.text_preview.clone(),
            document.file_names.clone(),
            document.link_urls.clone(),
            document.file_paths.clone(),
            document.char_count,
        );
        let render_payload = codec
            .encrypt(&document.entry_id, &fields)
            .map_err(|e| anyhow::anyhow!("encrypt render payload: {e}"))?;

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
            source_device: document.source_device.clone(),
            payload_state: document.payload_state.clone(),
            render_payload: Some(render_payload),
        })
    }
}

impl SearchDocumentRow {
    /// Convert a stored row back into a domain `SearchDocument`, decrypting the
    /// render payload with `codec`.
    ///
    /// Structural columns (`file_type`, `file_extensions`) parse eagerly and a
    /// failure there is a hard error. A `render_payload` that is `NULL` or fails
    /// to decode does NOT fail the whole row: the render fields are blanked and
    /// `render_corrupted` is set so the caller can schedule a repair.
    pub fn to_domain(&self, codec: &RenderPayloadCodec) -> Result<DecodedDocument> {
        // Re-add surrounding quotes so serde_json can deserialize the string enum.
        let content_type_json = format!("\"{}\"", self.file_type);
        let content_type: ContentType = serde_json::from_str(&content_type_json)?;
        let file_extensions: Vec<String> = serde_json::from_str(&self.file_extensions)?;

        let entry_id: EntryId = self.entry_id.clone().into();
        let (fields, render_corrupted) = match &self.render_payload {
            Some(bytes) => match codec.decrypt(&entry_id, bytes) {
                Ok(fields) => (fields, false),
                Err(err) => {
                    warn!(
                        entry_id = %self.entry_id,
                        error = %err,
                        "search: render payload decode failed, blanking render fields"
                    );
                    (RenderFields::default(), true)
                }
            },
            None => {
                warn!(
                    entry_id = %self.entry_id,
                    "search: render payload is NULL on current index version, blanking render fields"
                );
                (RenderFields::default(), true)
            }
        };

        let document = SearchDocument {
            entry_id,
            event_id: self.event_id.clone().into(),
            active_time_ms: self.active_time_ms,
            captured_at_ms: self.captured_at_ms,
            content_type,
            // Tag membership lives in `search_entry_tag`, not on the document row.
            // The read side hydrates it via a separate query when the search/filter
            // path begins consuming tags; document-only reads carry an empty set.
            tags: Vec::new(),
            file_extensions,
            mime_type: self.mime_type.clone(),
            indexed_at_ms: self.indexed_at_ms,
            index_version: self.index_version.clone(),
            text_preview: fields.text_preview,
            file_names: fields.file_names,
            file_paths: fields.file_paths,
            link_urls: fields.link_urls,
            source_device: self.source_device.clone(),
            payload_state: self.payload_state.clone(),
            char_count: fields.char_count,
        };

        Ok(DecodedDocument {
            document,
            render_corrupted,
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
    pub plaintext_purge_done_ms: Option<i64>,
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
    pub plaintext_purge_done_ms: Option<i64>,
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
            plaintext_purge_done_ms: self.plaintext_purge_done_ms,
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
            plaintext_purge_done_ms: None,
        }
    }
}

// ──────────────────────────────────────────────
// search_entry_tag
// ──────────────────────────────────────────────

/// Insertable row for `search_entry_tag` — one membership row per
/// `(entry_id, tag_id)`.
///
/// Pure derived data: rebuilt from the entry's content rules and user-state, so
/// the row carries nothing beyond the identity triple. `tag_id` stores the
/// `TagId` as its transparent string form.
#[derive(Debug, Clone, PartialEq, Eq, Insertable)]
#[diesel(table_name = search_entry_tag)]
pub struct NewSearchEntryTagRow {
    pub profile_id: String,
    pub entry_id: String,
    pub tag_id: String,
}

impl NewSearchEntryTagRow {
    /// Build membership rows for all of `document`'s tags, bound to `profile_id`.
    ///
    /// Returns an empty `Vec` when the document carries no tags.
    pub fn rows_for_document(profile_id: &str, document: &SearchDocument) -> Vec<Self> {
        document
            .tags
            .iter()
            .map(|tag| Self {
                profile_id: profile_id.to_string(),
                entry_id: document.entry_id.to_string(),
                tag_id: tag.as_str().to_string(),
            })
            .collect()
    }
}

// ──────────────────────────────────────────────
// Row-level unit tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::search::document::ContentType;
    use uc_core::search::tag::TagId;

    fn doc_with_tags(entry_id: &str, tags: Vec<TagId>) -> SearchDocument {
        SearchDocument {
            entry_id: entry_id.into(),
            event_id: "ev".into(),
            active_time_ms: 0,
            captured_at_ms: 0,
            content_type: ContentType::Text,
            tags,
            file_extensions: vec![],
            mime_type: "text/plain".into(),
            indexed_at_ms: 0,
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: None,
            file_names: vec![],
            file_paths: vec![],
            link_urls: vec![],
            source_device: None,
            payload_state: None,
            char_count: None,
        }
    }

    #[test]
    fn entry_tag_rows_map_each_tag_to_a_row() {
        let doc = doc_with_tags("e1", vec![TagId::link(), TagId::favorited()]);
        let rows = NewSearchEntryTagRow::rows_for_document("p1", &doc);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].profile_id, "p1");
        assert_eq!(rows[0].entry_id, "e1");
        assert_eq!(rows[0].tag_id, "link");
        assert_eq!(rows[1].tag_id, "favorited");
    }

    #[test]
    fn entry_tag_rows_empty_when_document_has_no_tags() {
        let doc = doc_with_tags("e1", vec![]);
        assert!(NewSearchEntryTagRow::rows_for_document("p1", &doc).is_empty());
    }
}
