//! Build a `SystemClipboardSnapshot` from a Wayland `data_offer` plus its
//! advertised MIME list.
//!
//! Mime selection mirrors what `CommonClipboardImpl::read_snapshot` extracts
//! via `clipboard-rs` so downstream sync behavior stays consistent across
//! Linux Wayland / X11 / macOS / Windows: text + html + uri-list +
//! `image/*`. Anything else is ignored to keep the snapshot bounded — if a
//! future use case needs it we can widen the filter.
//!
//! Protocol-agnostic via [`super::backend::OfferLike`]; reused by both
//! `wlr-data-control` and `ext-data-control`.

use anyhow::Result;
use std::time::Duration;
use tracing::{debug, warn};
use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;
use wayland_client::Connection;

use super::backend::OfferLike;
use super::transfer;

/// 2 seconds matches the upper bound used by other Wayland clipboard
/// managers (klipper, copyq) before declaring a misbehaving source dead.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Hard upper bound per mime payload. The full snapshot can carry multiple
/// reps, so this is per-rep rather than per-snapshot. 32 MiB matches the
/// existing `MAX_IMAGE_FILE_BYTES` ceiling in `common.rs` plus a little
/// headroom for HTML/RTF that occasionally exceeds plain image budgets.
const MAX_MIME_BYTES: usize = 32 * 1024 * 1024;

fn is_interesting_mime(mime: &str) -> bool {
    matches!(
        mime,
        "text/plain"
            | "text/plain;charset=utf-8"
            | "UTF8_STRING"
            | "STRING"
            | "TEXT"
            | "text/html"
            | "text/uri-list"
            | "text/x-uri"
            | "text/x-moz-url"
    ) || mime.starts_with("image/")
}

/// Map a wayland MIME string to the internal `format_id` used by
/// `SystemClipboardSnapshot`. Keep these aligned with the format IDs that
/// `CommonClipboardImpl::read_snapshot` produces on other platforms so that
/// downstream dedup / sync logic doesn't need a per-platform branch.
fn format_id_for(mime: &str) -> &'static str {
    match mime {
        "text/plain" | "text/plain;charset=utf-8" | "UTF8_STRING" | "STRING" | "TEXT" => "text",
        "text/html" => "html",
        "text/uri-list" => "files",
        s if s.starts_with("image/") => "image",
        _ => "raw",
    }
}

pub(super) fn build_from_offer<O: OfferLike>(
    conn: &Connection,
    offer: &O,
    mimes: &[String],
) -> Result<SystemClipboardSnapshot> {
    debug!(
        offered_mimes = mimes.len(),
        "wayland: building snapshot from offer"
    );
    let mut reps = Vec::new();
    let mut text_captured = false;
    let mut image_captured = false;

    for mime in mimes {
        if !is_interesting_mime(mime) {
            continue;
        }
        // Skip secondary text mimes once we've captured a primary one — the
        // compositor often advertises STRING + UTF8_STRING + text/plain;charset=utf-8
        // as aliases of the same data, and reading all three would inflate
        // the snapshot with duplicates that downstream dedup wouldn't catch
        // (different format_id but same bytes).
        let is_text_mime = matches!(
            mime.as_str(),
            "text/plain" | "text/plain;charset=utf-8" | "UTF8_STRING" | "STRING" | "TEXT"
        );
        if is_text_mime && text_captured {
            continue;
        }
        let is_image_mime = mime.starts_with("image/");
        if is_image_mime && image_captured {
            continue;
        }

        match transfer::pipe_receive(conn, offer, mime, READ_TIMEOUT, MAX_MIME_BYTES) {
            Ok(bytes) => {
                debug!(
                    mime = %mime,
                    size = bytes.len(),
                    "wayland: read mime payload"
                );
                if is_text_mime {
                    text_captured = true;
                }
                if is_image_mime {
                    image_captured = true;
                }
                reps.push(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    format_id_for(mime).into(),
                    Some(MimeType(mime.to_string())),
                    bytes,
                ));
            }
            Err(e) => {
                warn!(mime = %mime, error = %e, "wayland: failed to read mime payload");
            }
        }
    }

    Ok(SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: reps,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interesting_mime_includes_text_html_uri_image() {
        assert!(is_interesting_mime("text/plain"));
        assert!(is_interesting_mime("text/plain;charset=utf-8"));
        assert!(is_interesting_mime("UTF8_STRING"));
        assert!(is_interesting_mime("text/html"));
        assert!(is_interesting_mime("text/uri-list"));
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
    fn format_id_mapping_matches_common_rs() {
        assert_eq!(format_id_for("text/plain"), "text");
        assert_eq!(format_id_for("UTF8_STRING"), "text");
        assert_eq!(format_id_for("text/html"), "html");
        assert_eq!(format_id_for("text/uri-list"), "files");
        assert_eq!(format_id_for("image/png"), "image");
        assert_eq!(format_id_for("image/jpeg"), "image");
    }
}
