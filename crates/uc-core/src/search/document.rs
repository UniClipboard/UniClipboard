//! Search document and posting row types — mirrors the SQLite schema owned by Phase 90.
//!
//! Hard-delete semantic: when an entry is deleted, the document and all its
//! postings are removed from the index entirely. No soft-delete timestamp field.

use crate::ids::{EntryId, EventId};
use crate::search::tag::TagId;
use serde::{Deserialize, Serialize};

/// Physical content-type classification used for search filtering — the
/// single-valued "what data form is this?" dimension.
///
/// Exactly one value per entry. Orthogonal to the multi-valued tag dimension
/// (see [`crate::search::tag`]): a web URL is physically `Text` and additionally
/// carries the derived `link` tag, rather than being its own content type.
/// `html` is the `text/html` rich-text MIME — an independent data form, not a
/// tag. `Other` is an internal-only fallback and is not exposed as a filter.
///
/// Maps to stable backend enum values; frontend localizes display text independently.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ContentType {
    Text,
    Image,
    File,
    Html,
    Other,
}

/// One row per indexable clipboard entry.
///
/// Hard-delete semantic enforced by design: there is no soft-delete timestamp.
/// When a clipboard entry is deleted, the document row is removed entirely.
/// `index_version` allows safe schema migration and rebuild triggering.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchDocument {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub content_type: ContentType,
    /// Derived tags (e.g. the builtin `link` rule) and mirrored user-state tags
    /// (e.g. `favorited`) attached to this entry. Orthogonal to `content_type`:
    /// zero or more per entry. Membership is persisted in a dedicated table, not
    /// as a document column, so it can carry many ids per entry.
    pub tags: Vec<TagId>,
    pub file_extensions: Vec<String>,
    pub mime_type: String,
    pub indexed_at_ms: i64,
    /// Normalization and tokenizer schema version.
    /// A mismatch triggers a full index rebuild in Phase 91.
    pub index_version: String,
    /// Optional truncated preview for UI rendering (populated by Phase 89 use case).
    /// Truncation logic lives in the use case, not here.
    pub text_preview: Option<String>,
    /// Full character count of the entry's primary text content, mirrored as a
    /// render column so the UI can show the real total length next to the
    /// truncated `text_preview`. `None` for entries with no inline text (image /
    /// file / payload not inline). Capture-time stable.
    pub char_count: Option<i64>,
    /// Display names of the files this entry references (from a `file://`
    /// uri-list). Empty when the entry carries no files. Capture-time stable, so
    /// it is mirrored as an index render column rather than fetched lazily.
    pub file_names: Vec<String>,
    /// Web URLs (http/https) carried by this entry, sharing the detection
    /// contract with the `link` tag so render and filter stay consistent. Empty
    /// when none. Capture-time stable.
    pub link_urls: Vec<String>,
    /// Originating device id, resolved from the clipboard event. `None` when the
    /// event is unknown (source untrusted). Used for render and match decisions.
    pub source_device: Option<String>,
    /// Marker for an unrecoverable paste payload: `Some("Lost")` when the paste
    /// representation is permanently lost, `None` otherwise. Live indexing writes
    /// the healthy default; rebuild backfills the authoritative value.
    pub payload_state: Option<String>,
}

/// One row per `(term_tag, entry_id)` pair in the inverted index.
///
/// `term_tag` is `HMAC-SHA256(search_key, normalized_token)` — 32 bytes.
/// Never stores plaintext tokens; the HMAC is computed in Phase 90 infra.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPosting {
    /// HMAC-SHA256 output over the normalized token. 32 bytes.
    pub term_tag: Vec<u8>,
    pub entry_id: EntryId,
    /// Bitmask of source fields the term was extracted from:
    /// body = 1, html = 2, url = 4, file_path = 8, file_name = 16.
    pub field_mask: u8,
    /// Number of times this term appears in the document.
    pub term_freq: u32,
}

/// Read-only projection of the `search_index_meta` row.
///
/// Exposed via `SearchIndexPort::get_index_meta()`.
/// Infrastructure owns storage; uc-core only sees this view.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchIndexMeta {
    /// Current normalization/tokenizer schema version.
    pub index_version: String,
    /// True if the index is blocked (e.g. rebuild in progress with version swap).
    pub search_blocked: bool,
    /// Millisecond timestamp of the last rebuild start, or None if never rebuilt.
    pub last_rebuild_started_at_ms: Option<i64>,
    /// Millisecond timestamp of the last completed rebuild, or None if never completed.
    pub last_rebuild_completed_at_ms: Option<i64>,
}
