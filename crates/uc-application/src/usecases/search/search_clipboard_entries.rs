//! Use case that executes a structured search query against the local
//! encrypted search index.

use std::sync::Arc;
use uc_core::ports::SearchIndexPort;
use uc_core::search::tag::SearchTagCount;
use uc_core::search::{SearchError, SearchIndexMeta, SearchQuery, SearchResultsPage};

pub(crate) struct SearchClipboardEntriesUseCase {
    search_index: Arc<dyn SearchIndexPort>,
}

impl SearchClipboardEntriesUseCase {
    pub(crate) fn from_port(search_index: Arc<dyn SearchIndexPort>) -> Self {
        Self { search_index }
    }

    #[tracing::instrument(
        name = "usecase.search_clipboard_entries.execute",
        skip(self, query),
        fields(
            query_len = query.query_string.len(),
            operator = ?query.operator,
            limit = query.limit,
            offset = query.offset
        )
    )]
    pub(crate) async fn execute(
        &self,
        query: SearchQuery,
    ) -> Result<SearchResultsPage, SearchError> {
        let page = self.search_index.search(query).await?;
        tracing::debug!(
            total = page.total,
            returned = page.items.len(),
            has_more = page.has_more,
            "search completed"
        );
        Ok(page)
    }

    pub(crate) async fn index_meta(&self) -> Result<SearchIndexMeta, SearchError> {
        self.search_index.get_index_meta().await
    }

    pub(crate) async fn list_tags(&self) -> Result<Vec<SearchTagCount>, SearchError> {
        self.search_index.list_tags().await
    }
}
