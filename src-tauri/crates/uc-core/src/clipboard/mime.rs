use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MimeType(pub String);

impl MimeType {
    pub fn text_plain() -> Self {
        Self("text/plain".into())
    }
    pub fn text_html() -> Self {
        Self("text/html".into())
    }
    pub fn text_markdown() -> Self {
        Self("text/markdown".into())
    }
    pub fn text_rtf() -> Self {
        Self("text/rtf".into())
    }
    pub fn text_xml() -> Self {
        Self("text/xml".into())
    }
    pub fn uri_list() -> Self {
        Self("text/uri-list".into())
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The `type/subtype` part of the MIME, lowercased with surrounding
    /// whitespace stripped. Parameters (`;charset=...`) are discarded.
    ///
    /// Comparing essences — rather than full MIME strings — is the only
    /// reliable way to classify a MIME, because real-world sources advertise
    /// the same type with arbitrary parameters and casing
    /// (`text/plain`, `text/plain;charset=utf-8`, `Text/Plain; charset="UTF-8"`).
    pub fn essence(&self) -> String {
        self.0
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
    }

    /// Lookup a parameter value (case-insensitive name match). Returns the
    /// raw value with surrounding whitespace and a single pair of double
    /// quotes stripped (no full RFC 2045 quoted-string parser; covers the
    /// 99% case of `charset="utf-8"`).
    pub fn parameter(&self, name: &str) -> Option<String> {
        let mut parts = self.0.split(';');
        let _ = parts.next();
        for raw in parts {
            let (k, v) = match raw.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            if k.trim().eq_ignore_ascii_case(name) {
                let v = v.trim();
                let unquoted = v
                    .strip_prefix('"')
                    .and_then(|s| s.strip_suffix('"'))
                    .unwrap_or(v);
                return Some(unquoted.to_string());
            }
        }
        None
    }

    /// Classify into a semantic bucket usable for all platform write
    /// decisions and application-layer rep filtering.
    ///
    /// Callers that care about specific subtypes (e.g. `image/png` vs
    /// `image/tiff`) should match on the returned [`MimeClass`] variants
    /// rather than reading the underlying string.
    pub fn classify(&self) -> MimeClass {
        MimeClass::from_essence(&self.essence())
    }

    pub fn is_text_plain(&self) -> bool {
        matches!(self.classify(), MimeClass::TextPlain)
    }
    pub fn is_text_html(&self) -> bool {
        matches!(self.classify(), MimeClass::TextHtml)
    }
    pub fn is_text_rtf(&self) -> bool {
        matches!(self.classify(), MimeClass::TextRtf)
    }
    pub fn is_uri_list(&self) -> bool {
        matches!(self.classify(), MimeClass::UriList)
    }
    pub fn is_image(&self) -> bool {
        matches!(self.classify(), MimeClass::Image(_))
    }
    pub fn is_text_like(&self) -> bool {
        matches!(
            self.classify(),
            MimeClass::TextPlain
                | MimeClass::TextHtml
                | MimeClass::TextRtf
                | MimeClass::TextMarkdown
                | MimeClass::UriList
                | MimeClass::TextLink
                | MimeClass::TextOther
        )
    }
}

impl fmt::Display for MimeType {
    /// Formats the MIME type by writing its inner string to the provided formatter.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::MimeType;
    /// let m = MimeType("text/plain".to_string());
    /// assert_eq!(format!("{}", m), "text/plain");
    /// ```
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for MimeType {
    type Err = anyhow::Error;

    /// Parses a MIME type string into a `MimeType` without performing validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use uc_core::MimeType;
    /// let m: MimeType = "text/plain".parse().unwrap();
    /// assert_eq!(m, MimeType("text/plain".to_string()));
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(MimeType(s.to_string()))
    }
}

impl std::ops::Deref for MimeType {
    type Target = String;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Semantic classification of a MIME type.
///
/// Use this — never the raw string — when making clipboard-routing
/// decisions. The set of variants is intentionally narrow: every variant
/// corresponds to a clipboard behavior the platform layer must support;
/// anything not listed is [`MimeClass::Unrecognized`] and must be handled
/// by callers (typically by refusing to write, per §11.2 of
/// `uc-platform/AGENTS.md`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MimeClass {
    /// `text/plain` (any charset variant) and the `public.utf8-plain-text`
    /// UTI (treated as plain text for clipboard purposes).
    TextPlain,
    TextHtml,
    TextRtf,
    TextMarkdown,
    /// `text/uri-list` (RFC 2483) and the historical `file/uri-list`
    /// variant — both carry newline-separated `file://` or `http(s)://`
    /// URIs and must be treated as a file list for clipboard purposes.
    UriList,
    /// `text/x-uri`, `text/x-url`, `text/uri` — link-bearing text MIMEs
    /// distinct from `text/uri-list`'s file-list semantics.
    TextLink,
    /// Any `text/*` subtype that doesn't match a more specific variant
    /// above (e.g. `text/csv`, `text/yaml`, `text/javascript`).
    TextOther,
    Image(ImageKind),
    /// `application/octet-stream` — present here so callers can match it
    /// explicitly and apply byte-sniffing recovery instead of refusing.
    OctetStream,
    /// MIME doesn't match any clipboard-relevant category. Platform write
    /// paths must refuse this rather than guess.
    Unrecognized,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImageKind {
    Png,
    Jpeg,
    Tiff,
    Gif,
    Webp,
    Bmp,
    /// `image/*` of an unknown subtype.
    Other,
}

impl MimeClass {
    /// Classify an already-lowercased essence string (no parameters).
    fn from_essence(essence: &str) -> Self {
        // Plain text covers both the standard MIME and the macOS UTI that
        // sometimes shows up in `mime` fields (notably when an upstream
        // adapter passes the UTI through verbatim).
        match essence {
            "text/plain" | "public.utf8-plain-text" | "public.text" => return Self::TextPlain,
            "text/html" => return Self::TextHtml,
            "text/rtf" | "application/rtf" => return Self::TextRtf,
            "text/markdown" => return Self::TextMarkdown,
            "text/uri-list" | "file/uri-list" => return Self::UriList,
            "text/x-uri" | "text/x-url" | "text/uri" => return Self::TextLink,
            "application/octet-stream" => return Self::OctetStream,
            "image/png" => return Self::Image(ImageKind::Png),
            "image/jpeg" | "image/jpg" => return Self::Image(ImageKind::Jpeg),
            "image/tiff" | "image/tif" => return Self::Image(ImageKind::Tiff),
            "image/gif" => return Self::Image(ImageKind::Gif),
            "image/webp" => return Self::Image(ImageKind::Webp),
            "image/bmp" | "image/x-bmp" => return Self::Image(ImageKind::Bmp),
            _ => {}
        }

        if let Some(rest) = essence.strip_prefix("image/") {
            // Defensive: covers any `image/*` subtype we didn't enumerate
            // above. Real-world image payloads should still be carried as
            // PNG by the time they reach the platform layer (image
            // normalization happens upstream), but a non-PNG `image/*`
            // arriving here is still recognizable as an image.
            let _ = rest;
            return Self::Image(ImageKind::Other);
        }
        if essence.starts_with("text/") {
            return Self::TextOther;
        }
        Self::Unrecognized
    }
}

/// Single source of truth: platform format identifiers (NSPasteboard UTIs,
/// X11/Wayland MIME atoms, internal short tags like `"text"` / `"image"`)
/// to their default MIME type.
///
/// Used by all platform write paths to decide a `rep`'s effective MIME
/// when its `mime` field is absent. Returning [`None`] means the
/// format identifier is not recognized as carrying a clipboard-relevant
/// payload — callers must treat this as "refuse to write" rather than
/// guessing.
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
    fn essence_strips_parameters_and_normalizes_case() {
        assert_eq!(MimeType("text/plain".into()).essence(), "text/plain");
        assert_eq!(
            MimeType("text/plain;charset=utf-8".into()).essence(),
            "text/plain"
        );
        assert_eq!(
            MimeType("Text/Plain; Charset=UTF-8".into()).essence(),
            "text/plain"
        );
        assert_eq!(
            MimeType("  text/plain  ;  charset = utf-8  ".into()).essence(),
            "text/plain"
        );
        assert_eq!(
            MimeType("text/plain;charset=\"utf-8\"".into()).essence(),
            "text/plain"
        );
    }

