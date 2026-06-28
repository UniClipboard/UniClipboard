//! Search domain models — types and errors referenced by SearchIndexPort and
//! SearchKeyDerivationPort in `crate::ports::search`.
//!
//! This module is pure contract definition: no implementations, no database access,
//! no HTTP routes. Implementation layers live in uc-infra (Phase 90+) and
//! daemon routes live in uc-daemon (Phase 92).

pub mod document;
pub mod error;
pub mod key;
pub mod pipeline_input;
pub mod query;
pub mod result;
pub mod tag;

pub use document::{ContentType, SearchDocument, SearchIndexMeta, SearchPosting};
pub use error::SearchError;
pub use key::SearchKey;
pub use pipeline_input::SearchPipelineInput;
pub use query::{QueryOperator, SearchQuery, TimeRangeFilter};
pub use result::{RebuildProgress, RebuildStage, SearchResult, SearchResultsPage};
pub use tag::{builtin as builtin_tags, TagId, TagKind, TagRule, TaggableContent};
