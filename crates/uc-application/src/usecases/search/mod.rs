//! Application-layer search use cases. `pub(crate)` per
//! `uc-application/AGENTS.md` §11.4 — external callers go through
//! `SearchFacade`.

pub(crate) mod search_clipboard_entries;

pub(crate) use search_clipboard_entries::SearchClipboardEntriesUseCase;
