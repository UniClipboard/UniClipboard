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
