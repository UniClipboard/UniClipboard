//! Single-line `text/uri-list` parsing, shared by callers that need to
//! classify one line at a time (as opposed to collapsing a whole rep into a
//! deduped path list — see [`super::extract_file_paths_from_snapshot`]).

use std::path::PathBuf;

/// Classification of one `text/uri-list` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UriListLineKind {
    /// A `file://` URI that resolved to a local path.
    File(PathBuf),
    /// Empty, a `#` comment, or anything that isn't a resolvable `file://` URI.
    NonFile,
}

#[cfg(target_os = "macos")]
fn resolve_apfs_file_reference(_path: &std::path::Path) -> Option<PathBuf> {
    None
}

/// Parse a single raw `text/uri-list` line (untrimmed, as it appears in the
/// representation's inline text) into a [`UriListLineKind`].
pub(crate) fn parse_uri_list_line(raw_line: &str) -> UriListLineKind {
    let line = raw_line.trim();
    if line.is_empty() || line.starts_with('#') {
        return UriListLineKind::NonFile;
    }

    let Ok(url) = url::Url::parse(line) else {
        return UriListLineKind::NonFile;
    };
    if url.scheme() != "file" {
        return UriListLineKind::NonFile;
    }
    let Ok(path) = url.to_file_path() else {
        return UriListLineKind::NonFile;
    };

    #[cfg(target_os = "macos")]
    let resolved = resolve_apfs_file_reference(&path).unwrap_or(path);
    #[cfg(not(target_os = "macos"))]
    let resolved = path;

    UriListLineKind::File(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blank_and_comment_lines_are_non_file() {
        assert_eq!(parse_uri_list_line(""), UriListLineKind::NonFile);
        assert_eq!(parse_uri_list_line("   "), UriListLineKind::NonFile);
        assert_eq!(parse_uri_list_line("# a comment"), UriListLineKind::NonFile);
    }

    #[test]
    fn non_file_scheme_is_non_file() {
        assert_eq!(
            parse_uri_list_line("https://example.com/a"),
            UriListLineKind::NonFile
        );
    }

    #[test]
    fn unparseable_text_is_non_file() {
        assert_eq!(parse_uri_list_line("not a url"), UriListLineKind::NonFile);
    }

    #[test]
    fn file_uri_resolves_to_path() {
        let kind = parse_uri_list_line("file:///tmp/a.txt");
        assert_eq!(kind, UriListLineKind::File(PathBuf::from("/tmp/a.txt")));
    }
}
