//! Search use cases (SIDX-01).
//!
//! Each use case is a thin orchestrator over `SearchIndexPort`. Construction
//! of `SearchDocument` / `Vec<SearchPosting>` (tokenization, HMAC tagging) is
//! the caller's responsibility — see Phase 90 tokenizer pipeline.
pub mod index_clipboard_entry;
pub mod rebuild_search_index;
pub mod remove_indexed_entry;
pub mod search_clipboard_entries;

pub use index_clipboard_entry::IndexClipboardEntry;
pub use rebuild_search_index::RebuildSearchIndex;
pub use remove_indexed_entry::RemoveIndexedEntry;
pub use search_clipboard_entries::SearchClipboardEntries;
