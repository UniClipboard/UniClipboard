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
    fn default_mime_round_trip() {
        for fid in ["text", "html", "rtf", "image", "files"] {
            let mime = default_mime_for_format(fid).unwrap();
            assert_eq!(format_id_for(mime), fid);
        }
        assert_eq!(default_mime_for_format("unknown"), None);
    }
}
