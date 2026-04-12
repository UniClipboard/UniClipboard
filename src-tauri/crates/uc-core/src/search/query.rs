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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_serde_round_trip() {
        let q = SearchQuery {
            query_string: "hello world".into(),
            operator: QueryOperator::Or,
            time_range: Some(TimeRangeFilter::Last7d),
            content_types: vec![ContentType::Text, ContentType::Html],
            extensions: vec!["pdf".into()],
            limit: 20,
            offset: 0,
        };
        let json = serde_json::to_string(&q).unwrap();
        let back: SearchQuery = serde_json::from_str(&json).unwrap();
        assert_eq!(q, back);
    }

    #[test]
    fn time_range_absolute_serde() {
        let r = TimeRangeFilter::Absolute {
            from_ms: 1_000,
            to_ms: 2_000,
        };
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("\"from_ms\":1000"));
        assert!(json.contains("\"to_ms\":2000"));
        let back: TimeRangeFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn query_operator_lowercase() {
        let s = serde_json::to_string(&QueryOperator::And).unwrap();
        assert_eq!(s, "\"and\"");
        let s = serde_json::to_string(&QueryOperator::Or).unwrap();
        assert_eq!(s, "\"or\"");
    }

    #[test]
    fn time_range_preset_variants_round_trip() {
        let presets = [
            TimeRangeFilter::Today,
            TimeRangeFilter::Yesterday,
            TimeRangeFilter::Last24h,
            TimeRangeFilter::Last7d,
            TimeRangeFilter::Last30d,
            TimeRangeFilter::ThisWeek,
            TimeRangeFilter::ThisMonth,
        ];
        for preset in presets {
            let json = serde_json::to_string(&preset).unwrap();
            let back: TimeRangeFilter = serde_json::from_str(&json).unwrap();
            assert_eq!(preset, back);
        }
    }
}
