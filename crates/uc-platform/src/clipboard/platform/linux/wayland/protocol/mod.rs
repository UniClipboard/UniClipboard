//! Per-protocol Wayland data-control backends.
//!
//! Two protocols target the same use case:
//!
//! - `wlr-data-control-unstable-v1` ([`wlr`]) — niri / sway / hyprland /
//!   KDE Plasma 5+ / wlroots-based compositors.
//! - `ext-data-control-v1` ([`ext`]) — GNOME mutter ≥ 47, KDE Plasma 6, and
//!   recent wlroots compositors that have caught up with the standardized
//!   protocol.
//!
//! Many compositors advertise both. [`select`] picks one with the rule:
//!
//! 1. If `UC_FORCE_DATA_CONTROL=ext|wlr` is set, use that and only that.
//! 2. Otherwise prefer `ext` (the standardized future-proof choice), fall
//!    back to `wlr` (broader real-world coverage today).
//!
//! Override is meant for development verification — niri exposes both
//! protocols, so flipping the env var lets us exercise the non-default code
//! path without leaving the Wayland session.
//!
//! ## Why probe both
//!
//! Mutter advertises only `ext`. Older sway/wlroots advertise only `wlr`.
//! Newer everything advertises both. We have to probe what's actually there
//! before we commit to a backend.

pub(super) mod ext;
pub(super) mod wlr;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};
use wayland_client::Connection;

use super::clipboard::{WaylandClipboard, WaylandClipboardInner};
use super::event_loop::{WaylandEventLoop, WaylandEventLoopInner};

const FORCE_ENV: &str = "UC_FORCE_DATA_CONTROL";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Protocol {
    Wlr,
    Ext,
}

fn forced_choice() -> Option<Protocol> {
    let raw = std::env::var(FORCE_ENV).ok()?;
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.as_str() {
        "ext" | "ext-data-control" | "ext-data-control-v1" => Some(Protocol::Ext),
        "wlr" | "wlr-data-control" | "wlr-data-control-unstable-v1" => Some(Protocol::Wlr),
        other => {
            warn!(
                value = other,
                env = FORCE_ENV,
                "unknown UC_FORCE_DATA_CONTROL value; ignoring (expected 'ext' or 'wlr')"
            );
            None
        }
    }
}

/// Select a backend for the given connection. Returns `Ok(None)` if neither
/// protocol is advertised (caller falls back to legacy/X11).
fn select(conn: &Connection) -> Result<Option<Protocol>> {
    let force = forced_choice();
    let ext_available = ext::probe(conn).context("ext-data-control probe failed")?;
    let wlr_available = wlr::probe(conn).context("wlr-data-control probe failed")?;

    info!(
        ext = ext_available,
        wlr = wlr_available,
        force = ?force,
        "wayland data-control protocol probe"
    );

    if let Some(forced) = force {
        match forced {
            Protocol::Ext if ext_available => return Ok(Some(Protocol::Ext)),
            Protocol::Wlr if wlr_available => return Ok(Some(Protocol::Wlr)),
            other => {
                warn!(
                    forced = ?other,
                    "UC_FORCE_DATA_CONTROL set but the requested protocol is not advertised; \
                     falling through to default selection"
                );
            }
        }
    }

    if ext_available {
        return Ok(Some(Protocol::Ext));
    }
    if wlr_available {
        return Ok(Some(Protocol::Wlr));
    }
    Ok(None)
}

/// Connect to the wayland session and bring up an event-loop backend.
///
/// - `Ok(Some(_))` — backend ready; caller drives `run()`.
/// - `Ok(None)` — connect succeeded but no data-control protocol advertised.
/// - `Err(_)` — connect failed.
pub(crate) fn try_new_event_loop() -> Result<Option<WaylandEventLoop>> {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "wayland: cannot connect; skipping wayland event loop");
            return Ok(None);
        }
    };

    let proto = match select(&conn)? {
        Some(p) => p,
        None => {
            debug!("wayland: no data-control protocol advertised; skipping wayland event loop");
            return Ok(None);
        }
    };

    match proto {
        Protocol::Ext => {
            info!("wayland event loop: ext-data-control");
            Ok(Some(WaylandEventLoop {
                inner: WaylandEventLoopInner::Ext(ext::ExtEventLoop::with_connection(conn)),
            }))
        }
        Protocol::Wlr => {
            info!("wayland event loop: wlr-data-control");
            Ok(Some(WaylandEventLoop {
                inner: WaylandEventLoopInner::Wlr(wlr::WlrEventLoop::with_connection(conn)),
            }))
        }
    }
}

