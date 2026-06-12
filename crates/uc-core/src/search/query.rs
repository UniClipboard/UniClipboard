//! SearchQuery domain model — structured input for `SearchIndexPort::search()`.

use crate::search::document::ContentType;
use serde::{Deserialize, Serialize};

/// Top-level boolean operator joining tokenized query terms.
///
/// Per D-10, mixing AND and OR in one query is an error (InvalidQuery).
/// Mixed operator detection and validation is a Phase 89 use-case concern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueryOperator {
    And,
    Or,
}

/// Time range filter — either a preset window or an absolute millisecond range.
///
/// Preset variants are resolved to absolute timestamps by the use case / daemon
/// at query execution time (Phase 89/92); uc-core carries the enum opaquely.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimeRangeFilter {
    Today,
    Yesterday,
    Last24h,
    Last7d,
    Last30d,
    ThisWeek,
    ThisMonth,
    Absolute { from_ms: u64, to_ms: u64 },
}

/// Structured search query — mirrors the daemon HTTP request body shape.
///
/// Field ordering follows D-10 exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchQuery {
    /// Free-text query string. Tokenization and HMAC derivation happen in Phase 90 infra.
    pub query_string: String,
    /// Boolean operator for all terms. AND/OR mixing is rejected at parse time.
    pub operator: QueryOperator,
    /// Optional time range filter. None means no time restriction.
    pub time_range: Option<TimeRangeFilter>,
    /// Multi-select file type filter. Empty slice means no type restriction.
    pub content_types: Vec<ContentType>,
    /// File extension filter (e.g. `["md", "txt"]`). Empty means no restriction.
    pub extensions: Vec<String>,
    /// Maximum number of results to return.
    pub limit: u32,
    /// Offset for pagination.
    pub offset: u32,
}
