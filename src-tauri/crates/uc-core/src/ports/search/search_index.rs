//! SearchIndexPort — async trait implemented by uc-infra (Phase 91).
//!
//! All methods return Result<_, SearchError> to preserve typed error info
//! across the port boundary (per D-03, D-04, D-05). Infra adapters may use
//! anyhow::Error internally but MUST map to SearchError at method return.

use crate::ids::EntryId;
use crate::search::{
    RebuildProgress, SearchDocument, SearchError, SearchIndexMeta, SearchPosting, SearchQuery,
    SearchResultsPage,
};
use async_trait::async_trait;
use tokio::sync::mpsc::Sender;

/// Port for indexing and querying the local encrypted search index.
///
/// Implemented by uc-infra (Phase 91). Injected as `Arc<dyn SearchIndexPort + Send + Sync>`
/// into use cases and daemon state.
#[async_trait]
pub trait SearchIndexPort: Send + Sync {
    /// Index (insert or replace) a clipboard entry's document and its postings.
    ///
    /// Called by IndexClipboardEntry use case (Phase 89) after capture and persistence.
    /// If a document for `entry_id` already exists, it is replaced atomically.
    async fn index_entry(
        &self,
        document: SearchDocument,
        postings: Vec<SearchPosting>,
    ) -> Result<(), SearchError>;

    /// Remove a clipboard entry from the search index (document + all postings).
    ///
    /// Called synchronously by DeleteClipboardEntry (Phase 89) — hard-delete.
    async fn remove_entry(&self, entry_id: &EntryId) -> Result<(), SearchError>;

    /// Execute a structured query and return a paged result with full render metadata.
    ///
    /// Returns `SearchResultsPage` (not `Vec<EntryId>`) per D-01 / D-02 — avoids a second
    /// query in the route layer to hydrate UI row metadata or compute pagination truth.
    async fn search(&self, query: SearchQuery) -> Result<SearchResultsPage, SearchError>;

    /// Full index rebuild from a supplied entry list.
    ///
    /// Uses a channel to emit `RebuildProgress` so the daemon can forward events
    /// over WebSocket without uc-core knowing about WS (D-07).
    /// Phase 91 implements version-flag atomic swap inside.
    async fn rebuild(
        &self,
        entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
        progress_tx: Sender<RebuildProgress>,
    ) -> Result<(), SearchError>;

    /// Read-only projection of search_index_meta (index_version, search_blocked, timestamps).
    async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::{RebuildStage, SearchIndexMeta, SearchResult};
    use std::sync::Arc;

    struct StubPort;

    #[async_trait]
    impl SearchIndexPort for StubPort {
        async fn index_entry(
            &self,
            _d: SearchDocument,
            _p: Vec<SearchPosting>,
        ) -> Result<(), SearchError> {
            Ok(())
        }
        async fn remove_entry(&self, _id: &EntryId) -> Result<(), SearchError> {
            Ok(())
        }
        async fn search(&self, _q: SearchQuery) -> Result<SearchResultsPage, SearchError> {
            Ok(SearchResultsPage {
                items: vec![],
                total: 0,
                has_more: false,
            })
        }
        async fn rebuild(
            &self,
            _e: Vec<(SearchDocument, Vec<SearchPosting>)>,
            _tx: Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }
        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            Ok(SearchIndexMeta {
                index_version: "v1".into(),
                search_blocked: false,
                last_rebuild_started_at_ms: None,
                last_rebuild_completed_at_ms: None,
            })
        }
    }

    #[test]
    fn search_index_port_is_object_safe() {
        let _port: Arc<dyn SearchIndexPort + Send + Sync> = Arc::new(StubPort);
    }

    #[tokio::test]
    async fn search_index_port_rebuild_accepts_mpsc_sender() {
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(8);
        let port: Arc<dyn SearchIndexPort + Send + Sync> = Arc::new(StubPort);
        let result = port.rebuild(vec![], tx).await;
        assert!(result.is_ok());
        // Reference RebuildStage to keep the import live in test scope.
        let _ = RebuildStage::Started;
    }

    #[tokio::test]
    async fn search_index_port_search_returns_results_not_entry_ids() {
        let port: Arc<dyn SearchIndexPort + Send + Sync> = Arc::new(StubPort);
        use crate::search::{QueryOperator, SearchQuery};
        let q = SearchQuery {
            query_string: "test".into(),
            operator: QueryOperator::And,
            time_range: None,
            content_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        };
        let page: SearchResultsPage = port.search(q).await.unwrap();
        assert!(page.items.is_empty());
        assert_eq!(page.total, 0);
        assert!(!page.has_more);
        // SearchResult import kept alive via _ usage
        let _: Option<SearchResult> = None;
    }
}