    #[test]
    fn parameter_lookup_is_case_insensitive_and_unquotes() {
        let m = MimeType("text/plain;charset=utf-8".into());
        assert_eq!(m.parameter("charset"), Some("utf-8".into()));
        assert_eq!(m.parameter("CHARSET"), Some("utf-8".into()));
        assert_eq!(m.parameter("boundary"), None);

        let m = MimeType("text/plain; charset=\"UTF-8\"".into());
        assert_eq!(m.parameter("charset"), Some("UTF-8".into()));
    }

    #[test]
    fn classify_text_plain_variants() {
        // The exact regression that motivated this work: Linux upstream
        // changed text mime to `text/plain;charset=utf-8`, and the macOS
        // write path's `Some("text/plain") =>` arm missed it, dropping
        // the rep into the `set_buffer` fallback and breaking paste.
        for s in [
            "text/plain",
            "text/plain;charset=utf-8",
            "Text/Plain; Charset=UTF-8",
            "  text/plain ; charset = \"utf-8\" ",
            "TEXT/PLAIN",
            "public.utf8-plain-text",
        ] {
            assert_eq!(
                MimeType(s.into()).classify(),
                MimeClass::TextPlain,
                "expected TextPlain for {s:?}"
            );
        }
    }

    #[test]
    fn classify_html_rtf_uri_link() {
        assert_eq!(MimeType("text/html".into()).classify(), MimeClass::TextHtml);
        assert_eq!(
            MimeType("text/html;charset=utf-8".into()).classify(),
            MimeClass::TextHtml
        );
        assert_eq!(MimeType("text/rtf".into()).classify(), MimeClass::TextRtf);
        assert_eq!(
            MimeType("application/rtf".into()).classify(),
            MimeClass::TextRtf
        );
        assert_eq!(
            MimeType("text/uri-list".into()).classify(),
            MimeClass::UriList
        );
        assert_eq!(
            MimeType("file/uri-list".into()).classify(),
            MimeClass::UriList
        );
        assert_eq!(
            MimeType("text/x-url".into()).classify(),
            MimeClass::TextLink
        );
    }

