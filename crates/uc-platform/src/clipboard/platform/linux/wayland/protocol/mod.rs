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
}
