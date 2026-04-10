//! Search ports — async traits implemented by uc-infra and injected into
//! use cases via Arc<dyn Port + Send + Sync>.

pub mod search_index;
pub mod search_key;

pub use search_index::SearchIndexPort;
pub use search_key::SearchKeyDerivationPort;
