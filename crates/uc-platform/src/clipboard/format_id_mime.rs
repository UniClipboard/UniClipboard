//! Translate platform-native clipboard format identifiers to RFC media types.
//!
//! This is the engine-edge translation table for desktop hosts: macOS UTIs
//! (`public.png`, `NSStringPboardType`), Linux's project-internal short
//! tags (`text`, `html`, `image`, `files`), and Windows clipboard short
//! tags all map to an RFC MIME (`text/plain`, `image/png`, ...).
//!
//! The engine layer (`uc-core`) intentionally does not know about
//! `public.*` UTI strings or Windows `CF_*` short tags. Capture adapters
//! and write paths invoke this function at the platform/engine boundary
//! so the engine only ever sees RFC media types.
//!
//! When future mobile hosts (`uc-platform-ios`, `uc-platform-android`)
//! are added, each provides its own translation table — UTType IDs on
//! iOS map to MIME, Android `ClipDescription` MIMEs are already RFC.

use uc_core::MimeType;

/// Translate a platform-native format identifier to its default RFC MIME.
///
/// Used by platform write paths to resolve a `rep`'s effective MIME when
/// the rep arrived with `mime: None`. Returning [`None`] means the
/// identifier is not recognized as carrying a clipboard-relevant payload;
/// callers must treat that as "refuse to write" rather than guessing.
///
/// Inputs accepted today (desktop hosts):
/// - macOS UTIs: `public.utf8-plain-text`, `public.text`, `public.html`,
///   `public.rtf`, `public.png`, `public.tiff`, `public.jpeg`,
///   `public.file-url`, plus the legacy NeXTSTEP names
///   `NSStringPboardType` / `NSFilenamesPboardType` and
///   `Apple HTML pasteboard type`.
/// - Project-internal short tags emitted by Linux capture and used by
///   shared infrastructure: `text`, `html`, `rtf`, `image`, `files`.
pub fn format_id_default_mime(format_id: &str) -> Option<MimeType> {
    let normalized = format_id.trim().to_ascii_lowercase();
    let s: &str = match normalized.as_str() {
        // Text plain (UTIs, Windows clipboard short tag, generic "text")
        "public.utf8-plain-text" | "public.text" | "nsstringpboardtype" | "text" => "text/plain",
        // HTML
        "public.html" | "apple html pasteboard type" | "html" => "text/html",
        // RTF
        "public.rtf" | "rtf" => "text/rtf",
        // Image: format_id "image" is the project-internal canonical
        // identifier for image reps; both `image` and `public.png`
        // default to PNG because image normalization upstream re-encodes
        // everything to PNG (see uc-infra/src/clipboard/background_blob_worker.rs).
        "public.png" | "image" => "image/png",
        "public.tiff" => "image/tiff",
        "public.jpeg" | "public.jpg" => "image/jpeg",
        // File URI list
        "public.file-url" | "nsfilenamespboardtype" | "files" => "text/uri-list",
        _ => return None,
    };
    Some(MimeType(s.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_id_default_mime_handles_known_ids() {
        assert_eq!(
            format_id_default_mime("text"),
            Some(MimeType("text/plain".into()))
        );
        assert_eq!(
            format_id_default_mime("public.utf8-plain-text"),
            Some(MimeType("text/plain".into()))
        );
        assert_eq!(
            format_id_default_mime("NSStringPboardType"),
            Some(MimeType("text/plain".into()))
        );
        assert_eq!(
            format_id_default_mime("html"),
            Some(MimeType("text/html".into()))
        );
        assert_eq!(
            format_id_default_mime("Apple HTML pasteboard type"),
            Some(MimeType("text/html".into()))
        );
        assert_eq!(
            format_id_default_mime("image"),
            Some(MimeType("image/png".into()))
        );
        assert_eq!(
            format_id_default_mime("public.png"),
            Some(MimeType("image/png".into()))
        );
        assert_eq!(
            format_id_default_mime("public.tiff"),
            Some(MimeType("image/tiff".into()))
        );
        assert_eq!(
            format_id_default_mime("files"),
            Some(MimeType("text/uri-list".into()))
        );
        assert_eq!(
            format_id_default_mime("public.file-url"),
            Some(MimeType("text/uri-list".into()))
        );
        // Trim + case fold so callers don't need to pre-normalize.
        assert_eq!(
            format_id_default_mime("  HTML  "),
            Some(MimeType("text/html".into()))
        );
        // Unknown identifier returns None — callers must refuse to write.
        assert_eq!(format_id_default_mime("application/foo"), None);
        assert_eq!(format_id_default_mime("unknown-format"), None);
    }
}
