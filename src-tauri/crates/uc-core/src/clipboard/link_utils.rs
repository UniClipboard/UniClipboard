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

#[cfg(test)]
mod tests {
    use super::*;

    // --- is_single_url ---

    #[test]
    fn is_single_url_https() {
        assert!(is_single_url("https://github.com"));
    }

    #[test]
    fn is_single_url_with_path_query_fragment() {
        assert!(is_single_url("https://github.com/repo?q=1#section"));
    }

    #[test]
    fn is_single_url_ftp() {
        assert!(is_single_url("ftp://files.example.com/doc.pdf"));
    }

    #[test]
    fn is_single_url_mailto() {
        assert!(is_single_url("mailto:user@example.com"));
    }

    #[test]
    fn is_single_url_with_whitespace_trimmed() {
        assert!(is_single_url("  https://github.com  "));
    }

    #[test]
    fn is_single_url_mixed_content_false() {
        assert!(!is_single_url("see https://github.com"));
    }

    #[test]
    fn is_single_url_plain_text_false() {
        assert!(!is_single_url("not a url"));
    }

    #[test]
    fn is_single_url_empty_false() {
        assert!(!is_single_url(""));
    }

    // --- parse_uri_list ---

    #[test]
    fn parse_uri_list_multiple_urls() {
        let result = parse_uri_list("https://a.com\nhttps://b.com");
        assert_eq!(result, vec!["https://a.com", "https://b.com"]);
    }

    #[test]
    fn parse_uri_list_with_comments_and_blanks() {
        let result = parse_uri_list("# comment\nhttps://a.com\n\nhttps://b.com");
        assert_eq!(result, vec!["https://a.com", "https://b.com"]);
    }

    // --- is_all_urls ---

    #[test]
    fn is_all_urls_multiline() {
        assert!(is_all_urls("https://a.com\nhttps://b.com\nhttps://c.com"));
    }

    #[test]
    fn is_all_urls_with_blank_lines() {
        assert!(is_all_urls("https://a.com\n\nhttps://b.com\n"));
    }

    #[test]
    fn is_all_urls_single_url() {
        assert!(is_all_urls("https://a.com"));
    }

    #[test]
    fn is_all_urls_mixed_content_false() {
        assert!(!is_all_urls("https://a.com\nnot a url\nhttps://b.com"));
    }

    #[test]
    fn is_all_urls_empty_false() {
        assert!(!is_all_urls(""));
    }

    // --- extract_domain ---

    #[test]
    fn extract_domain_https() {
        assert_eq!(
            extract_domain("https://github.com/repo"),
            Some("github.com".to_string())
        );
    }

    #[test]
    fn extract_domain_mailto_returns_none() {
        assert_eq!(extract_domain("mailto:user@example.com"), None);
    }
}
