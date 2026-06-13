//! Shared Linux clipboard READ-path MIME mapping.
//!
//! Both Linux backends — Wayland (`wlr-data-control` / `ext-data-control`)
//! and X11 (ICCCM selections) — observe the SAME set of clipboard MIME /
//! atom names and must map them to the SAME internal `format_id` and the
//! SAME RFC media type so downstream dedup / sync logic never needs a
//! per-backend branch. Historically each backend carried its own copy of
//! these helpers (`x11::atoms` and `wayland::snapshot`), and the two drifted
//! (different atom sets, a missing RFC translation on the Wayland side that
//! could push a non-RFC atom name into a `MimeType`). This module is the
//! single source of truth for the read path; both backends call into it.
//!
//! Ownership note (`uc-platform` AGENTS §6.4 / §9.2): platform-atom knowledge
//! (`UTF8_STRING`, `STRING`, `TEXT`, `text/plain;charset=UTF-8`, ...) stays
//! confined to this layer. The values returned upward are stable platform
//! semantics — internal `format_id` strings and RFC media types — never raw
//! OS atom names.
//!
//! Scope is the READ path only. The advertise/write helper
//! `default_mime_for_format` is intentionally NOT hoisted here: it has two
//! separate write-side homes (`x11::atoms` and `wayland::protocol`) and is
//! exercised by the writers, not the snapshot readers.

/// Map a clipboard MIME / X11 atom name to the internal `format_id` used by
/// `SystemClipboardSnapshot`. Kept aligned with the format IDs that
/// `CommonClipboardImpl::read_snapshot` produces on other platforms so that
/// downstream dedup / sync logic doesn't need a per-platform branch.
pub(super) fn format_id_for(mime: &str) -> &'static str {
    match mime {
        "text/plain"
        | "text/plain;charset=utf-8"
        | "text/plain;charset=UTF-8"
        | "UTF8_STRING"
        | "STRING"
        | "TEXT" => "text",
        "text/html" => "html",
        "text/rtf" => "rtf",
        "text/uri-list" | "text/x-uri" | "text/x-moz-url" => "files",
        s if s.starts_with("image/") => "image",
        _ => "raw",
    }
}

/// Is this MIME / atom one we want to capture into a snapshot? This is the
/// shared whitelist for both Linux backends. Every entry that is NOT a
/// platform-native atom (`UTF8_STRING` / `STRING` / `TEXT`) is already a
/// valid RFC media type, which is what keeps [`rfc_mime_for`]'s caller-side
/// fallback safe (see that fn's docs).
pub(super) fn is_interesting_mime(mime: &str) -> bool {
    matches!(
        mime,
        "text/plain"
            | "text/plain;charset=utf-8"
            | "text/plain;charset=UTF-8"
            | "UTF8_STRING"
            | "STRING"
            | "TEXT"
            | "text/html"
            | "text/rtf"
            | "text/uri-list"
            | "text/x-uri"
            | "text/x-moz-url"
    ) || mime.starts_with("image/")
}

/// Once we've captured a primary text representation, skip the aliased text
/// atoms — the compositor / selection owner typically advertises
/// `STRING` + `UTF8_STRING` + `text/plain;charset=utf-8` as duplicates of the
/// same bytes, and we don't want N copies in the snapshot. Returns true only
/// for the plain-text aliases (NOT `text/html` / `text/rtf` / `text/uri-list`,
/// which carry distinct payloads).
pub(super) fn is_text_mime(mime: &str) -> bool {
    matches!(
        mime,
        "text/plain"
            | "text/plain;charset=utf-8"
            | "text/plain;charset=UTF-8"
            | "UTF8_STRING"
            | "STRING"
            | "TEXT"
    )
}