/// Connect to the wayland session and bring up a clipboard read/write
/// backend.
pub(crate) fn try_new_clipboard() -> Result<Option<WaylandClipboard>> {
    let conn = match Connection::connect_to_env() {
        Ok(c) => c,
        Err(e) => {
            warn!(error = %e, "wayland: cannot connect; skipping wayland clipboard");
            return Ok(None);
        }
    };

    let proto = match select(&conn)? {
        Some(p) => p,
        None => {
            debug!("wayland: no data-control protocol advertised; skipping wayland clipboard");
            return Ok(None);
        }
    };

    match proto {
        Protocol::Ext => {
            info!("wayland clipboard: ext-data-control");
            let c = ext::ExtClipboard::spawn(conn)?;
            Ok(Some(WaylandClipboard {
                inner: WaylandClipboardInner::Ext(c),
            }))
        }
        Protocol::Wlr => {
            info!("wayland clipboard: wlr-data-control");
            let c = wlr::WlrClipboard::spawn(conn)?;
            Ok(Some(WaylandClipboard {
                inner: WaylandClipboardInner::Wlr(c),
            }))
        }
    }
}

/// Coarse `format_id` → mime mapping mirroring the writer side of
/// `CommonClipboardImpl::write_snapshot`. Falls back to `None` for unknown
/// `format_id`s; unknown reps are skipped (caller surfaces the error).
///
/// Shared between [`wlr`] and [`ext`] write paths.
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

/// MIME aliases advertised for a UTF-8 plain-text payload, canonical type first.
///
/// Unlike X11 ICCCM (where the selection owner converts on demand), a Wayland
/// data-control source must advertise *every* MIME a paster might negotiate
/// over — the compositor performs no conversion. Toolkits disagree on the
/// canonical text type: GTK / Firefox request `text/plain;charset=utf-8` and
/// the X11 atom name `UTF8_STRING`, Qt also accepts `STRING` / `TEXT`, and
/// older code uses a bare `text/plain`. Advertising only one of these makes the
/// selection invisible to apps that ask for the others — Firefox's Wayland
/// address bar pastes nothing from a bare `text/plain` offer. Mirror
/// `wl-clipboard` (and the X11 writer's alias set) by offering the whole family,
/// all backed by the same bytes.
pub(super) const TEXT_PLAIN_MIME_ALIASES: &[&str] = &[
    "text/plain;charset=utf-8",
    "text/plain;charset=UTF-8",
    "text/plain",
    "UTF8_STRING",
    "STRING",
    "TEXT",
];

/// Returns true when `mime` denotes a UTF-8 plain-text payload that should be
/// advertised under the full [`TEXT_PLAIN_MIME_ALIASES`] set.
///
/// Accepts the X11 atom-name aliases (`UTF8_STRING` / `STRING` / `TEXT`) and
/// `text/plain` whose charset is either absent or `utf-8`. Clipboard text
/// synced by the app is always UTF-8, so a bare `text/plain` (no charset) is
/// treated as UTF-8. A `text/plain` carrying any other charset (e.g. GBK,
/// ISO-8859-1) is rejected — expanding it to the UTF-8 alias family would
/// re-advertise non-UTF-8 bytes as `UTF8_STRING` / `text/plain;charset=utf-8`
/// and corrupt the paste.
pub(super) fn is_text_plain_mime(mime: &str) -> bool {
    let normalized = mime.trim().to_ascii_lowercase();

    // X11 atom-name aliases always denote UTF-8 plain text.
    if matches!(normalized.as_str(), "utf8_string" | "string" | "text") {
        return true;
    }

    // Split the media type from its parameters; require exactly `text/plain`.
    let mut parts = normalized.split(';').map(str::trim);
    if parts.next() != Some("text/plain") {
        return false;
    }

    // A charset parameter, if present, must be UTF-8. Absence => UTF-8.
    for param in parts {
        if let Some(charset) = param.strip_prefix("charset=") {
            if charset != "utf-8" {
                return false;
            }
        }
    }
    true
}

