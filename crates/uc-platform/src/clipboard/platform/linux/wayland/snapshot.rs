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

use super::super::mime::{
    format_id_for, is_interesting_mime, is_text_mime, read_mime_sort_key, rfc_mime_for,
};
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

    // Stable-sort the interesting mimes so UTF-8 text variants come before
    // Latin-1 fallbacks, and decodable `image/*` targets come before
    // undecodable ones (`image/png` over `image/xpm`, issue #1029). See
    // `read_mime_sort_key` for the why. Mimes that tie on the key keep their
    // relative advertise-order position via the stable sort.
    let mut interesting_mimes: Vec<&String> =
        mimes.iter().filter(|m| is_interesting_mime(m)).collect();
    interesting_mimes.sort_by_key(|m| read_mime_sort_key(m.as_str()));

    for mime in interesting_mimes {
        // Skip secondary text mimes once we've captured a primary one — the
        // compositor often advertises STRING + UTF8_STRING + text/plain;charset=utf-8
        // as aliases of the same data, and reading all three would inflate
        // the snapshot with duplicates that downstream dedup wouldn't catch
        // (different format_id but same bytes).
        let is_text = is_text_mime(mime);
        if is_text && text_captured {
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
                if is_text {
                    text_captured = true;
                }
                if is_image_mime {
                    image_captured = true;
                }
                // Canonicalize platform-native targets (UTF8_STRING / STRING
                // / TEXT) to an RFC media type before storing — mirrors the
                // X11 reader. Never store a raw atom name as a MimeType:
                // fall back to the original only when it is already
                // RFC-shaped, otherwise omit the mime (None).
                let rfc_mime = match rfc_mime_for(mime) {
                    Some(canonical) => Some(MimeType(canonical.to_string())),
                    None if mime.contains('/') => Some(MimeType(mime.to_string())),
                    None => None,
                };
                reps.push(ObservedClipboardRepresentation::new(
                    RepresentationId::new(),
                    format_id_for(mime).into(),
                    rfc_mime,
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
