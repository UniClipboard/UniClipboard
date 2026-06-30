//! URL parsing utilities for link content type detection.
//!
//! Provides functions to detect single URLs in text, parse URI lists (RFC 2483),
//! and extract domain names from URLs.

use url::Url;

/// Check if the given text (after trimming) is a single valid URL with no extra content.
///
/// Returns `true` when the trimmed text is non-empty, contains no whitespace,
/// and successfully parses as a URL.
pub fn is_single_url(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    // If there's any whitespace in the trimmed text, it's not a single URL
    if trimmed.contains(char::is_whitespace) {
        return false;
    }
    Url::parse(trimmed).is_ok()
}

/// Check if the given text consists entirely of URLs (one per line).
///
/// Returns `true` when every non-empty line (after trimming) is a valid URL.
/// Requires at least one URL to be present.
pub fn is_all_urls(text: &str) -> bool {
    let lines: Vec<&str> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return false;
    }
    lines
        .iter()
        .all(|line| !line.contains(char::is_whitespace) && Url::parse(line).is_ok())
}

/// Parse a `text/uri-list` body per RFC 2483.
///
/// Lines starting with `#` are comments and are skipped.
/// Empty lines are skipped. Remaining lines are collected as URL strings.
pub fn parse_uri_list(content: &str) -> Vec<String> {
    content
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| line.to_string())
        .collect()
}

/// Extract the domain (host) from a URL string.
///
/// Returns `None` if parsing fails or the URL scheme has no host (e.g. `mailto:`).
pub fn extract_domain(url_str: &str) -> Option<String> {
    Url::parse(url_str)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

/// True when `candidate` is a single `http`/`https` URL with no surrounding or
/// internal whitespace.
fn is_web_url(candidate: &str) -> bool {
    let trimmed = candidate.trim();
    if trimmed.is_empty() || trimmed.contains(char::is_whitespace) {
        return false;
    }
    matches!(Url::parse(trimmed), Ok(u) if matches!(u.scheme(), "http" | "https"))
}

/// Detect the web URLs in a clipboard entry — the single contract shared by the
/// `link` tag rule and the `linkUrls` render metadata so the two never diverge.
///
/// Collects `http`/`https` URLs from two sources:
/// - `uri_list`: every entry whose scheme is `http`/`https` is kept (non-web
///   schemes such as `file://` or `mailto:` are dropped).
/// - `plain_text`: contributes its URLs **only** when every non-empty line is
///   itself a web URL (a single bare URL is the one-line case). Prose that
///   merely contains a URL contributes nothing.
///
/// Returns the URLs in the order encountered (uri-list first, then plain text);
/// empty when the entry holds no web URL.
pub fn detect_link_urls(uri_list: &[String], plain_text: Option<&str>) -> Vec<String> {
    let mut urls: Vec<String> = uri_list
        .iter()
        .map(|u| u.trim())
        .filter(|u| is_web_url(u))
        .map(|u| u.to_string())
        .collect();

    if let Some(text) = plain_text {
        let lines: Vec<&str> = text
            .lines()
            .map(|l| l.trim())
            .filter(|l| !l.is_empty())
            .collect();
        if !lines.is_empty() && lines.iter().all(|l| is_web_url(l)) {
            urls.extend(lines.into_iter().map(|l| l.to_string()));
        }
    }

    urls
}

#[cfg(test)]
mod link_detection_tests {
    use super::detect_link_urls;

    #[test]
    fn uri_list_keeps_only_web_urls() {
        let uris = vec![
            "https://example.com".to_string(),
            "http://a.test/path".to_string(),
            "mailto:x@y.z".to_string(),
            "ftp://files.test".to_string(),
        ];
        assert_eq!(
            detect_link_urls(&uris, None),
            vec![
                "https://example.com".to_string(),
                "http://a.test/path".to_string(),
            ]
        );
    }

    #[test]
    fn plain_text_single_url_detected() {
        assert_eq!(
            detect_link_urls(&[], Some("  https://example.com  ")),
            vec!["https://example.com".to_string()]
        );
    }

    #[test]
    fn plain_text_multi_line_all_urls_detected() {
        let text = "https://a.test\nhttp://b.test\n";
        assert_eq!(
            detect_link_urls(&[], Some(text)),
            vec!["https://a.test".to_string(), "http://b.test".to_string()]
        );
    }

    #[test]
    fn prose_containing_a_url_is_not_a_link() {
        assert!(detect_link_urls(&[], Some("see https://example.com for details")).is_empty());
    }

    #[test]
    fn plain_text_mixed_url_and_prose_lines_is_not_a_link() {
        let text = "https://a.test\njust some notes";
        assert!(detect_link_urls(&[], Some(text)).is_empty());
    }

    #[test]
    fn non_web_scheme_plain_text_is_not_a_link() {
        assert!(detect_link_urls(&[], Some("mailto:x@y.z")).is_empty());
        assert!(detect_link_urls(&[], Some("file:///etc/hosts")).is_empty());
    }

    #[test]
    fn empty_inputs_yield_no_urls() {
        assert!(detect_link_urls(&[], None).is_empty());
        assert!(detect_link_urls(&[], Some("   ")).is_empty());
    }
}
