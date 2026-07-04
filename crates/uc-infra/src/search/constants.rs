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
/// - `search-v4`: derived-tag model. `content_type` drops `link` (now a derived
///   tag) and adds `html`; tag membership is persisted in `search_entry_tag` and
///   rebuilt alongside documents/postings. The bump forces a full rebuild so the
///   tag table and reclassified `content_type` values are recomputed.
/// - `search-v5`: render columns on `search_document` (`file_names`,
///   `link_urls`, `source_device`, `payload_state`). The bump forces a rebuild
///   so existing rows backfill the new columns (image dimensions and file sizes
///   stay lazy by design). See the `add_search_document_render_columns` migration.
/// - `search-v6`: content_type is classified over the whole representation set by
///   precedence (`file > image > html > text`) instead of from a single paste
///   representation's MIME — the paste rep is chosen for paste fidelity, not
///   classification (a web-image copy's paste rep is its `<img>` html, which
///   mislabels it `Html`). The image nature moves to a derived `image` tag, so a
///   copied image file is `File`+`image` while a web image / screenshot / pure
///   bitmap is `Image`. The bump forces a rebuild so existing rows reclassify
///   (no schema change).
/// - `search-v7`: the derived `code` tag covers rich text / HTML and plain-text
///   snippets that look like source code. The bump forces existing plain-text
///   code rows to backfill the tag.
/// - `search-v8`: adds the `char_count` render column — the full character count
///   of an entry's primary text content, so the UI shows the real total length
///   instead of the capped `text_preview` length. The rebuild backfills it for
///   existing rows (see the `add_search_document_char_count` migration).
/// - `search-v9`: adds the `file_paths` render column — the local filesystem
///   paths of an entry's referenced files (aligned with `file_names`), so a
///   row can resolve a file's on-disk location without a lazy fetch. The
///   rebuild backfills it (see the `add_search_document_file_paths` migration).
/// - `search-v10`: encrypts the content-derived render columns. The five
///   plaintext columns (`text_preview`, `file_names`, `link_urls`, `file_paths`,
///   `char_count`) are dropped and replaced by a single AEAD-sealed
///   `render_payload` BLOB. The rebuild backfills the encrypted payload from the
///   decrypted representations (see the `encrypt_search_render_columns`
///   migration). `source_device` and `payload_state` stay plaintext by design.
pub const CURRENT_INDEX_VERSION: &str = "search-v10";

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
