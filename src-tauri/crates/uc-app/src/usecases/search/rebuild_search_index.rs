//! RebuildSearchIndex use case — triggers a full index rebuild via the search port (D-04).

use std::sync::Arc;
use tokio::sync::mpsc::Sender;
use uc_core::ports::SearchIndexPort;
use uc_core::search::{RebuildProgress, SearchDocument, SearchError, SearchPosting};

/// Use case that triggers a full index rebuild.
///
/// The caller supplies an `mpsc::Sender<RebuildProgress>` so the daemon can forward
/// progress events over WebSocket without uc-core having any WebSocket knowledge (D-07).
/// The Sender is forwarded directly to the port — not cloned into a new channel.
pub struct RebuildSearchIndex {
    search_index: Arc<dyn SearchIndexPort>,
}

impl RebuildSearchIndex {
    /// Construct from a `SearchIndexPort`.
    pub fn from_port(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    /// Trigger a full index rebuild with the supplied entries and progress channel.
    ///
    /// Delegates directly to `SearchIndexPort::rebuild`, forwarding the caller-supplied
    /// `progress_tx` unchanged. Propagates the result unchanged.
    #[tracing::instrument(
        name = "usecase.rebuild_search_index.execute",
        skip(self, entries, progress_tx),
        fields(entry_count = entries.len())
    )]
    pub async fn execute(
        &self,
        entries: Vec<(SearchDocument, Vec<SearchPosting>)>,
        progress_tx: Sender<RebuildProgress>,
    ) -> Result<(), SearchError> {
        tracing::info!(entry_count = entries.len(), "starting search index rebuild");
        self.search_index.rebuild(entries, progress_tx).await?;
        tracing::info!("search index rebuild completed");
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
        ContentType, RebuildProgress, RebuildStage, SearchDocument, SearchError, SearchIndexMeta,
        SearchPosting, SearchQuery, SearchResultsPage,
    };

    struct MockSearchIndex {
        /// Number of entries received in last rebuild call.
        last_entry_count: Arc<Mutex<Option<usize>>>,
        fail_next: Arc<Mutex<Option<SearchError>>>,
    }

    impl MockSearchIndex {
        fn new() -> Self {
            Self {
                last_entry_count: Arc::new(Mutex::new(None)),
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

        async fn search(&self, _q: SearchQuery) -> Result<SearchResultsPage, SearchError> {
            Ok(SearchResultsPage {
                items: vec![],
                total: 0,
                has_more: false,
            })
        }

        async fn rebuild(
            &self,
            e: Vec<(SearchDocument, Vec<SearchPosting>)>,
            tx: tokio::sync::mpsc::Sender<RebuildProgress>,
        ) -> Result<(), SearchError> {
            if let Some(err) = self.fail_next.lock().await.take() {
                return Err(err);
            }
            *self.last_entry_count.lock().await = Some(e.len());
            // Send a progress event through the forwarded Sender to prove it was not dropped.
            let _ = tx
                .send(RebuildProgress {
                    stage: RebuildStage::Started,
                    indexed: 0,
                    total: e.len() as u32,
                })
                .await;
            Ok(())
        }

        async fn get_index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
            unimplemented!("not exercised in this test")
        }
    }

    fn make_test_document(entry_id: &str) -> SearchDocument {
        SearchDocument {
            entry_id: EntryId::from(entry_id),
            event_id: EventId::from("evt-rebuild"),
            active_time_ms: 1000,
            captured_at_ms: 1000,
            content_type: ContentType::Text,
            file_extensions: vec![],
            mime_type: "text/plain".into(),
            indexed_at_ms: 1000,
            index_version: "v1".into(),
            text_preview: None,
        }
    }

    #[tokio::test]
    async fn execute_forwards_sender_and_entries_to_port() {
        let mock = Arc::new(MockSearchIndex::new());
        let last_count = mock.last_entry_count.clone();

        let uc = RebuildSearchIndex::from_port(mock as Arc<dyn SearchIndexPort>);
        let entries = vec![
            (make_test_document("e1"), vec![]),
            (make_test_document("e2"), vec![]),
        ];
        let (tx, mut rx) = tokio::sync::mpsc::channel::<RebuildProgress>(4);

        let result = uc.execute(entries, tx).await;
        assert!(result.is_ok());

        // The mock sends progress via the forwarded Sender; the test receives it here.
        let progress = rx.recv().await.expect("expected a RebuildProgress message");
        assert_eq!(progress.stage, RebuildStage::Started);
        assert_eq!(progress.total, 2);

        assert_eq!(*last_count.lock().await, Some(2));
    }

    #[tokio::test]
    async fn execute_propagates_port_error() {
        let mock = Arc::new(MockSearchIndex::new());
        *mock.fail_next.lock().await = Some(SearchError::IndexNotReady);

        let uc = RebuildSearchIndex::from_port(mock as Arc<dyn SearchIndexPort>);
        let (tx, _rx) = tokio::sync::mpsc::channel::<RebuildProgress>(4);

        let result = uc.execute(vec![], tx).await;
        assert!(matches!(result, Err(SearchError::IndexNotReady)));
    }
}
