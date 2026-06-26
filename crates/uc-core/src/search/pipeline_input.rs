//! Input snapshot consumed by the search pipeline.
//!
//! Carries everything the search pipeline needs to derive a `SearchDocument`
//! and a set of `SearchPosting` rows from a single clipboard entry.
//!
//! Lives in `uc-core` because all fields are domain-typed and the struct is
//! a public boundary between the application layer (which builds it from
//! representations) and the search pipeline port (which consumes it).

use crate::ids::{EntryId, EventId};
use crate::search::document::ContentType;
use crate::search::tag::TagId;

#[derive(Debug, Clone)]
pub struct SearchPipelineInput {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub content_type: ContentType,
    /// Derived tags (e.g. `link`) produced by evaluating tag rules over the
    /// entry's content. Orthogonal to `content_type`; zero or more per entry.
    pub tags: Vec<TagId>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    pub plain_text: Option<String>,
    pub html_text: Option<String>,
    pub uri_list: Vec<String>,
    pub file_paths: Vec<String>,
    pub file_names: Vec<String>,
    pub text_preview: Option<String>,
    /// Web URLs (http/https) to mirror as the `link_urls` render column. Shares
    /// the [`crate::clipboard::link_utils::detect_link_urls`] contract with the
    /// `link` tag rule so render and filter stay in lock-step. Empty when none.
    pub link_urls: Vec<String>,
    /// Originating device id resolved from the clipboard event, or `None` when
    /// the source is unknown/untrusted. Resolved by the caller (async) before
    /// building this input, since the projection builder itself is synchronous.
    pub source_device: Option<String>,
    /// `Some("Lost")` when the paste representation is permanently lost,
    /// `None` otherwise. Derived from the paste representation's availability.
    pub payload_state: Option<String>,
}
