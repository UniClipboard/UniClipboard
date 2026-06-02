//! ICCCM / XDND / freedesktop atoms used by the X11 clipboard backend.
//!
//! Atom set mirrors what `clipboard_rs`'s X11 path interns plus the few extra
//! mime aliases the reader needs to recognize as "this is text" / "this is
//! html" so that snapshot building is symmetric with the Wayland backend's
//! `snapshot::is_interesting_mime` filter.

use x11rb::atom_manager;

atom_manager! {
    /// Interned atom set shared between reader, writer, and watcher. Each
    /// `X11Server::connect` interns the full set in one round-trip via
    /// `Atoms::new(&conn)?.reply()?`.
    pub(super) Atoms: AtomCookies {
        // ICCCM selection atoms.
        CLIPBOARD,
        TARGETS,
        INCR,
        TIMESTAMP,
        MULTIPLE,
        SAVE_TARGETS,
        ATOM,

        // Text mimes (ICCCM + freedesktop).
        UTF8_STRING,
        STRING,
        TEXT,
        TEXT_PLAIN: b"text/plain",
        TEXT_PLAIN_UTF8: b"text/plain;charset=utf-8",
        TEXT_PLAIN_UTF8_BIG: b"text/plain;charset=UTF-8",

        // Rich text / HTML.
        TEXT_HTML: b"text/html",
        TEXT_RTF: b"text/rtf",

        // URI list (file drops).
        TEXT_URI_LIST: b"text/uri-list",
        // GNOME/Nautilus extensions — we recognize but don't actively serve.
        GNOME_COPIED_FILES: b"x-special/gnome-copied-files",

        // Image — we serve PNG by default; readers may surface other image/*
        // mimes opportunistically via the dynamic per-mime atom lookup.
        IMAGE_PNG: b"image/png",

        // Property name we use for our own convert_selection round-trip.
        // ICCCM only requires that requestor + property are unique per
        // outstanding request; we use a fixed name on a fresh window so we
        // don't need any per-request bookkeeping.
        UC_CLIPBOARD_PROP: b"_UC_CLIPBOARD_PAYLOAD",
    }
}

/// Format ID assigned to a representation derived from a given X11 mime atom name.
///
/// Kept aligned with `wayland::snapshot::format_id_for` so the downstream
/// dedup / sync logic doesn't need a per-platform branch.
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
        "text/uri-list" => "files",
        s if s.starts_with("image/") => "image",
        _ => "raw",
    }
}

/// Is this MIME one we want to capture into a snapshot? Kept symmetric with
/// `wayland::snapshot::is_interesting_mime`.
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
    ) || mime.starts_with("image/")
}

/// Once we've captured a text representation, skip the aliased text atoms —
/// the compositor / source typically advertises STRING + UTF8_STRING +
/// text/plain;charset=utf-8 as duplicates of the same bytes, and we don't
/// want N copies in the snapshot.
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
/// `UTF8_STRING` in their `TARGETS` reply. If we honored that order we'd
/// read the `STRING` copy first — which for URLs containing non-ASCII
/// characters (e.g. `http://host/job/玉兔Pro/`) is the percent-encoded
/// `http://host/job/%E7%8E%89%E5%85%94Pro/` fallback the source emits as
/// the 7-bit-safe variant, leaving the user's paste / sync target with the
/// `%XX` form. By sorting text candidates with this key before reading, we
/// always reach for the UTF-8 native variant first regardless of source
/// ordering.
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

/// Translate an X11 atom name to an RFC MIME type.
///
/// Platform-native targets (`UTF8_STRING`, `STRING`, `TEXT`) are not valid
/// RFC media types; they must be mapped before constructing a
/// `MimeType`. Returns `None` for names that are already RFC-shaped — the
/// caller should use the original string.
pub(super) fn rfc_mime_for(atom_name: &str) -> Option<&'static str> {
    match atom_name {
        "UTF8_STRING" | "STRING" | "TEXT" => Some("text/plain"),
        _ => None,
    }
}

/// Map a snapshot `format_id` → the canonical X11 mime atom name we advertise.
/// Mirrors `wayland::protocol::default_mime_for_format`.
pub(super) fn default_mime_for_format(format_id: &str) -> Option<&'static str> {
    match format_id {
        "text" => Some("text/plain;charset=utf-8"),
        "html" => Some("text/html"),
        "rtf" => Some("text/rtf"),
        "image" => Some("image/png"),
        "files" => Some("text/uri-list"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interesting_mime_includes_text_html_uri_image() {
        assert!(is_interesting_mime("text/plain"));
        assert!(is_interesting_mime("UTF8_STRING"));
        assert!(is_interesting_mime("text/html"));
        assert!(is_interesting_mime("text/uri-list"));
        assert!(is_interesting_mime("image/png"));
        assert!(is_interesting_mime("image/jpeg"));
    }

    #[test]
    fn interesting_mime_excludes_unknown() {
        assert!(!is_interesting_mime("application/octet-stream"));
        assert!(!is_interesting_mime("x-special/gnome-copied-files"));
    }

    #[test]
    fn format_id_mapping_matches_wayland() {
        assert_eq!(format_id_for("text/plain"), "text");
        assert_eq!(format_id_for("UTF8_STRING"), "text");
        assert_eq!(format_id_for("text/html"), "html");
        assert_eq!(format_id_for("text/rtf"), "rtf");
        assert_eq!(format_id_for("text/uri-list"), "files");
        assert_eq!(format_id_for("image/png"), "image");
        assert_eq!(format_id_for("image/jpeg"), "image");
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
    fn default_mime_round_trip() {
        for fid in ["text", "html", "rtf", "image", "files"] {
            let mime = default_mime_for_format(fid).unwrap();
            assert_eq!(format_id_for(mime), fid);
        }
        assert_eq!(default_mime_for_format("unknown"), None);
    }
}
