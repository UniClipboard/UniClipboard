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

    /// Build a `SearchDocument` from the input (no key needed).
    ///
    /// Sets `index_version` to `CURRENT_INDEX_VERSION`.
    /// Deduplicates and sorts `file_extensions`.
    pub fn build_document(&self, input: &SearchPipelineInput) -> SearchDocument {
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
            file_extensions: exts,
            mime_type: input.mime_type.clone(),
            indexed_at_ms: chrono::Utc::now().timestamp_millis(),
            index_version: CURRENT_INDEX_VERSION.to_string(),
            text_preview: extracted.text_preview,
        }
    }

    /// Build `Vec<SearchPosting>` from the input with a derived `SearchKey`.
    ///
    /// Postings are aggregated: if the same term appears in multiple fields, its
    /// `field_mask` is ORed across all fields and `term_freq` counts total
    /// occurrences across all fields (including duplicates within a segment).
    ///
    /// Output is sorted by `term_tag` (then `field_mask`) for deterministic ordering.
    pub fn build_postings(
        &self,
        input: &SearchPipelineInput,
        search_key: &SearchKey,
    ) -> Result<Vec<SearchPosting>> {
        let extracted = self.extractor.extract(input);
        let entry_id = input.entry_id.clone();

        // Map: term_tag_bytes → (field_mask, term_freq)
        let mut aggregated: HashMap<Vec<u8>, (u8, u32)> = HashMap::new();

        // Body / HTML: no prefix expansion — text is already capped at 1 000 chars by the
        // extractor, and prefix tokens on free-form prose offer little UX value.
        let body_fields: &[(&[String], u8)] = &[
            (&extracted.body, SEARCH_FIELD_BODY),
            (&extracted.html, SEARCH_FIELD_HTML),
        ];
        // Identifier-rich fields: prefix expansion enabled so partial queries like
        // "uniclip" match entries indexed under "uniclipboard".
        let rich_fields: &[(&[String], u8)] = &[
            (&extracted.url, SEARCH_FIELD_URL),
            (&extracted.file_path, SEARCH_FIELD_FILE_PATH),
            (&extracted.file_name, SEARCH_FIELD_FILE_NAME),
        ];

        for (segments, field_bit) in body_fields {
            for segment in *segments {
                let raw_token_counts =
                    count_raw_tokens(&self.tokenizer.tokenize_segment_no_prefix(segment), segment);
                for (token, freq) in raw_token_counts {
                    let tag = term_tag(search_key, &token)?;
                    let entry = aggregated.entry(tag).or_insert((0u8, 0u32));
                    entry.0 |= *field_bit;
                    entry.1 += freq;
                }
            }
        }

        for (segments, field_bit) in rich_fields {
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
    pub fn build(
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

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::{EntryId, EventId};
    use uc_core::search::document::ContentType;

    fn base_input() -> SearchPipelineInput {
        SearchPipelineInput {
            entry_id: EntryId::from("entry-01"),
            event_id: EventId::from("event-01"),
            active_time_ms: 1_000,
            captured_at_ms: 900,
            content_type: ContentType::Text,
            mime_type: "text/plain".to_string(),
            file_extensions: vec![],
            plain_text: None,
            html_text: None,
            uri_list: vec![],
            file_paths: vec![],
            file_names: vec![],
            text_preview: None,
        }
    }

    fn test_key() -> SearchKey {
        SearchKey([0xABu8; 32])
    }

    #[test]
    fn pipeline_writes_current_index_version() {
        let pipeline = SearchPipeline::new();
        let input = SearchPipelineInput {
            plain_text: Some("hello".to_string()),
            ..base_input()
        };
        let doc = pipeline.build_document(&input);
        assert_eq!(doc.index_version, CURRENT_INDEX_VERSION);
    }

    #[test]
    fn pipeline_aggregates_repeated_hits_into_one_posting_with_increased_term_freq() {
        let pipeline = SearchPipeline::new();
        // "hello hello hello" — same token appears 3× in body
        let input = SearchPipelineInput {
            plain_text: Some("hello hello hello".to_string()),
            ..base_input()
        };
        let postings = pipeline.build_postings(&input, &test_key()).unwrap();

        // Find the posting for "hello" (we don't know the tag, but term_freq > 1)
        let max_freq = postings.iter().map(|p| p.term_freq).max().unwrap_or(0);
        assert!(
            max_freq >= 3,
            "expected term_freq >= 3 for repeated token, got max {max_freq}"
        );
    }

    #[test]
    fn pipeline_field_mask_ors_across_fields() {
        let pipeline = SearchPipeline::new();
        // "rust" appears in both plain_text and a URL segment
        let input = SearchPipelineInput {
            plain_text: Some("rust".to_string()),
            uri_list: vec!["https://rust-lang.org/rust".to_string()],
            ..base_input()
        };
        let postings = pipeline.build_postings(&input, &test_key()).unwrap();

        // There should be at least one posting with both BODY and URL bits set
        let combined_mask_posting = postings.iter().find(|p| {
            p.field_mask & SEARCH_FIELD_BODY != 0 && p.field_mask & SEARCH_FIELD_URL != 0
        });
        assert!(
            combined_mask_posting.is_some(),
            "expected posting with both BODY and URL field bits; postings: {postings:?}"
        );
    }

    #[test]
    fn pipeline_postings_output_is_deterministic() {
        let pipeline = SearchPipeline::new();
        let input = SearchPipelineInput {
            plain_text: Some("alpha beta gamma".to_string()),
            ..base_input()
        };
        let p1 = pipeline.build_postings(&input, &test_key()).unwrap();
        let p2 = pipeline.build_postings(&input, &test_key()).unwrap();
        assert_eq!(p1.len(), p2.len());
        for (a, b) in p1.iter().zip(p2.iter()) {
            assert_eq!(a.term_tag, b.term_tag);
            assert_eq!(a.field_mask, b.field_mask);
            assert_eq!(a.term_freq, b.term_freq);
        }
    }

    #[test]
    fn pipeline_document_deduplicates_file_extensions() {
        let pipeline = SearchPipeline::new();
        let input = SearchPipelineInput {
            file_extensions: vec!["txt".to_string(), "md".to_string(), "txt".to_string()],
            ..base_input()
        };
        let doc = pipeline.build_document(&input);
        assert_eq!(
            doc.file_extensions,
            vec!["md".to_string(), "txt".to_string()]
        );
    }

    #[test]
    fn build_returns_document_and_postings_together() {
        let pipeline = SearchPipeline::new();
        let input = SearchPipelineInput {
            plain_text: Some("hello world".to_string()),
            ..base_input()
        };
        let result = pipeline.build(&input, &test_key());
        assert!(result.is_ok());
        let (doc, postings) = result.unwrap();
        assert_eq!(doc.index_version, CURRENT_INDEX_VERSION);
        assert!(!postings.is_empty());
    }
}
