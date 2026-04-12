//! IndexClipboardEntry use case — indexes a clipboard entry into the search index (SIDX-01).

use std::sync::Arc;
use uc_core::ports::SearchIndexPort;
use uc_core::search::{SearchDocument, SearchError, SearchPosting};

/// Use case that indexes a clipboard entry's document and postings into the search index.
///
/// This is a thin orchestrator: the caller is responsible for constructing
/// `SearchDocument` and `Vec<SearchPosting>`. Tokenization and HMAC computation
/// live in Phase 90.
pub struct IndexClipboardEntry {
    search_index: Arc<dyn SearchIndexPort>,
}

impl IndexClipboardEntry {
    /// Construct from a `SearchIndexPort`.
    pub fn from_port(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    /// Index the given document and postings into the search index.
    ///
    /// Delegates directly to `SearchIndexPort::index_entry` and propagates the result unchanged.
    #[tracing::instrument(
        name = "usecase.index_clipboard_entry.execute",
        skip(self, document, postings),
        fields(entry_id = %document.entry_id, posting_count = postings.len())
    )]
    pub async fn execute(
        &self,
        document: SearchDocument,
        postings: Vec<SearchPosting>,
    ) -> Result<(), SearchError> {
        self.search_index.index_entry(document, postings).await?;
        tracing::debug!("entry indexed successfully");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use uc_core::ids::{EntryId, EventId};
    use uc_core::search::{
        ContentType, RebuildProgress, SearchDocument, SearchError, SearchIndexMeta, SearchPosting,
        SearchQuery, SearchResultsPage,
    };

    struct MockSearchIndex {
        last_doc: Arc<Mutex<Option<SearchDocument>>>,
        last_postings: Arc<Mutex<Option<Vec<SearchPosting>>>>,
        fail_next: Arc<Mutex<Option<SearchError>>>,
    }

    impl MockSearchIndex {
        fn new() -> Self {
            Self {
                last_doc: Arc::new(Mutex::new(None)),
                last_postings: Arc::new(Mutex::new(None)),
                fail_next: Arc::new(Mutex::new(None)),
            }
        }
    }

    #[async_trait]
    impl SearchIndexPort for MockSearchIndex {
        async fn index_entry(
            &self,
            d: SearchDocument,
            p: Vec<SearchPosting>,
        ) -> Result<(), SearchError> {
            if let Some(e) = self.fail_next.lock().await.take() {
                return Err(e);
            }
            *self.last_doc.lock().await = Some(d);
            *self.last_postings.lock().await = Some(p);
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
            _tx: tokio::sync::mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            unimplemented!("not exercised in this test")
        }
    }

    fn make_test_document(entry_id: &str) -> SearchDocument {
        SearchDocument {
            entry_id: EntryId::from(entry_id),
            event_id: EventId::from("evt-test"),
            active_time_ms: 1000,
            captured_at_ms: 1000,
            content_type: ContentType::Text,
            file_extensions: vec![],
            mime_type: "text/plain".into(),
            indexed_at_ms: 1000,
            index_version: "v1".into(),
            text_preview: Some("hello world".into()),
        }
    }

    fn make_test_postings(entry_id: &str) -> Vec<SearchPosting> {
        vec![SearchPosting {
            term_tag: vec![1, 2, 3, 4],
            entry_id: EntryId::from(entry_id),
            field_mask: 1,
            term_freq: 1,
        }]
    }

    #[tokio::test]
    async fn execute_delegates_to_port_and_propagates_ok() {
        let mock = Arc::new(MockSearchIndex::new());
        let last_doc = mock.last_doc.clone();
        let last_postings = mock.last_postings.clone();

        let uc = IndexClipboardEntry::from_port(mock as Arc<dyn SearchIndexPort>);
        let doc = make_test_document("entry-1");
        let postings = make_test_postings("entry-1");

        let result = uc.execute(doc.clone(), postings.clone()).await;
        assert!(result.is_ok());
        assert_eq!(*last_doc.lock().await, Some(doc));
        assert_eq!(*last_postings.lock().await, Some(postings));
    }

    #[tokio::test]
    async fn execute_propagates_port_error() {
        let mock = Arc::new(MockSearchIndex::new());
        *mock.fail_next.lock().await = Some(SearchError::IndexUnavailable);

        let uc = IndexClipboardEntry::from_port(mock as Arc<dyn SearchIndexPort>);
        let result = uc
            .execute(make_test_document("entry-2"), make_test_postings("entry-2"))
            .await;

        assert!(matches!(result, Err(SearchError::IndexUnavailable)));
    }
}
