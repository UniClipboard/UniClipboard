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

#[derive(Debug, Clone)]
pub struct SearchPipelineInput {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub content_type: ContentType,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    pub plain_text: Option<String>,
    pub html_text: Option<String>,
    pub uri_list: Vec<String>,
    pub file_paths: Vec<String>,
    pub file_names: Vec<String>,
    pub text_preview: Option<String>,
}
