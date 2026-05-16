//! X11 selection-owner state + `SelectionRequest` service routines.
//!
//! Becoming the `CLIPBOARD` owner is a two-step dance:
//!
//! 1. `set_selection_owner(window, CLIPBOARD, CURRENT_TIME)` — tell the X
//!    server we're the new owner. Triggers a `SelectionClear` to the
//!    previous owner.
//! 2. As long as we hold ownership, the server forwards `SelectionRequest`
//!    events to us each time another client `convert_selection`s on
//!    `CLIPBOARD`. We respond by writing the requested data into the
//!    requestor-supplied property and sending back a `SelectionNotify`.
//!
//! ICCCM §2.2 (TARGETS): the requestor will probe us with `target=TARGETS`
//! first to discover the mime list we support. We answer that with the
//! atoms we've registered for the current snapshot, plus `TARGETS` itself
//! and `TIMESTAMP`. Anything not in the registered set is refused with
//! `property=None`.
//!
//! Phase 3 caveat: this writer always writes the whole payload in a single
//! `change_property8` call. x11rb transparently uses BIG-REQUESTS so this
//! supports several-MiB payloads, but extremely large transfers (>16 MiB
//! depending on the server's max request length) should switch to INCR
//! on the write side. The reader already handles incoming INCR; outgoing
//! INCR is a follow-up.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing::{debug, warn};
use uc_core::clipboard::SystemClipboardSnapshot;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{
    Atom, AtomEnum, ConnectionExt as _, EventMask, PropMode, SelectionNotifyEvent,
    SelectionRequestEvent, SELECTION_NOTIFY_EVENT,
};
use x11rb::wrapper::ConnectionExt as _;
use x11rb::CURRENT_TIME;

use super::atoms::default_mime_for_format;
use super::connection::X11Server;

/// State the X11 worker thread carries while it owns `CLIPBOARD`. Cleared
/// on `SelectionClear` (someone else took ownership) or on an empty-snapshot
/// write request.
pub(super) struct WriterState {
    /// MIME atom → payload bytes. `Arc` so we can clone cheaply when
    /// serving a SelectionRequest without holding the state lock through
    /// the write. (Single-threaded today; keeps the door open if we ever
    /// move serving off the worker thread.)
    pub(super) payloads: HashMap<Atom, Arc<Vec<u8>>>,
    /// Snapshot the writer wrote — handed back by `read_snapshot` when the
    /// reader is asked while we're still the owner (mirrors wayland's
    /// `cached_snapshot`).
    pub(super) cached_snapshot: Option<SystemClipboardSnapshot>,
}

impl WriterState {
    pub(super) fn new() -> Self {
        Self {
            payloads: HashMap::new(),
            cached_snapshot: None,
        }
    }

    pub(super) fn clear(&mut self) {
        self.payloads.clear();
        self.cached_snapshot = None;
    }
}

/// Take ownership of `CLIPBOARD` and stash payloads for the upcoming
/// `SelectionRequest` round-trips.
///
/// An empty snapshot clears our content and releases ownership (set owner
/// to `XCB_NONE`) — symmetric with wayland's `set_selection(None)`.
pub(super) fn install_snapshot(
    server: &X11Server,
    state: &mut WriterState,
    snapshot: SystemClipboardSnapshot,
) -> Result<()> {
    let conn = &server.conn;
    let atoms = server.atoms;

    if snapshot.representations.is_empty() {
        state.clear();
        conn.set_selection_owner(x11rb::NONE, atoms.CLIPBOARD, CURRENT_TIME)
            .context("x11: set_selection_owner(NONE) failed")?
            .check()
            .context("x11: set_selection_owner(NONE) check failed")?;
        conn.flush().context("x11: flush after clear-owner")?;
        return Ok(());
    }

    let mut payloads: HashMap<Atom, Arc<Vec<u8>>> = HashMap::new();
    for rep in &snapshot.representations {
        let mime = rep
            .mime
            .as_ref()
            .map(|m| m.0.as_str().to_string())
            .or_else(|| default_mime_for_format(&rep.format_id).map(String::from));
        let Some(mime) = mime else {
            warn!(
                format_id = %rep.format_id,
                "x11 write: rep has no mime + no default mapping; skipping"
            );
            continue;
        };
        let atom = server
            .intern_atom(&mime)
            .with_context(|| format!("x11 write: failed to intern mime atom {mime:?}"))?;

        // For text mimes we also advertise the legacy ICCCM aliases so
        // applications that ask for STRING / UTF8_STRING / text/plain
        // without the charset suffix still get bytes (mirrors what wlr /
        // ext data-control sources do via their multi-offer dance, and
        // what clipboard_rs's `file_uri_list_to_clipboard_data` did for X11).
        let bytes = Arc::new(rep.expect_inline_bytes().to_vec());
        match mime.as_str() {
            "text/plain;charset=utf-8" | "text/plain;charset=UTF-8" => {
                payloads.entry(atoms.UTF8_STRING).or_insert(bytes.clone());
                payloads.entry(atoms.STRING).or_insert(bytes.clone());
                payloads.entry(atoms.TEXT).or_insert(bytes.clone());
                payloads.entry(atoms.TEXT_PLAIN).or_insert(bytes.clone());
                payloads
                    .entry(atoms.TEXT_PLAIN_UTF8)
                    .or_insert(bytes.clone());
                payloads
                    .entry(atoms.TEXT_PLAIN_UTF8_BIG)
                    .or_insert(bytes.clone());
            }
            "text/plain" => {
                payloads.entry(atoms.TEXT_PLAIN).or_insert(bytes.clone());
                payloads.entry(atoms.TEXT).or_insert(bytes.clone());
                payloads.entry(atoms.STRING).or_insert(bytes.clone());
            }
            _ => {}
        }
        payloads.insert(atom, bytes);
    }

    if payloads.is_empty() {
        anyhow::bail!("x11 write: no mime could be derived from snapshot");
    }

    state.payloads = payloads;
    state.cached_snapshot = Some(snapshot);

    conn.set_selection_owner(server.window, atoms.CLIPBOARD, CURRENT_TIME)
        .context("x11: set_selection_owner failed")?
        .check()
        .context("x11: set_selection_owner check failed")?;

    // Spec ambiguity workaround: SetSelectionOwner is supposed to succeed
    // even if another client races us, but the server only honors it if we
    // are actually the new owner. Verify with get_selection_owner so we
    // surface "we lost the race" as an error rather than silently writing
    // payloads no one will read.
    let owner = conn
        .get_selection_owner(atoms.CLIPBOARD)
        .context("x11: get_selection_owner request failed")?
        .reply()
        .context("x11: get_selection_owner reply failed")?
        .owner;
    if owner != server.window {
        state.clear();
        anyhow::bail!("x11 write: lost race to take CLIPBOARD ownership");
    }

    conn.flush().context("x11: flush after set-owner")?;
    debug!(
        mimes = state.payloads.len(),
        "x11 write: clipboard ownership taken"
    );
    Ok(())
}