/// Lower number = read first when the source advertises multiple aliased
/// text targets. ICCCM recommends `UTF8_STRING` / `text/plain;charset=utf-8`
/// over the Latin-1-bounded `STRING` and `TEXT`, but real-world sources
/// (Chromium, file managers, some IDE widgets) often list `STRING` *before*
/// `UTF8_STRING` in their `TARGETS` / offered-mimes reply. If we honored that
/// order we'd read the `STRING` copy first — which for URLs containing
/// non-ASCII characters (e.g. `http://host/job/玉兔Pro/`) is the
/// percent-encoded `http://host/job/%E7%8E%89%E5%85%94Pro/` fallback the
/// source emits as the 7-bit-safe variant, leaving the user's paste / sync
/// target with the `%XX` form. By sorting text candidates with this key
/// before reading, we always reach for the UTF-8 native variant first
/// regardless of source ordering.
pub(super) fn text_mime_priority(mime: &str) -> u32 {
    match mime {
        // Explicit UTF-8 plain-text MIMEs — always preferred when present.
        "text/plain;charset=utf-8" | "text/plain;charset=UTF-8" => 0,
        // ICCCM's UTF-8 atom — second-best (some legacy sources reorder it).
        "UTF8_STRING" => 1,
        // Charset-less `text/plain`; modern sources treat it as UTF-8, but
        // it's allowed to be Latin-1, so prefer the explicit variants above.
        "text/plain" => 2,
        // ICCCM defines `STRING` as Latin-1 only — sources commonly use it
        // for a 7-bit-safe (often percent-encoded) fallback. Read last.
        "STRING" => 3,
        // ICCCM `TEXT` is locale-encoded. Same story as `STRING`.
        "TEXT" => 4,
        // Non-text mimes — keep them after all text mimes.
        _ => u32::MAX,
    }
}

