//! Text extraction from clipboard content for search indexing.
//!
//! `SearchTextExtractor` converts a `SearchPipelineInput` (a snapshot of a clipboard entry's
//! searchable fields) into `ExtractedSearchText` — a structured collection of raw text
//! segments ready for tokenization.
//!
//! Extraction rules:
//! - If `plain_text` is present it is authoritative for `body`. HTML body is skipped.
//! - If `plain_text` is absent and `html_text` is present, visible text is stripped from HTML.
//! - URLs are parsed; host, path segments, and query param names are extracted (not values).
//! - File paths are split into directory segments, stem, and extension.
//! - File names are kept as whole segment plus stem/extension parts.
//! - `text_preview` is carried through or derived from the best available source.

use uc_core::ids::{EntryId, EventId};
use uc_core::search::document::ContentType;
use url::Url;

// ──────────────────────────────────────────────────────────────────────────────
// Input / Output types
// ──────────────────────────────────────────────────────────────────────────────

/// Structured input passed to `SearchTextExtractor` and `SearchPipeline`.
///
/// Callers (use-cases) build this from the clipboard entry's representations.
#[derive(Debug, Clone)]
pub struct SearchPipelineInput {
    pub entry_id: EntryId,
    pub event_id: EventId,
    pub active_time_ms: i64,
    pub captured_at_ms: i64,
    pub content_type: ContentType,
    pub mime_type: String,
    pub file_extensions: Vec<String>,
    pub plain_text: Option<String>,
    pub html_text: Option<String>,
    pub uri_list: Vec<String>,
    pub file_paths: Vec<String>,
    pub file_names: Vec<String>,
    pub text_preview: Option<String>,
}

/// Extracted search text, structured per field.
///
/// Each `Vec<String>` contains raw segments that will be tokenized independently.
#[derive(Debug, Clone, Default)]
pub struct ExtractedSearchText {
    /// Raw text from authoritative plain text content.
    pub body: Vec<String>,
    /// Visible text stripped from HTML (used when plain_text is absent).
    pub html: Vec<String>,
    /// Host, path segments, and query param names from URLs (no values).
    pub url: Vec<String>,
    /// Directory segments, file stems, and extensions from file paths.
    pub file_path: Vec<String>,
    /// File names as whole segment and stem/extension parts.
    pub file_name: Vec<String>,
    /// Short preview for display purposes.
    pub text_preview: Option<String>,
}

// ──────────────────────────────────────────────────────────────────────────────
// Extractor
// ──────────────────────────────────────────────────────────────────────────────

/// Stateless text extractor.
///
/// Call `extract()` with a `SearchPipelineInput` to get `ExtractedSearchText`.
pub struct SearchTextExtractor;

impl SearchTextExtractor {
    /// Extract all searchable text segments from the input.
    pub fn extract(&self, input: &SearchPipelineInput) -> ExtractedSearchText {
        let mut out = ExtractedSearchText::default();

        // Body: plain text is authoritative. Fall back to HTML visible text.
        // Capped at BODY_INDEX_CHAR_LIMIT to bound tokenization and term-frequency
        // scan cost (count_raw_tokens is O(unique_tokens × text_length)).
        if let Some(ref plain) = input.plain_text {
            let capped = cap_text(plain);
            out.body.push(capped);
        } else if let Some(ref html) = input.html_text {
            let visible = strip_html_tags(html);
            if !visible.is_empty() {
                out.html.push(cap_text(&visible));
            }
        }

        // URL: extract host, path segments, query param names (not values).
        for raw_url in &input.uri_list {
            if let Ok(parsed) = Url::parse(raw_url.trim()) {
                // Host
                if let Some(host) = parsed.host_str() {
                    out.url.push(host.to_string());
                }
                // Path segments (skip empty)
                for segment in parsed.path_segments().into_iter().flatten() {
                    let s = segment.trim();
                    if !s.is_empty() {
                        out.url.push(s.to_string());
                    }
                }
                // Query param names only (not values)
                for (key, _value) in parsed.query_pairs() {
                    let k = key.trim().to_string();
                    if !k.is_empty() {
                        out.url.push(k);
                    }
                }
            }
        }

        // File paths: split into directory segments, stem, and extension.
        for path in &input.file_paths {
            extract_path_segments(path, &mut out.file_path);
        }

        // File names: whole segment plus stem/extension parts.
        for name in &input.file_names {
            out.file_name.push(name.clone());
            // Also split stem and extension
            let (stem, ext) = split_stem_ext(name);
            if !stem.is_empty() && stem != *name {
                out.file_name.push(stem);
            }
            if !ext.is_empty() {
                out.file_name.push(ext);
            }
        }

        // text_preview: pass through or derive from best available source.
        out.text_preview = input.text_preview.clone().or_else(|| {
            // Derive from plain text
            if let Some(ref plain) = input.plain_text {
                return Some(derive_preview(plain));
            }
            // Derive from HTML visible text
            if let Some(ref html) = input.html_text {
                let visible = strip_html_tags(html);
                if !visible.is_empty() {
                    return Some(derive_preview(&visible));
                }
            }
            // Derive from first file name
            if let Some(first_name) = input.file_names.first() {
                return Some(derive_preview(first_name));
            }
            // Derive from first URL host
            if let Some(raw_url) = input.uri_list.first() {
                if let Ok(parsed) = Url::parse(raw_url.trim()) {
                    if let Some(host) = parsed.host_str() {
                        return Some(host.to_string());
                    }
                }
            }
            None
        });

        out
    }
}

