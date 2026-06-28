//! Search pipeline: builds `SearchDocument` and `Vec<SearchPosting>` from clipboard content.
//!
//! `SearchPipeline` combines `SearchTextExtractor` and `SearchTokenizer` into a single entry
//! point. Callers provide a `SearchPipelineInput` and a `SearchKey`; the pipeline returns
//! ready-to-store `SearchDocument` and aggregated `SearchPosting` values.
//!
//! This module does NOT implement live database adapters or query logic.

use std::collections::HashMap;

use anyhow::Result;
use unicode_normalization::UnicodeNormalization;

use uc_core::ports::search::search_pipeline::SearchPipelinePort;
use uc_core::search::document::{SearchDocument, SearchPosting};
use uc_core::search::key::SearchKey;

use crate::search::constants::{
    CURRENT_INDEX_VERSION, SEARCH_FIELD_BODY, SEARCH_FIELD_FILE_NAME, SEARCH_FIELD_FILE_PATH,
    SEARCH_FIELD_HTML, SEARCH_FIELD_URL,
};
use crate::search::search_key_derivation::term_tag;
use crate::search::text_extractor::{SearchPipelineInput, SearchTextExtractor};
use crate::search::tokenizer::SearchTokenizer;

/// Combines extractor + tokenizer into a single build step.
///
/// Produces:
/// - `SearchDocument` — metadata row ready for the search_document table.
/// - `Vec<SearchPosting>` — aggregated posting rows ready for search_posting.
pub struct SearchPipeline {
    extractor: SearchTextExtractor,
    tokenizer: SearchTokenizer,
}

impl Default for SearchPipeline {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchPipeline {
    /// Create a new `SearchPipeline`.
    pub fn new() -> Self {
        Self {
            extractor: SearchTextExtractor,
            tokenizer: SearchTokenizer,
        }
    }
}

impl SearchPipelinePort for SearchPipeline {
    /// Build a `SearchDocument` from the input (no key needed).
    ///
    /// Sets `index_version` to `CURRENT_INDEX_VERSION`.
    /// Deduplicates and sorts `file_extensions`.
    fn build_document(&self, input: &SearchPipelineInput) -> SearchDocument {
        let extracted = self.extractor.extract(input);

        // Deduplicate and sort file_extensions
        let mut exts = input.file_extensions.clone();
        exts.sort();
        exts.dedup();

        SearchDocument {
            entry_id: input.entry_id.clone(),
            event_id: input.event_id.clone(),
            active_time_ms: input.active_time_ms,
            captured_at_ms: input.captured_at_ms,
            content_type: input.content_type.clone(),
            tags: input.tags.clone(),
            file_extensions: exts,
            mime_type: input.mime_type.clone(),
            indexed_at_ms: chrono::Utc::now().timestamp_millis(),
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: extracted.text_preview,
            char_count: input.char_count,
            file_names: input.file_names.clone(),
            link_urls: input.link_urls.clone(),
            source_device: input.source_device.clone(),
            payload_state: input.payload_state.clone(),
        }
    }

    /// Build `Vec<SearchPosting>` from the input with a derived `SearchKey`.
    ///
    /// Postings are aggregated: if the same term appears in multiple fields, its
    /// `field_mask` is ORed across all fields and `term_freq` counts total
    /// occurrences across all fields (including duplicates within a segment).
    ///
    /// Output is sorted by `term_tag` (then `field_mask`) for deterministic ordering.
    fn build_postings(
        &self,
        input: &SearchPipelineInput,
        search_key: &SearchKey,
    ) -> Result<Vec<SearchPosting>> {
        let extracted = self.extractor.extract(input);
        let entry_id = input.entry_id.clone();

        // Map: term_tag_bytes → (field_mask, term_freq)
        let mut aggregated: HashMap<Vec<u8>, (u8, u32)> = HashMap::new();

        // All searchable fields go through the same tokenize path. Prefix
        // expansion is decided per token (length + non-CJK), not per field, so
        // identifiers embedded in plain-text body (`localhost`, `apiUserManager`)
        // get the same partial-search support as URL/file fields. `field_mask`
        // is still tracked so BM25 ranking can boost matches on identifier-rich
        // fields. Body / HTML are already char-capped by the extractor, which
        // bounds the cost of `count_raw_tokens` (#580).
        let fields: &[(&[String], u8)] = &[
            (&extracted.body, SEARCH_FIELD_BODY),
            (&extracted.html, SEARCH_FIELD_HTML),
            (&extracted.url, SEARCH_FIELD_URL),
            (&extracted.file_path, SEARCH_FIELD_FILE_PATH),
            (&extracted.file_name, SEARCH_FIELD_FILE_NAME),
        ];

        for (segments, field_bit) in fields {
            for segment in *segments {
                let raw_token_counts =
                    count_raw_tokens(&self.tokenizer.tokenize_segment(segment), segment);
                for (token, freq) in raw_token_counts {
                    let tag = term_tag(search_key, &token)?;
                    let entry = aggregated.entry(tag).or_insert((0u8, 0u32));
                    entry.0 |= *field_bit;
                    entry.1 += freq;
                }
            }
        }

        // Build sorted Vec<SearchPosting>
        let mut postings: Vec<SearchPosting> = aggregated
            .into_iter()
            .map(|(tag, (field_mask, term_freq))| SearchPosting {
                term_tag: tag,
                entry_id: entry_id.clone(),
                field_mask,
                term_freq,
            })
            .collect();

        // Sort by term_tag then field_mask for determinism
        postings.sort_by(|a, b| {
            a.term_tag
                .cmp(&b.term_tag)
                .then(a.field_mask.cmp(&b.field_mask))
        });

        Ok(postings)
    }

    /// Build both `SearchDocument` and `Vec<SearchPosting>` in one call.
    fn build(
        &self,
        input: &SearchPipelineInput,
        search_key: &SearchKey,
    ) -> Result<(SearchDocument, Vec<SearchPosting>)> {
        let document = self.build_document(input);
        let postings = self.build_postings(input, search_key)?;
        Ok((document, postings))
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Count how many times each token from `tokens` appears in `raw_segment`.
///
/// This estimates term frequency by counting substring occurrences of the token
/// in the lowercased raw segment. For compound tokens (identifiers), we fall
/// back to a count of 1.
fn count_raw_tokens(tokens: &[String], raw_segment: &str) -> Vec<(String, u32)> {
    let lowered: String = raw_segment.nfkc().collect::<String>().to_lowercase();

    tokens
        .iter()
        .map(|token| {
            // Count non-overlapping occurrences of this token in the lowercased segment.
            let count = count_occurrences(&lowered, token);
            (token.clone(), count.max(1))
        })
        .collect()
}

/// Count non-overlapping occurrences of `needle` in `haystack`.
fn count_occurrences(haystack: &str, needle: &str) -> u32 {
    if needle.is_empty() {
        return 0;
    }
    let mut count = 0u32;
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle.as_ref() as &str) {
        count += 1;
        start += pos + needle.len();
        if start >= haystack.len() {
            break;
        }
    }
    count
}

// ──────────────────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────────────────
