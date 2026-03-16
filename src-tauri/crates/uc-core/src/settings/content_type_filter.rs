use super::model::ContentTypes;
use crate::clipboard::link_utils::{is_all_urls, is_single_url};
use crate::clipboard::SystemClipboardSnapshot;

/// Categories of clipboard content determined by MIME type analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentTypeCategory {
    Text,
    Image,
    RichText,
    Link,
    File,
    CodeSnippet,
    Unknown,
}

/// Classify a clipboard snapshot by examining its representations' MIME types.
///
/// File URIs are given highest priority: if **any** representation contains a
/// `text/uri-list` with `file://` URIs, the snapshot is classified as `File`
/// regardless of representation order.  This is necessary because macOS (and
/// some Linux DEs) place convenience representations (e.g. `image/png` for an
/// image file copy) *before* the `text/uri-list` representation, which would
/// otherwise cause the snapshot to be mis-classified as `Image`.
///
/// After the file-URI pre-scan, the remaining representations are checked in
/// order using first-match semantics.
///
/// For `text/uri-list`, the representation data is inspected to distinguish between
/// file URIs (`file://`) and web links (`http://`, `https://`, etc.).
pub fn classify_snapshot(snapshot: &SystemClipboardSnapshot) -> ContentTypeCategory {
    // Pre-scan: if any representation is a file URI list, this is a file copy.
    // This must happen before first-match iteration so that an `image/*`
    // representation placed earlier by the OS does not shadow the file URI.
    for rep in &snapshot.representations {
        if let Some(ref mime) = rep.mime {
            if mime.0.as_str() == "text/uri-list"
                && classify_uri_list(&rep.bytes) == ContentTypeCategory::File
            {
                return ContentTypeCategory::File;
            }
        }
    }

    for rep in &snapshot.representations {
        if let Some(ref mime) = rep.mime {
            let m = mime.0.as_str();
            // Order matters: check specific patterns before generic ones.
            // text/html and text/uri-list must match before the text/plain check.
            match m {
                "text/html" => return ContentTypeCategory::RichText,
                "text/uri-list" => return classify_uri_list(&rep.bytes),
                "text/plain" => {
                    // Check if the plain text content is URL(s)
                    if let Ok(text) = std::str::from_utf8(&rep.bytes) {
                        if is_single_url(text) || is_all_urls(text) {
                            return ContentTypeCategory::Link;
                        }
                    }
                    return ContentTypeCategory::Text;
                }
                "application/octet-stream" => return ContentTypeCategory::File,
                _ if m.starts_with("image/") => return ContentTypeCategory::Image,
                _ => {}
            }
        }
    }
    ContentTypeCategory::Unknown
}

/// Sub-classify a `text/uri-list` representation by inspecting the URI data.
///
/// Per RFC 2483, lines starting with `#` are comments and ignored.
/// The first non-empty, non-comment line determines classification:
/// - Starts with `file://` (case-insensitive) => `File`
/// - Otherwise => `Link`
/// - If data is not valid UTF-8 => `Link` (fallback)
fn classify_uri_list(bytes: &[u8]) -> ContentTypeCategory {
    let text = match std::str::from_utf8(bytes) {
        Ok(s) => s,
        Err(_) => return ContentTypeCategory::Link,
    };

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("file://") {
            return ContentTypeCategory::File;
        }
        return ContentTypeCategory::Link;
    }

    // No non-comment URIs found; default to Link
    ContentTypeCategory::Link
}

