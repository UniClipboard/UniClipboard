//! ICCCM / XDND / freedesktop atoms used by the X11 clipboard backend.
//!
//! Atom set mirrors what `clipboard_rs`'s X11 path interns plus the few extra
//! mime aliases the reader needs to recognize. The READ-path MIME predicates
//! (`format_id_for`, `is_interesting_mime`, `is_text_mime`, `rfc_mime_for`,
//! `text_mime_priority`) live in the shared `super::super::mime` module so
//! the X11 and Wayland backends stay in lockstep; this file keeps only the
//! interned atom set plus the write-side `default_mime_for_format` advertise
//! helper.

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
    // `format_id_for` moved to the shared read-path module; the write-side
    // round-trip below still needs it to assert advertise/parse symmetry.
    use super::super::super::mime::format_id_for;

    #[test]
    fn default_mime_round_trip() {
        for fid in ["text", "html", "rtf", "image", "files"] {
            let mime = default_mime_for_format(fid).unwrap();
            assert_eq!(format_id_for(mime), fid);
        }
        assert_eq!(default_mime_for_format("unknown"), None);
    }
}