/// Translate a platform-native clipboard target name to an RFC MIME type.
///
/// The native targets `UTF8_STRING`, `STRING`, and `TEXT` are X11 ICCCM /
/// Wayland atom names, NOT valid RFC media types; they must be canonicalized
/// before constructing a `MimeType` so the value stored in a snapshot is a
/// real media type (e.g. text/plain), never an atom name. Returns `None` for
/// names that are already RFC-shaped — callers fall back to the original
/// string for those (every other [`is_interesting_mime`] entry is a valid
/// RFC media type, so that fallback is always RFC-valid).
pub(super) fn rfc_mime_for(atom_name: &str) -> Option<&'static str> {
    match atom_name {
        "UTF8_STRING" | "STRING" | "TEXT" => Some("text/plain"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interesting_mime_includes_text_html_uri_image() {
        assert!(is_interesting_mime("text/plain"));
        assert!(is_interesting_mime("text/plain;charset=utf-8"));
        assert!(is_interesting_mime("text/plain;charset=UTF-8"));
        assert!(is_interesting_mime("UTF8_STRING"));
        assert!(is_interesting_mime("text/html"));
        assert!(is_interesting_mime("text/rtf"));
        assert!(is_interesting_mime("text/uri-list"));
        assert!(is_interesting_mime("text/x-uri"));
        assert!(is_interesting_mime("text/x-moz-url"));
        assert!(is_interesting_mime("image/png"));
        assert!(is_interesting_mime("image/jpeg"));
        assert!(is_interesting_mime("image/svg+xml"));
    }

    #[test]
    fn interesting_mime_excludes_unknown() {
        assert!(!is_interesting_mime("application/octet-stream"));
        assert!(!is_interesting_mime("x-special/gnome-copied-files"));
        assert!(!is_interesting_mime("application/x-kde4-urilist"));
    }

    #[test]
    fn format_id_mapping_is_stable() {
        assert_eq!(format_id_for("text/plain"), "text");
        assert_eq!(format_id_for("text/plain;charset=UTF-8"), "text");
        assert_eq!(format_id_for("UTF8_STRING"), "text");
        assert_eq!(format_id_for("text/html"), "html");
        assert_eq!(format_id_for("text/rtf"), "rtf");
        assert_eq!(format_id_for("text/uri-list"), "files");
        assert_eq!(format_id_for("text/x-uri"), "files");
        assert_eq!(format_id_for("text/x-moz-url"), "files");
        assert_eq!(format_id_for("image/png"), "image");
        assert_eq!(format_id_for("image/jpeg"), "image");
        assert_eq!(format_id_for("application/octet-stream"), "raw");
    }

    #[test]
    fn text_mime_predicate_covers_plain_text_aliases_only() {
        for m in [
            "text/plain",
            "text/plain;charset=utf-8",
            "text/plain;charset=UTF-8",
            "UTF8_STRING",
            "STRING",
            "TEXT",
        ] {
            assert!(is_text_mime(m), "{m} should be a text mime");
        }
        for m in ["text/html", "text/rtf", "text/uri-list", "image/png"] {
            assert!(!is_text_mime(m), "{m} should not be a text mime");
        }
    }

    #[test]
    fn text_mime_priority_prefers_utf8_variants() {
        // Strict ordering: explicit UTF-8 < UTF8_STRING < text/plain < STRING < TEXT.
        assert!(text_mime_priority("text/plain;charset=utf-8") < text_mime_priority("UTF8_STRING"));
        assert!(text_mime_priority("text/plain;charset=UTF-8") < text_mime_priority("UTF8_STRING"));
        assert!(text_mime_priority("UTF8_STRING") < text_mime_priority("text/plain"));
        assert!(text_mime_priority("text/plain") < text_mime_priority("STRING"));
        assert!(text_mime_priority("STRING") < text_mime_priority("TEXT"));
    }

    #[test]
    fn text_mime_priority_demotes_non_text_to_back() {
        // Non-text MIMEs must sort after every text MIME so that the
        // reorder pass never moves them in front of a text variant.
        let last_text = text_mime_priority("TEXT");
        for non_text in ["text/html", "text/uri-list", "image/png", "x-special/foo"] {
            assert!(
                text_mime_priority(non_text) > last_text,
                "non-text mime {non_text} ranked above text"
            );
        }
    }

    #[test]
    fn sort_pulls_utf8_text_mime_in_front_of_string() {
        // Simulates a source that advertises STRING (percent-encoded URL)
        // before UTF8_STRING (UTF-8 original) — a documented Chromium
        // pattern. After sorting, UTF8_STRING must come first so we read
        // the UTF-8 variant and never touch the percent-encoded copy.
        let mut mimes: Vec<&str> = vec!["STRING", "UTF8_STRING", "text/html", "image/png"];
        mimes.sort_by_key(|m| text_mime_priority(m));
        assert_eq!(mimes[0], "UTF8_STRING");
        assert_eq!(mimes[1], "STRING");
        // Non-text mimes retained but pushed behind every text variant.
        assert!(mimes.contains(&"text/html"));
        assert!(mimes.contains(&"image/png"));
    }

    #[test]
    fn rfc_mime_canonicalizes_native_targets_only() {
        assert_eq!(rfc_mime_for("UTF8_STRING"), Some("text/plain"));
        assert_eq!(rfc_mime_for("STRING"), Some("text/plain"));
        assert_eq!(rfc_mime_for("TEXT"), Some("text/plain"));
        // Already-RFC names are passed through unchanged (None => keep original).
        assert_eq!(rfc_mime_for("text/plain"), None);
        assert_eq!(rfc_mime_for("text/html"), None);
        assert_eq!(rfc_mime_for("image/png"), None);
    }

    #[test]
    fn every_interesting_non_native_mime_is_rfc_shaped() {
        // Robustness guard: the read-path call sites fall back to the
        // original MIME string when `rfc_mime_for` returns None. That is
        // only safe if every non-native interesting MIME is already a
        // valid RFC media type (i.e. contains a '/'). Native atoms
        // (UTF8_STRING/STRING/TEXT) are exempt because rfc_mime_for
        // rewrites them.
        for m in [
            "text/plain",
            "text/plain;charset=utf-8",
            "text/plain;charset=UTF-8",
            "text/html",
            "text/rtf",
            "text/uri-list",
            "text/x-uri",
            "text/x-moz-url",
            "image/png",
            "image/jpeg",
        ] {
            assert!(is_interesting_mime(m), "{m} should be interesting");
            if rfc_mime_for(m).is_none() {
                assert!(
                    m.contains('/'),
                    "non-native interesting mime {m} must be RFC-shaped"
                );
            }
        }
    }
}
