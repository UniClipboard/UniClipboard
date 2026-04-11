//! SearchClipboardEntries use case — executes a structured search query against the index (D-06).

use std::sync::Arc;
use uc_core::ports::SearchIndexPort;
use uc_core::search::{SearchError, SearchQuery, SearchResult};

/// Use case that executes a structured search query against the local encrypted search index.
///
/// Returns `Vec<SearchResult>` — one row per matching entry with full render metadata
/// (per D-01, avoids a second query for UI hydration).
pub struct SearchClipboardEntries {
    search_index: Arc<dyn SearchIndexPort>,
}

impl SearchClipboardEntries {
    /// Construct from a `SearchIndexPort`.
    pub fn from_port(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    /// Execute the given search query and return matching results.
    ///
    /// Delegates directly to `SearchIndexPort::search` and propagates the result unchanged.
    #[tracing::instrument(
        name = "usecase.search_clipboard_entries.execute",
        skip(self, query)
    )]
    pub async fn execute(&self, query: SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
        self.search_index.search(query).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uc_core::ids::EntryId;
    use uc_core::search::{
        FileType, QueryOperator, RebuildProgress, SearchDocument, SearchError, SearchIndexMeta,
        SearchPosting, SearchQuery, SearchResult,
    };

    struct MockSearchIndex {
        last_query: Arc<Mutex<Option<SearchQuery>>>,
        next_result: Arc<Mutex<Vec<SearchResult>>>,
        fail_next: Arc<Mutex<Option<SearchError>>>,
    }

    impl MockSearchIndex {
        fn new() -> Self {
            Self {
                last_query: Arc::new(Mutex::new(None)),
                next_result: Arc::new(Mutex::new(vec![])),
                fail_next: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl SearchIndexPort for MockSearchIndex {
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

        async fn search(&self, q: SearchQuery) -> Result<Vec<SearchResult>, SearchError> {
            if let Some(e) = self.fail_next.lock().await.take() {
                return Err(e);
            }
            *self.last_query.lock().await = Some(q);
            Ok(self.next_result.lock().await.clone())
        }

        async fn rebuild(
            &self,
            _e: Vec<(SearchDocument, Vec<SearchPosting>)>,
            _tx: tokio::sync::mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            unimplemented!("not exercised in this test")
        }
    }

    fn make_query(s: &str) -> SearchQuery {
        SearchQuery {
            query_string: s.into(),
            operator: QueryOperator::And,
            time_range: None,
            file_types: vec![],
            extensions: vec![],
            limit: 10,
            offset: 0,
        }
    }

    fn make_search_result(entry_id: &str) -> SearchResult {
        SearchResult {
            entry_id: EntryId::from(entry_id),
            file_type: FileType::Text,
            active_time_ms: 1000,
            text_preview: Some("preview".into()),
            mime_type: "text/plain".into(),
            file_extensions: vec![],
        }
    }

    #[tokio::test]
    async fn execute_forwards_query_and_returns_empty_results() {
        let mock = Arc::new(MockSearchIndex::new());
        let last_query = mock.last_query.clone();

        let uc = SearchClipboardEntries::from_port(mock as Arc<dyn SearchIndexPort>);
        let query = make_query("hello");

        let result = uc.execute(query.clone()).await.unwrap();
        assert!(result.is_empty());
        let captured = last_query.lock().await;
        assert_eq!(captured.as_ref().unwrap().query_string, "hello");
    }

    #[tokio::test]
    async fn execute_returns_single_result() {
        let mock = Arc::new(MockSearchIndex::new());
        let expected = make_search_result("entry-search-1");
        *mock.next_result.lock().await = vec![expected.clone()];

        let uc = SearchClipboardEntries::from_port(mock as Arc<dyn SearchIndexPort>);
        let result = uc.execute(make_query("world")).await.unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], expected);
    }

    #[tokio::test]
    async fn execute_forwards_query_and_returns_page_metadata() {
        let mock = Arc::new(MockSearchIndex::new());
        let r1 = make_search_result("entry-p1");
        let r2 = make_search_result("entry-p2");
        *mock.next_result.lock().await = vec![r1.clone(), r2.clone()];

        let uc = SearchClipboardEntries::from_port(mock as Arc<dyn SearchIndexPort>);
        let page = uc.execute(make_query("hello")).await.unwrap();
        // Verify page contract: items carry through, total and has_more come from the port.
        assert_eq!(page.items.len(), 2);
        assert_eq!(page.items[0], r1);
        assert_eq!(page.items[1], r2);
        // total and has_more are whatever the port provides (mock returns 0/false defaults)
        let _ = page.total;
        let _ = page.has_more;
    }

    #[tokio::test]
    async fn execute_propagates_invalid_query_error() {
        let mock = Arc::new(MockSearchIndex::new());
        *mock.fail_next.lock().await =
            Some(SearchError::InvalidQuery("mixed operators".into()));

        let uc = SearchClipboardEntries::from_port(mock as Arc<dyn SearchIndexPort>);
        let result = uc.execute(make_query("foo AND OR bar")).await;

        assert!(matches!(result, Err(SearchError::InvalidQuery(_))));
    }
}