/// Check whether a content type category is allowed by the given content type toggles.
///
/// `Text`, `Image`, `File`, and `Link` are filterable. All other categories (including `Unknown`)
/// always return `true` — unimplemented types always sync.
pub fn is_content_type_allowed(category: ContentTypeCategory, ct: &ContentTypes) -> bool {
    match category {
        ContentTypeCategory::Text => ct.text,
        ContentTypeCategory::Image => ct.image,
        ContentTypeCategory::File => ct.file,
        ContentTypeCategory::Link => ct.link,
        // Unimplemented types always sync regardless of toggle state
        ContentTypeCategory::RichText
        | ContentTypeCategory::CodeSnippet
        | ContentTypeCategory::Unknown => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clipboard::{ObservedClipboardRepresentation, SystemClipboardSnapshot};
    use crate::ids::{FormatId, RepresentationId};
    use crate::settings::model::ContentTypes;
    use crate::MimeType;

    fn make_snapshot(mime: Option<&str>) -> SystemClipboardSnapshot {
        make_snapshot_with_data(mime, b"test data")
    }

    fn make_snapshot_with_data(mime: Option<&str>, data: &[u8]) -> SystemClipboardSnapshot {
        let reps = if let Some(m) = mime {
            vec![ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("test.format"),
                Some(MimeType(m.to_string())),
                data.to_vec(),
            )]
        } else {
            vec![]
        };
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: reps,
        }
    }

    fn make_multi_snapshot(mimes: &[&str]) -> SystemClipboardSnapshot {
        let reps = mimes
            .iter()
            .map(|m| {
                ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    FormatId::from("test.format"),
                    Some(MimeType(m.to_string())),
                    b"test data".to_vec(),
                )
            })
            .collect();
        SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: reps,
        }
    }

    // --- ContentTypes::default() tests ---

    #[test]
    fn content_types_default_returns_all_true() {
        let ct = ContentTypes::default();
        assert!(ct.text, "text should default to true");
        assert!(ct.image, "image should default to true");
        assert!(ct.link, "link should default to true");
        assert!(ct.file, "file should default to true");
        assert!(ct.code_snippet, "code_snippet should default to true");
        assert!(ct.rich_text, "rich_text should default to true");
    }

    // --- classify_snapshot tests ---

    #[test]
    fn classify_text_plain() {
        let snapshot = make_snapshot(Some("text/plain"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Text);
    }

    #[test]
    fn classify_image_png() {
        let snapshot = make_snapshot(Some("image/png"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Image);
    }

    #[test]
    fn classify_image_jpeg_wildcard() {
        let snapshot = make_snapshot(Some("image/jpeg"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Image);
    }

    #[test]
    fn classify_text_html_as_rich_text() {
        let snapshot = make_snapshot(Some("text/html"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::RichText);
    }

    #[test]
    fn classify_text_uri_list_with_http_as_link() {
        let snapshot = make_snapshot_with_data(Some("text/uri-list"), b"https://example.com\r\n");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Link);
    }

    #[test]
    fn classify_text_uri_list_with_file_uri_as_file() {
        let snapshot =
            make_snapshot_with_data(Some("text/uri-list"), b"file:///home/user/doc.pdf\r\n");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::File);
    }

    #[test]
    fn classify_text_uri_list_mixed_comment_and_file() {
        // Per RFC 2483, lines starting with # are comments
        let data = b"# This is a comment\r\nfile:///tmp/test.txt\r\nhttp://example.com\r\n";
        let snapshot = make_snapshot_with_data(Some("text/uri-list"), data);
        // First non-comment URI is file://, so should be File
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::File);
    }

    #[test]
    fn classify_text_uri_list_mixed_comment_and_http() {
        let data = b"# comment\r\nhttps://example.com\r\nfile:///tmp/test.txt\r\n";
        let snapshot = make_snapshot_with_data(Some("text/uri-list"), data);
        // First non-comment URI is https://, so should be Link
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Link);
    }

    #[test]
    fn classify_text_uri_list_non_utf8_falls_back_to_link() {
        let data: Vec<u8> = vec![0xFF, 0xFE, 0x00, 0x01];
        let snapshot = make_snapshot_with_data(Some("text/uri-list"), &data);
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Link);
    }

    #[test]
    fn classify_text_uri_list_file_case_insensitive() {
        let snapshot =
            make_snapshot_with_data(Some("text/uri-list"), b"FILE:///C:/Users/test.txt\r\n");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::File);
    }

    #[test]
    fn classify_application_octet_stream_as_file() {
        let snapshot = make_snapshot(Some("application/octet-stream"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::File);
    }

    #[test]
    fn classify_unknown_mime() {
        let snapshot = make_snapshot(Some("application/pdf"));
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Unknown);
    }

    #[test]
    fn classify_empty_representations() {
        let snapshot = make_snapshot(None);
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Unknown);
    }

    #[test]
    fn classify_multiple_representations_returns_first_recognized() {
        // image/png comes before text/plain, so Image should be returned
        let snapshot = make_multi_snapshot(&["image/png", "text/plain"]);
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Image);
    }

    #[test]
    fn classify_file_copy_with_image_before_uri_list() {
        // macOS places image/png before text/uri-list when copying an image file.
        // The snapshot should still be classified as File because the file URI
        // pre-scan takes priority over representation order.
        let reps = vec![
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("image"),
                Some(MimeType("image/png".to_string())),
                b"fake png data".to_vec(),
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("files"),
                Some(MimeType("text/uri-list".to_string())),
                b"file:///Users/test/photo.png\r\n".to_vec(),
            ),
        ];
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: reps,
        };
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::File);
    }

    #[test]
    fn classify_link_uri_list_with_image_stays_image() {
        // When text/uri-list contains http:// (not file://), the pre-scan
        // should NOT promote it to File — first-match should still apply.
        let reps = vec![
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("image"),
                Some(MimeType("image/png".to_string())),
                b"fake png data".to_vec(),
            ),
            ObservedClipboardRepresentation::new(
                RepresentationId::new(),
                FormatId::from("url"),
                Some(MimeType("text/uri-list".to_string())),
                b"https://example.com/image.png\r\n".to_vec(),
            ),
        ];
        let snapshot = SystemClipboardSnapshot {
            ts_ms: 1_713_000_000_000,
            representations: reps,
        };
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Image);
    }

    // --- is_content_type_allowed tests ---

    #[test]
    fn disallowed_text_when_text_false() {
        let ct = ContentTypes {
            text: false,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        };
        assert!(!is_content_type_allowed(ContentTypeCategory::Text, &ct));
    }

    #[test]
    fn disallowed_image_when_image_false() {
        let ct = ContentTypes {
            text: true,
            image: false,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        };
        assert!(!is_content_type_allowed(ContentTypeCategory::Image, &ct));
    }

    #[test]
    fn unimplemented_types_always_allowed() {
        // Even with all toggles false, unimplemented types always sync
        let ct = ContentTypes {
            text: false,
            image: false,
            link: false,
            file: false,
            code_snippet: false,
            rich_text: false,
        };
        assert!(is_content_type_allowed(ContentTypeCategory::RichText, &ct));
        assert!(is_content_type_allowed(
            ContentTypeCategory::CodeSnippet,
            &ct
        ));
        assert!(is_content_type_allowed(ContentTypeCategory::Unknown, &ct));
    }

    #[test]
    fn file_category_disallowed_when_file_false() {
        let ct = ContentTypes {
            text: true,
            image: true,
            link: true,
            file: false,
            code_snippet: true,
            rich_text: true,
        };
        assert!(!is_content_type_allowed(ContentTypeCategory::File, &ct));
    }

    #[test]
    fn file_category_allowed_when_file_true() {
        let ct = ContentTypes {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        };
        assert!(is_content_type_allowed(ContentTypeCategory::File, &ct));
    }

    // --- Plain-text URL detection in classify_snapshot ---

    #[test]
    fn classify_text_plain_single_url_as_link() {
        let snapshot = make_snapshot_with_data(Some("text/plain"), b"https://github.com");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Link);
    }

    #[test]
    fn classify_text_plain_non_url_as_text() {
        let snapshot = make_snapshot_with_data(Some("text/plain"), b"hello world");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Text);
    }

    #[test]
    fn classify_text_plain_mixed_content_as_text() {
        let snapshot = make_snapshot_with_data(Some("text/plain"), b"see https://github.com");
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Text);
    }

    #[test]
    fn classify_text_plain_multi_url_as_link() {
        let snapshot = make_snapshot_with_data(
            Some("text/plain"),
            b"https://a.com\nhttps://b.com\nhttps://c.com",
        );
        assert_eq!(classify_snapshot(&snapshot), ContentTypeCategory::Link);
    }

    // --- Link toggle filtering ---

    #[test]
    fn disallowed_link_when_link_false() {
        let ct = ContentTypes {
            text: true,
            image: true,
            link: false,
            file: true,
            code_snippet: true,
            rich_text: true,
        };
        assert!(!is_content_type_allowed(ContentTypeCategory::Link, &ct));
    }

    #[test]
    fn allowed_link_when_link_true() {
        let ct = ContentTypes {
            text: true,
            image: true,
            link: true,
            file: true,
            code_snippet: true,
            rich_text: true,
        };
        assert!(is_content_type_allowed(ContentTypeCategory::Link, &ct));
    }
}
