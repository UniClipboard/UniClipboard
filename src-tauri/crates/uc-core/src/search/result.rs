//! SearchResult and RebuildProgress — output types for SearchIndexPort.

use crate::ids::EntryId;
use crate::search::document::ContentType;
use serde::{Deserialize, Serialize};

/// Single search result row — carries the full metadata needed to render
/// a ClipboardItemRow in the UI without a second API call (per D-01).
///
/// Fields are exactly those specified in D-01:
/// entry_id, content_type, active_time_ms, text_preview, mime_type, file_extensions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResult {
    pub entry_id: EntryId,
    pub content_type: ContentType,
    pub active_time_ms: i64,
    /// Truncated preview (~80 chars) — truncation logic lives in Phase 89 use case, not here.
    pub text_preview: Option<String>,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
}

/// Paged output from `SearchIndexPort::search()`.
///
/// Carries all pagination metadata so the route layer does not need a
/// separate count query or fake `has_more` inference (per D-02, Phase 92).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultsPage {
    /// Matching entries for the requested page window.
    pub items: Vec<SearchResult>,
    /// Total count of matching entries across all pages (computed before pagination).
    pub total: u32,
    /// Whether more pages follow the current page window.
    pub has_more: bool,
}

/// Stage of a full index rebuild.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RebuildStage {
    Started,
    Indexing,
    Complete,
    Failed,
}

/// Progress update emitted by `SearchIndexPort::rebuild()` through an mpsc channel.
///
/// The daemon subscribes to the channel and forwards events over WebSocket —
/// uc-core has no WebSocket knowledge (D-07). This mirrors the TransferProgress
/// pattern already in the codebase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildProgress {
    pub stage: RebuildStage,
    /// Number of entries indexed so far.
    pub indexed: u32,
    /// Total entries to index (0 if unknown at start).
    pub total: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_result_serde_round_trip() {
        let r = SearchResult {
            entry_id: EntryId::from("entry-abc"),
            content_type: ContentType::Text,
            active_time_ms: 123_456,
            text_preview: Some("hello".into()),
            mime_type: "text/plain".into(),
            file_extensions: vec!["txt".into()],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn rebuild_progress_serde_round_trip() {
        let p = RebuildProgress {
            stage: RebuildStage::Indexing,
            indexed: 5,
            total: 100,
        };
        let json = serde_json::to_value(&p).unwrap();
        assert_eq!(json["stage"], "indexing");
        assert_eq!(json["indexed"], 5);
        assert_eq!(json["total"], 100);
        let back: RebuildProgress = serde_json::from_value(json).unwrap();
        assert_eq!(p, back);
    }

    #[test]
    fn rebuild_stage_all_variants_serde() {
        let stages = [
            RebuildStage::Started,
            RebuildStage::Indexing,
            RebuildStage::Complete,
            RebuildStage::Failed,
        ];
        for stage in stages {
            let json = serde_json::to_string(&stage).unwrap();
            let back: RebuildStage = serde_json::from_str(&json).unwrap();
            assert_eq!(stage, back);
        }
    }

    #[test]
    fn search_results_page_serde_round_trip() {
        let item = SearchResult {
            entry_id: EntryId::from("entry-page-1"),
            content_type: ContentType::Text,
            active_time_ms: 999,
            text_preview: Some("page item".into()),
            mime_type: "text/plain".into(),
            file_extensions: vec!["txt".into()],
        };
        let page = SearchResultsPage {
            items: vec![item.clone()],
            total: 42,
            has_more: true,
        };
        let json = serde_json::to_string(&page).unwrap();
        let back: SearchResultsPage = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total, 42);
        assert!(back.has_more);
        assert_eq!(back.items.len(), 1);
        assert_eq!(back.items[0], item);

        // has_more = false also round-trips
        let page2 = SearchResultsPage {
            items: vec![],
            total: 0,
            has_more: false,
        };
        let json2 = serde_json::to_string(&page2).unwrap();
        let back2: SearchResultsPage = serde_json::from_str(&json2).unwrap();
        assert!(!back2.has_more);
        assert_eq!(back2.total, 0);
    }

    #[test]
    fn search_result_no_text_preview() {
        let r = SearchResult {
            entry_id: EntryId::from("entry-xyz"),
            content_type: ContentType::Image,
            active_time_ms: 0,
            text_preview: None,
            mime_type: "image/png".into(),
            file_extensions: vec!["png".into()],
        };
        let json = serde_json::to_string(&r).unwrap();
        let back: SearchResult = serde_json::from_str(&json).unwrap();
        assert_eq!(r, back);
        assert!(back.text_preview.is_none());
    }
}
