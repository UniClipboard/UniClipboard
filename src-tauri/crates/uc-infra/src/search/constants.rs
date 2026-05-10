//! Authoritative search schema and tokenizer constants for `uc-infra`.
//!
//! `CURRENT_INDEX_VERSION` must be bumped whenever normalization rules change
//! (NFKC, separator splitting, camelCase, CJK bigram). A version mismatch
//! triggers a full index rebuild in Phase 91.

/// Current tokenizer/normalization schema version.
///
/// Bump this whenever the tokenization rules change to trigger a full rebuild.
///
/// History:
/// - `search-v2`: per-field prefix expansion (body/html unexpanded).
/// - `search-v3`: per-token prefix expansion (#580). Tokens whose length is in
///   `[3, 32]` and that are non-CJK are prefix-expanded regardless of field;
///   long opaque strings (>32 chars, no separators) are full-token only.
pub const CURRENT_INDEX_VERSION: &str = "search-v3";

/// Field-mask bit: term was extracted from the plain-text body.
pub const SEARCH_FIELD_BODY: u8 = 0b0000_0001;

/// Field-mask bit: term was extracted from visible HTML text.
pub const SEARCH_FIELD_HTML: u8 = 0b0000_0010;

/// Field-mask bit: term was extracted from a URL (host, path segments, query param names).
pub const SEARCH_FIELD_URL: u8 = 0b0000_0100;

/// Field-mask bit: term was extracted from a file path (directory segments, stem, extension).
pub const SEARCH_FIELD_FILE_PATH: u8 = 0b0000_1000;

/// Field-mask bit: term was extracted from a file name (display name / stem).
pub const SEARCH_FIELD_FILE_NAME: u8 = 0b0001_0000;