/// Resolve the ordered list of MIME types to advertise for a single
/// representation whose primary MIME is `primary_mime`.
///
/// Plain-text payloads (see [`is_text_plain_mime`]) expand to the full
/// [`TEXT_PLAIN_MIME_ALIASES`] family; every other format yields just its own
/// type. The first entry is the canonical/preferred type. Shared between the
/// [`wlr`] and [`ext`] write paths.
pub(super) fn offer_mimes_for(primary_mime: &str) -> Vec<String> {
    if is_text_plain_mime(primary_mime) {
        TEXT_PLAIN_MIME_ALIASES
            .iter()
            .map(|m| (*m).to_string())
            .collect()
    } else {
        vec![primary_mime.to_string()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mime_covers_known_format_ids() {
        assert_eq!(
            default_mime_for_format("text"),
            Some("text/plain;charset=utf-8")
        );
        assert_eq!(default_mime_for_format("html"), Some("text/html"));
        assert_eq!(default_mime_for_format("rtf"), Some("text/rtf"));
        assert_eq!(default_mime_for_format("image"), Some("image/png"));
        assert_eq!(default_mime_for_format("files"), Some("text/uri-list"));
        assert_eq!(default_mime_for_format("unknown"), None);
    }

    #[test]
    fn bare_text_plain_expands_to_full_utf8_alias_set() {
        // The bug: a remote text push normalizes to a bare `text/plain` rep, and
        // the Wayland writer used to advertise only that. Firefox's address bar
        // requests `text/plain;charset=utf-8` / `UTF8_STRING` and pasted nothing.
        let offered = offer_mimes_for("text/plain");
        assert!(offered.contains(&"text/plain;charset=utf-8".to_string()));
        assert!(offered.contains(&"UTF8_STRING".to_string()));
        assert!(offered.contains(&"text/plain".to_string()));
        // Canonical type must come first so paste-priority pickers prefer it.
        assert_eq!(
            offered.first().map(String::as_str),
            Some("text/plain;charset=utf-8")
        );
    }

    #[test]
    fn charset_qualified_text_also_expands() {
        let offered = offer_mimes_for("text/plain;charset=utf-8");
        assert_eq!(offered.len(), TEXT_PLAIN_MIME_ALIASES.len());
        assert!(offered.contains(&"UTF8_STRING".to_string()));
    }

    #[test]
    fn non_text_mimes_are_left_untouched() {
        assert_eq!(offer_mimes_for("text/html"), vec!["text/html".to_string()]);
        assert_eq!(offer_mimes_for("image/png"), vec!["image/png".to_string()]);
        assert_eq!(
            offer_mimes_for("text/uri-list"),
            vec!["text/uri-list".to_string()]
        );
    }

    #[test]
    fn x11_atom_names_count_as_text() {
        assert!(is_text_plain_mime("UTF8_STRING"));
        assert!(is_text_plain_mime("STRING"));
        assert!(is_text_plain_mime("TEXT"));
        assert!(!is_text_plain_mime("text/html"));
        assert!(!is_text_plain_mime("image/png"));
    }

    #[test]
    fn non_utf8_text_plain_is_rejected() {
        // A non-UTF-8 charset must NOT expand to the UTF-8 alias family —
        // otherwise its bytes would be advertised as UTF8_STRING /
        // text/plain;charset=utf-8 and corrupt paste.
        assert!(!is_text_plain_mime("text/plain;charset=gbk"));
        assert!(!is_text_plain_mime("text/plain;charset=iso-8859-1"));
        assert!(!is_text_plain_mime("text/plain; charset=GBK"));
        // The old prefix check false-matched this; the parser rejects it.
        assert!(!is_text_plain_mime("text/plaintext"));

        // Bare and explicit utf-8 still count; a non-charset param is ignored.
        assert!(is_text_plain_mime("text/plain"));
        assert!(is_text_plain_mime("text/plain;charset=utf-8"));
        assert!(is_text_plain_mime("text/plain;charset=UTF-8"));
        assert!(is_text_plain_mime("text/plain; charset=utf-8"));
        assert!(is_text_plain_mime("text/plain;format=flowed"));

        // The expansion path stays intact for accepted variants.
        assert_eq!(
            offer_mimes_for("text/plain;charset=gbk"),
            vec!["text/plain;charset=gbk".to_string()]
        );
        assert_eq!(
            offer_mimes_for("text/plain").len(),
            TEXT_PLAIN_MIME_ALIASES.len()
        );
    }
}
