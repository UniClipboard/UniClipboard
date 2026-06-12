//! Search pipeline port.
//!
//! Abstracts the build step that converts a clipboard-entry snapshot
//! (`SearchPipelineInput`) into a `SearchDocument` and the matching
//! aggregated `SearchPosting` rows. Concrete implementation (text
//! extraction + tokenization + HMAC term tagging) lives in `uc-infra`.
//!
//! The port is synchronous because the underlying work is pure CPU
//! (no IO). Implementations must be `Send + Sync` so a single instance
//! can be shared across the runtime via `Arc<dyn SearchPipelinePort>`.

use anyhow::Result;

use crate::search::document::{SearchDocument, SearchPosting};
use crate::search::key::SearchKey;
use crate::search::pipeline_input::SearchPipelineInput;

pub trait SearchPipelinePort: Send + Sync {
    /// Build only the `SearchDocument` (does not require a search key).
    fn build_document(&self, input: &SearchPipelineInput) -> SearchDocument;

    /// Build the inverted-index postings for `input`, tagged with `search_key`.
    fn build_postings(
        &self,
        input: &SearchPipelineInput,
        search_key: &SearchKey,
    ) -> Result<Vec<SearchPosting>>;

    /// Convenience: build both document and postings in one call.
    fn build(
        &self,
        input: &SearchPipelineInput,
        search_key: &SearchKey,
    ) -> Result<(SearchDocument, Vec<SearchPosting>)>;
}