    #[test]
    fn classify_image_subtypes() {
        assert_eq!(
            MimeType("image/png".into()).classify(),
            MimeClass::Image(ImageKind::Png)
        );
        assert_eq!(
            MimeType("Image/PNG".into()).classify(),
            MimeClass::Image(ImageKind::Png)
        );
        assert_eq!(
            MimeType("image/jpeg".into()).classify(),
            MimeClass::Image(ImageKind::Jpeg)
        );
        assert_eq!(
            MimeType("image/jpg".into()).classify(),
            MimeClass::Image(ImageKind::Jpeg)
        );
        assert_eq!(
            MimeType("image/tiff".into()).classify(),
            MimeClass::Image(ImageKind::Tiff)
        );
        assert_eq!(
            MimeType("image/heic".into()).classify(),
            MimeClass::Image(ImageKind::Other)
        );
    }

    #[test]
    fn classify_text_other_and_unrecognized() {
        assert_eq!(MimeType("text/csv".into()).classify(), MimeClass::TextOther);
        assert_eq!(
            MimeType("text/yaml".into()).classify(),
            MimeClass::TextOther
        );
        assert_eq!(
            MimeType("application/octet-stream".into()).classify(),
            MimeClass::OctetStream
        );
        assert_eq!(
            MimeType("application/json".into()).classify(),
            MimeClass::Unrecognized
        );
        assert_eq!(MimeType("".into()).classify(), MimeClass::Unrecognized);
    }

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

    #[test]
    fn convenience_predicates_agree_with_classify() {
        let m = MimeType("text/plain;charset=utf-8".into());
        assert!(m.is_text_plain());
        assert!(m.is_text_like());
        assert!(!m.is_image());

        let m = MimeType("image/png".into());
        assert!(m.is_image());
        assert!(!m.is_text_plain());
    }
}