/// Respond to a `SelectionRequest` event delivered to our window.
///
/// - `target == TARGETS` → write the list of advertised atoms + TARGETS +
///   TIMESTAMP as a `format=32` ATOM property.
/// - `target == TIMESTAMP` → respond with `CURRENT_TIME` (ICCCM compliance;
///   most clients ignore it, but Klipper / KDE inspect it during the
///   manager handshake).
/// - `target == <known mime atom>` → write the payload bytes as `format=8`.
/// - otherwise → refuse (`property=None` in the reply).
pub(super) fn service_selection_request(
    server: &X11Server,
    state: &WriterState,
    event: SelectionRequestEvent,
) -> Result<()> {
    let conn = &server.conn;
    let atoms = server.atoms;

    // Some clients use property=None to mean "use target as property" (an
    // ancient ICCCM convention for "obsolete clients"); honor that.
    let target_property = if event.property == x11rb::NONE {
        event.target
    } else {
        event.property
    };

    let success = if event.target == atoms.TARGETS {
        let mut targets: Vec<Atom> = state.payloads.keys().copied().collect();
        targets.push(atoms.TARGETS);
        targets.push(atoms.TIMESTAMP);
        // ATOM property values are written via the 32-bit form.
        conn.change_property32(
            PropMode::REPLACE,
            event.requestor,
            target_property,
            AtomEnum::ATOM,
            &targets,
        )
        .context("x11: change_property32(TARGETS) failed")?
        .check()
        .context("x11: change_property32(TARGETS) check failed")?;
        true
    } else if event.target == atoms.TIMESTAMP {
        let ts: [u32; 1] = [CURRENT_TIME];
        conn.change_property32(
            PropMode::REPLACE,
            event.requestor,
            target_property,
            AtomEnum::INTEGER,
            &ts,
        )
        .context("x11: change_property32(TIMESTAMP) failed")?
        .check()
        .context("x11: change_property32(TIMESTAMP) check failed")?;
        true
    } else if let Some(bytes) = state.payloads.get(&event.target) {
        conn.change_property8(
            PropMode::REPLACE,
            event.requestor,
            target_property,
            event.target,
            bytes,
        )
        .context("x11: change_property8(payload) failed")?
        .check()
        .context("x11: change_property8(payload) check failed")?;
        true
    } else {
        debug!(
            target = %server.atom_name(event.target),
            "x11 selection-request: unknown target — refusing"
        );
        false
    };

    let notify = SelectionNotifyEvent {
        response_type: SELECTION_NOTIFY_EVENT,
        sequence: event.sequence,
        time: event.time,
        requestor: event.requestor,
        selection: event.selection,
        target: event.target,
        property: if success {
            target_property
        } else {
            x11rb::NONE
        },
    };
    conn.send_event(false, event.requestor, EventMask::NO_EVENT, notify)
        .context("x11: send_event(SelectionNotify) failed")?
        .check()
        .context("x11: send_event(SelectionNotify) check failed")?;
    conn.flush().context("x11: flush after selection reply")?;
    Ok(())
}