// ──────────────────────────────────────────────────────────────────────────────
// Private helpers
// ──────────────────────────────────────────────────────────────────────────────

/// Strip HTML tags and decode common HTML entities into plain text.
///
/// This is a simple approach: remove `<...>` spans, decode `&amp;`, `&lt;`,
/// `&gt;`, `&quot;`, `&nbsp;`, `&apos;`, and collapse whitespace.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    // Decode common entities
    let decoded = out
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ");

    // Collapse whitespace
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Split a file path into segments: directory parts, stem, extension.
fn extract_path_segments(path: &str, out: &mut Vec<String>) {
    // Normalize separators
    let normalized = path.replace('\\', "/");
    let parts: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();

    let part_count = parts.len();
    for (i, part) in parts.into_iter().enumerate() {
        if i < part_count - 1 {
            // Directory segment
            out.push(part.to_string());
        } else {
            // Last segment: stem and extension
            let (stem, ext) = split_stem_ext(part);
            out.push(part.to_string()); // whole file name
            if !stem.is_empty() && stem != part {
                out.push(stem);
            }
            if !ext.is_empty() {
                out.push(ext);
            }
        }
    }
}

/// Split a filename into (stem, extension).
/// Returns (`name`, `""`) if there is no extension.
fn split_stem_ext(name: &str) -> (String, String) {
    if let Some(dot_pos) = name.rfind('.') {
        if dot_pos > 0 {
            let stem = &name[..dot_pos];
            let ext = &name[dot_pos + 1..];
            return (stem.to_string(), ext.to_string());
        }
    }
    (name.to_string(), String::new())
}

/// Maximum characters indexed from plain-text and HTML body fields.
///
/// Bounds the O(unique_tokens × text_length) cost in `count_raw_tokens`.
/// Content beyond this limit is not searchable but is still stored in full
/// via the blob / representation layer.
const BODY_INDEX_CHAR_LIMIT: usize = 1_000;

/// Truncate `text` to [`BODY_INDEX_CHAR_LIMIT`] characters on a char boundary.
fn cap_text(text: &str) -> String {
    text.chars().take(BODY_INDEX_CHAR_LIMIT).collect()
}

/// Derive a short preview (up to 200 chars) from text.
fn derive_preview(text: &str) -> String {
    text.chars().take(200).collect()
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

    #[test]
    fn plain_text_wins_over_html() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            plain_text: Some("hello world".to_string()),
            html_text: Some("<b>html content</b>".to_string()),
            ..base_input()
        };
        let result = ext.extract(&input);
        assert_eq!(result.body, vec!["hello world"]);
        assert!(
            result.html.is_empty(),
            "html field should be empty when plain_text is present"
        );
    }

    #[test]
    fn html_visible_text_used_when_plain_text_absent() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            plain_text: None,
            html_text: Some("<p>Hello <b>World</b></p>".to_string()),
            ..base_input()
        };
        let result = ext.extract(&input);
        assert!(
            result.body.is_empty(),
            "body should be empty when plain_text is absent"
        );
        assert!(!result.html.is_empty(), "html should have visible text");
        assert_eq!(result.html[0], "Hello World");
    }

    #[test]
    fn url_extraction_includes_host_path_query_key_names_but_not_values() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            uri_list: vec!["https://github.com/rust-lang/rust?q=search&tab=code".to_string()],
            ..base_input()
        };
        let result = ext.extract(&input);
        // Host
        assert!(
            result.url.contains(&"github.com".to_string()),
            "should contain host"
        );
        // Path segments
        assert!(
            result.url.contains(&"rust-lang".to_string()),
            "should contain path segment"
        );
        assert!(
            result.url.contains(&"rust".to_string()),
            "should contain path segment"
        );
        // Query param names
        assert!(
            result.url.contains(&"q".to_string()),
            "should contain query key name"
        );
        assert!(
            result.url.contains(&"tab".to_string()),
            "should contain query key name"
        );
        // Query values should NOT be present
        assert!(
            !result.url.contains(&"search".to_string()),
            "should NOT contain query value"
        );
        assert!(
            !result.url.contains(&"code".to_string()),
            "should NOT contain query value"
        );
    }

    #[test]
    fn file_path_extraction_includes_directory_segments_stem_and_extension() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            file_paths: vec!["/home/user/documents/report.pdf".to_string()],
            ..base_input()
        };
        let result = ext.extract(&input);
        assert!(result.file_path.contains(&"home".to_string()));
        assert!(result.file_path.contains(&"user".to_string()));
        assert!(result.file_path.contains(&"documents".to_string()));
        assert!(
            result.file_path.contains(&"report.pdf".to_string()),
            "whole filename"
        );
        assert!(result.file_path.contains(&"report".to_string()), "stem");
        assert!(result.file_path.contains(&"pdf".to_string()), "extension");
    }

    #[test]
    fn text_preview_derived_from_plain_text_when_not_provided() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            plain_text: Some("Hello from clipboard".to_string()),
            text_preview: None,
            ..base_input()
        };
        let result = ext.extract(&input);
        assert!(result.text_preview.is_some());
        assert!(result.text_preview.unwrap().contains("Hello"));
    }

    #[test]
    fn text_preview_passed_through_when_provided() {
        let ext = SearchTextExtractor;
        let input = SearchPipelineInput {
            plain_text: Some("long text that would be in body".to_string()),
            text_preview: Some("custom preview".to_string()),
            ..base_input()
        };
        let result = ext.extract(&input);
        assert_eq!(result.text_preview, Some("custom preview".to_string()));
    }
}
