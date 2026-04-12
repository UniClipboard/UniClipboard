//! RemoveIndexedEntry use case — removes a clipboard entry from the search index (D-05).

use std::sync::Arc;
use uc_core::ids::EntryId;
use uc_core::ports::SearchIndexPort;
use uc_core::search::SearchError;

/// Use case that removes a clipboard entry (document + all postings) from the search index.
///
/// Called synchronously by the delete integration path (Phase 89, D-05) — hard-delete semantic.
pub struct RemoveIndexedEntry {
    search_index: Arc<dyn SearchIndexPort>,
}

impl RemoveIndexedEntry {
    /// Construct from a `SearchIndexPort`.
    pub fn from_port(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    /// Remove the search index document and postings for the given entry.
    ///
    /// Delegates directly to `SearchIndexPort::remove_entry` and propagates the result unchanged.
    #[tracing::instrument(
        name = "usecase.remove_indexed_entry.execute",
        skip(self),
        fields(entry_id = %entry_id)
    )]
    pub async fn execute(&self, entry_id: &EntryId) -> Result<(), SearchError> {
        self.search_index.remove_entry(entry_id).await?;
        tracing::debug!("entry removed from search index");
        Ok(())
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
        RebuildProgress, SearchDocument, SearchError, SearchIndexMeta, SearchPosting, SearchQuery,
        SearchResultsPage,
    };

    struct MockSearchIndex {
        captured_entry_id: Arc<Mutex<Option<EntryId>>>,
        fail_next: Arc<Mutex<Option<SearchError>>>,
    }

    impl MockSearchIndex {
        fn new() -> Self {
            Self {
                captured_entry_id: Arc::new(Mutex::new(None)),
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

        async fn remove_entry(&self, id: &EntryId) -> Result<(), SearchError> {
            if let Some(e) = self.fail_next.lock().await.take() {
                return Err(e);
            }
            *self.captured_entry_id.lock().await = Some(id.clone());
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
            _tx: tokio::sync::mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            unimplemented!("not exercised in this test")
        }
    }

    #[tokio::test]
    async fn execute_forwards_entry_id_to_port() {
        let mock = Arc::new(MockSearchIndex::new());
        let captured = mock.captured_entry_id.clone();

        let uc = RemoveIndexedEntry::from_port(mock as Arc<dyn SearchIndexPort>);
        let entry_id = EntryId::from("entry-to-remove");

        let result = uc.execute(&entry_id).await;
        assert!(result.is_ok());
        assert_eq!(*captured.lock().await, Some(entry_id));
    }

    #[tokio::test]
    async fn execute_propagates_port_error() {
        let mock = Arc::new(MockSearchIndex::new());
        *mock.fail_next.lock().await = Some(SearchError::IndexNotReady);

        let uc = RemoveIndexedEntry::from_port(mock as Arc<dyn SearchIndexPort>);
        let result = uc.execute(&EntryId::from("entry-error")).await;

        assert!(matches!(result, Err(SearchError::IndexNotReady)));
    }
}
