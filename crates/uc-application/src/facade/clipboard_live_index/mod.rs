use std::sync::Arc;

use async_trait::async_trait;
use thiserror::Error;
use tracing::debug;
use uc_core::ids::EntryId;
use uc_core::ports::clipboard::GetClipboardEntryPort;
use uc_core::ports::search::SearchPipelinePort;
use uc_core::ports::{SearchIndexPort, SearchKeyDerivationPort, SelectRepresentationPolicyPort};
use uc_core::SystemClipboardSnapshot;

use crate::facade::SearchProjectionBuilder;

#[derive(Debug, Clone)]
pub struct ClipboardLiveIndexInput {
    pub entry_id: String,
    /// Shared snapshot. Live indexing only reads the snapshot, so callers pass
    /// an `Arc` clone instead of deep-copying the (potentially multi-megabyte
    /// image) payload — see the daemon clipboard watcher, which shares one
    /// snapshot between live indexing and outbound dispatch.
    pub snapshot: Arc<SystemClipboardSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardLiveIndexOutcome {
    Indexed,
    Skipped { reason: String },
}

#[derive(Debug, Error)]
pub enum ClipboardLiveIndexError {
    #[error("clipboard live index failed: {0}")]
    Internal(String),
}

#[async_trait]
pub trait ClipboardLiveIndexPort: Send + Sync {
    async fn index_capture(
        &self,
        input: ClipboardLiveIndexInput,
    ) -> Result<ClipboardLiveIndexOutcome, ClipboardLiveIndexError>;
}

pub struct ClipboardLiveIndexDeps {
    pub clipboard_entry_repo: Arc<dyn GetClipboardEntryPort>,
    pub representation_policy: Arc<dyn SelectRepresentationPolicyPort>,
    pub search_key_derivation: Arc<dyn SearchKeyDerivationPort>,
    pub search_pipeline: Arc<dyn SearchPipelinePort>,
    pub search_index: Arc<dyn SearchIndexPort>,
}

pub struct ClipboardLiveIndexer {
    deps: ClipboardLiveIndexDeps,
}

impl ClipboardLiveIndexer {
    pub fn new(deps: ClipboardLiveIndexDeps) -> Self {
        Self { deps }
    }
}

#[async_trait]
impl ClipboardLiveIndexPort for ClipboardLiveIndexer {
    async fn index_capture(
        &self,
        input: ClipboardLiveIndexInput,
    ) -> Result<ClipboardLiveIndexOutcome, ClipboardLiveIndexError> {
        let entry_id = EntryId::from(input.entry_id.as_str());
        let entry = match self
            .deps
            .clipboard_entry_repo
            .get_entry(&entry_id)
            .await
            .map_err(|err| ClipboardLiveIndexError::Internal(err.to_string()))?
        {
            Some(entry) => entry,
            None => {
                return Ok(ClipboardLiveIndexOutcome::Skipped {
                    reason: "entry_not_found".to_string(),
                })
            }
        };

        let selection = self
            .deps
            .representation_policy
            .select(input.snapshot.as_ref())
            .map_err(|err| ClipboardLiveIndexError::Internal(err.to_string()))?;

        let Some(pipeline_input) = SearchProjectionBuilder::build_from_capture(
            &entry,
            input.snapshot.as_ref(),
            &selection,
        ) else {
            return Ok(ClipboardLiveIndexOutcome::Skipped {
                reason: "no_searchable_content".to_string(),
            });
        };

        let search_key = match self.deps.search_key_derivation.derive_search_key().await {
            Ok(search_key) => search_key,
            Err(err) => {
                debug!(
                    error = %err,
                    entry_id = %entry_id,
                    "search: key derivation failed, skipping live index"
                );
                return Ok(ClipboardLiveIndexOutcome::Skipped {
                    reason: "search_key_unavailable".to_string(),
                });
            }
        };

        let (document, postings) = self
            .deps
            .search_pipeline
            .build(&pipeline_input, &search_key)
            .map_err(|err| ClipboardLiveIndexError::Internal(err.to_string()))?;

        if postings.is_empty() {
            return Ok(ClipboardLiveIndexOutcome::Skipped {
                reason: "no_postings".to_string(),
            });
        }

        self.deps
            .search_index
            .index_entry(document, postings)
            .await
            .map_err(|err| ClipboardLiveIndexError::Internal(err.to_string()))?;

        Ok(ClipboardLiveIndexOutcome::Indexed)
    }
}

pub struct ClipboardLiveIndexFacade {
    indexer: Arc<dyn ClipboardLiveIndexPort>,
}

impl ClipboardLiveIndexFacade {
    pub fn new(indexer: Arc<dyn ClipboardLiveIndexPort>) -> Self {
        Self { indexer }
    }

    pub async fn index_capture(
        &self,
        input: ClipboardLiveIndexInput,
    ) -> Result<ClipboardLiveIndexOutcome, ClipboardLiveIndexError> {
        self.indexer.index_capture(input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use uc_core::SystemClipboardSnapshot;

    struct FakeIndexer;

    #[async_trait]
    impl ClipboardLiveIndexPort for FakeIndexer {
        async fn index_capture(
            &self,
            _input: ClipboardLiveIndexInput,
        ) -> Result<ClipboardLiveIndexOutcome, ClipboardLiveIndexError> {
            Ok(ClipboardLiveIndexOutcome::Indexed)
        }
    }

    #[tokio::test]
    async fn index_capture_accepts_application_entry_id() {
        let facade = ClipboardLiveIndexFacade::new(std::sync::Arc::new(FakeIndexer));
        let outcome = facade
            .index_capture(ClipboardLiveIndexInput {
                entry_id: "entry-a".to_string(),
                snapshot: Arc::new(SystemClipboardSnapshot {
                    representations: Vec::new(),
                    ts_ms: 0,
                }),
            })
            .await
            .unwrap();

        assert_eq!(outcome, ClipboardLiveIndexOutcome::Indexed);
    }
}
