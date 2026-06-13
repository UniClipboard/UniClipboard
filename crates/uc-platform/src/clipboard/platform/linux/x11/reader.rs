//! Read the X11 `CLIPBOARD` selection into a `SystemClipboardSnapshot`.
//!
//! The protocol dance per ICCCM §2.6:
//!
//! 1. `convert_selection(requestor, CLIPBOARD, TARGETS, property)` — ask the
//!    owner what mimes it supports.
//! 2. Wait for `SelectionNotify` addressed to our window.
//! 3. `get_property(... type=ATOM)` — list of mime atoms.
//! 4. For each interesting mime, repeat 1-3 with that mime atom as the
//!    `target`. The reply property's `type` is normally the mime atom and
//!    `value` is the bytes — unless it's `INCR`, in which case we switch to
//!    `read_incr` and accumulate chunks via `PropertyNotify(state=NEW_VALUE)`.
//!
//! Blocking model: the reader runs on the X11 worker thread and uses
//! poll(2)-with-deadline so a misbehaving owner can't wedge the worker.
//! `SelectionRequest` events for our own outgoing selection (when we're
//! the owner) MAY arrive while we wait; those are delegated to
//! [`super::writer::service_selection_request`] from within the wait loop
//! so an external paster doesn't time out while we're reading.

use std::cell::Cell;
use std::os::fd::AsFd;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use rustix::event::{poll, PollFd, PollFlags};
use tracing::{debug, info, warn};
use uc_core::clipboard::{MimeType, ObservedClipboardRepresentation, SystemClipboardSnapshot};
use uc_core::ids::RepresentationId;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::{Atom, AtomEnum, ConnectionExt as _, Property, PropertyNotifyEvent};
use x11rb::protocol::Event;
use x11rb::CURRENT_TIME;

use super::super::mime::{
    format_id_for, is_interesting_mime, is_text_mime, rfc_mime_for, text_mime_priority,
};
use super::connection::X11Server;
use super::writer::WriterState;

/// Match `wayland::snapshot::READ_TIMEOUT`. Two seconds is the upper bound
/// most clipboard managers (klipper, copyq) give a misbehaving owner.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Hard upper bound per mime payload. Matches the wayland reader.
const MAX_MIME_BYTES: usize = 32 * 1024 * 1024;

/// Initial buffer hint used both for the single-shot `get_property` window
/// and for the per-iteration INCR fetch. The X11 wire format counts in
/// "longs" (4 bytes) — `u32::MAX / 4` is the max get_property can request
/// in one round-trip. We pick a sane mid-range value so a single-property
/// read of a few-MiB payload completes in one shot, while INCR fetches
/// pull at most this much per `PropertyNotify`.
const PROPERTY_CHUNK_LONGS: u32 = 16 * 1024 * 1024 / 4;

/// Shared context for one snapshot read.
///
/// Bundles the optional writer state (so inbound `SelectionRequest`s can be
/// serviced inline) with a flag recording whether an
/// `XfixesSelectionNotify` was consumed — and would otherwise be lost —
/// while a read round-trip was in flight. The watcher event loop re-reads
/// when the flag is set; without it a clipboard change landing mid-read is
/// dropped permanently (issue #1029).
pub(super) struct ReadContext<'a> {
    writer_state: Option<&'a WriterState>,
    selection_changed: Cell<bool>,
}

impl<'a> ReadContext<'a> {
    pub(super) fn new(writer_state: Option<&'a WriterState>) -> Self {
        Self {
            writer_state,
            selection_changed: Cell::new(false),
        }
    }

    /// True when a selection-change notification was swallowed during the
    /// read. Reading resets the flag.
    pub(super) fn take_selection_changed(&self) -> bool {
        self.selection_changed.replace(false)
    }
}

/// Read the current `CLIPBOARD` contents into a snapshot.
///
/// Returns an empty snapshot (no representations) if there is no current
/// owner or if `TARGETS` came back with no usable mimes. Per-mime failures
/// are logged at warn and skipped — one broken mime should not lose the
/// others.
///
/// `ctx.writer_state` (if any) is consulted only for the corner case where
/// a `SelectionRequest` from an external paster arrives while we're
/// waiting: we service it inline so the paster doesn't time out.
pub(super) fn read_snapshot(
    server: &X11Server,
    ctx: &ReadContext<'_>,
) -> Result<SystemClipboardSnapshot> {
    let targets = match convert_selection_bytes(server, server.atoms.TARGETS, ctx)? {
        Some(b) => b,
        None => {
            info!("x11 read: no TARGETS reply (no owner, owner refused, or timed out) — empty snapshot");
            return Ok(empty_snapshot());
        }
    };

    let target_atoms = parse_atom_list(&targets);
    debug!(count = target_atoms.len(), "x11 read: target atoms");

    // Resolve atom→name in one batch (one round-trip per atom is fine; the
    // count is typically <16). Build the (atom, mime_name) pairs first so we
    // can run the dedup/filter pass on string names symmetric with wayland.
    let mut all_names: Vec<String> = Vec::with_capacity(target_atoms.len());
    let mut candidates: Vec<(Atom, String)> = Vec::with_capacity(target_atoms.len());
    for a in target_atoms {
        let name = server.atom_name(a);
        if is_interesting_mime(&name) {
            candidates.push((a, name.clone()));
        }
        all_names.push(name);
    }
    if candidates.is_empty() && !all_names.is_empty() {
        // Names are mime identifiers / atom names, never payload content.
        info!(
            targets = %all_names.join(", "),
            "x11 read: owner advertised no interesting mimes — empty snapshot"
        );
    }
    // Reorder so we read UTF-8-friendly text mimes (e.g. `UTF8_STRING`,
    // `text/plain;charset=utf-8`) before the Latin-1-bounded fallbacks
    // (`STRING`, `TEXT`). Sources like Chromium often advertise `STRING`
    // first with a percent-encoded copy of a non-ASCII URL, then
    // `UTF8_STRING` with the original UTF-8 — reading in advertise order
    // captures the percent-encoded variant, which the user then sees
    // wherever the entry is pasted or synced. `sort_by_key` is stable, so
    // non-text mimes (priority `u32::MAX`) keep their advertise-order
    // relative position — only the text mimes shuffle among themselves.
    candidates.sort_by_key(|(_, mime)| text_mime_priority(mime));

    let mut reps = Vec::new();
    let mut text_captured = false;
    let mut image_captured = false;

    for (atom, mime) in candidates {
        if is_text_mime(&mime) && text_captured {
            continue;
        }
        let image_mime = mime.starts_with("image/");
        if image_mime && image_captured {
            continue;
        }

        let bytes = match convert_selection_bytes(server, atom, ctx) {
            Ok(Some(b)) => b,
            Ok(None) => {
                info!(mime = %mime, "x11 read: owner refused mime (or reply timed out)");
                continue;
            }
            Err(e) => {
                warn!(mime = %mime, error = %e, "x11 read: mime fetch failed");
                continue;
            }
        };
        if bytes.is_empty() {
            info!(mime = %mime, "x11 read: owner returned empty payload for mime");
            continue;
        }

        if is_text_mime(&mime) {
            text_captured = true;
        }
        if image_mime {
            image_captured = true;
        }
        let fid = format_id_for(&mime);
        let rfc_mime = rfc_mime_for(&mime)
            .map(|m| MimeType(m.to_string()))
            .unwrap_or(MimeType(mime));
        reps.push(ObservedClipboardRepresentation::new(
            RepresentationId::new(),
            fid.into(),
            Some(rfc_mime),
            bytes,
        ));
    }

    Ok(SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: reps,
    })
}

fn empty_snapshot() -> SystemClipboardSnapshot {
    SystemClipboardSnapshot {
        ts_ms: chrono::Utc::now().timestamp_millis(),
        representations: Vec::new(),
    }
}

/// Issue `convert_selection(CLIPBOARD, target=<atom>)`, wait for the
/// `SelectionNotify`, then `get_property` the result. Handles INCR
/// streaming. Returns `Ok(None)` when the owner refused (selection cleared
/// or no such target).
fn convert_selection_bytes(
    server: &X11Server,
    target: Atom,
    ctx: &ReadContext<'_>,
) -> Result<Option<Vec<u8>>> {
    let conn = &server.conn;
    let atoms = server.atoms;

    // Clear any leftover property before issuing the request so a stale
    // value from a previous (failed) round-trip can't confuse get_property.
    let _ = conn
        .delete_property(server.window, atoms.UC_CLIPBOARD_PROP)
        .and_then(|c| Ok(c.check()));

    conn.convert_selection(
        server.window,
        atoms.CLIPBOARD,
        target,
        atoms.UC_CLIPBOARD_PROP,
        CURRENT_TIME,
    )
    .context("x11: convert_selection request failed")?
    .check()
    .context("x11: convert_selection check failed")?;
    conn.flush().context("x11: flush after convert_selection")?;

    // Wait for SelectionNotify addressed to our requestor window for this
    // selection. Service inbound SelectionRequest events that arrive in
    // the meantime so external pasters don't stall.
    let notify = wait_for_selection_notify(server, atoms.CLIPBOARD, target, ctx)?;
    let Some(notify) = notify else {
        return Ok(None);
    };
    if notify.property == x11rb::NONE {
        // Owner refused or selection cleared between request + reply.
        return Ok(None);
    }

    read_property_value(server, target, ctx)
}

/// Read the bytes currently stored at our `UC_CLIPBOARD_PROP`. Handles the
/// INCR continuation path: when the reply's `type` == `INCR`, the value is
/// a single u32 size hint, after which we accumulate chunks via
/// `PropertyNotify(state=NEW_VALUE)` until an empty chunk signals EOF.
fn read_property_value(
    server: &X11Server,
    target: Atom,
    ctx: &ReadContext<'_>,
) -> Result<Option<Vec<u8>>> {
    let conn = &server.conn;
    let atoms = server.atoms;

    // First get_property: ask for the full payload in one shot. If the reply
    // type is INCR we'll re-enter the streaming path; otherwise the whole
    // payload arrives here.
    let reply = conn
        .get_property(
            false,
            server.window,
            atoms.UC_CLIPBOARD_PROP,
            AtomEnum::ANY,
            0,
            PROPERTY_CHUNK_LONGS,
        )
        .context("x11: get_property request failed")?
        .reply()
        .context("x11: get_property reply failed")?;

    if reply.type_ == atoms.INCR {
        // Spec: client deletes the property to "ack" the INCR header, then
        // waits for PropertyNotify(NEW_VALUE) on the same property, repeating
        // get_property(delete=true) until an empty value arrives.
        conn.delete_property(server.window, atoms.UC_CLIPBOARD_PROP)
            .context("x11: delete_property (INCR ack)")?
            .check()
            .context("x11: delete_property check (INCR ack)")?;
        conn.flush().context("x11: flush after INCR ack")?;

        // Pre-allocate using the size hint when present (first u32 of the
        // INCR property is a lower bound the owner advertises). Always cap
        // the hint to MAX_MIME_BYTES so a hostile hint can't OOM us.
        let mut buf = Vec::new();
        if let Some(mut value) = reply.value32() {
            if let Some(hint) = value.next() {
                let hint = (hint as usize).min(MAX_MIME_BYTES);
                buf.reserve(hint);
            }
        }
        read_incr_into(server, target, &mut buf, ctx)?;
        return Ok(Some(buf));
    }

    // Non-INCR fast path. After we own the bytes, clear the property so the
    // next round-trip starts fresh.
    let bytes = reply.value;
    let _ = conn
        .delete_property(server.window, atoms.UC_CLIPBOARD_PROP)
        .and_then(|c| Ok(c.check()));
    Ok(Some(bytes))
}

/// Streaming receive for INCR. The owner side will deposit chunk after chunk
/// at `UC_CLIPBOARD_PROP` and signal each one with `PropertyNotify(NEW_VALUE)`.
/// We acknowledge each chunk by `get_property(delete=true)`, which both
/// reads the bytes and lets the owner know we're ready for the next chunk.
/// An empty chunk signals EOF.
fn read_incr_into(
    server: &X11Server,
    target: Atom,
    buf: &mut Vec<u8>,
    ctx: &ReadContext<'_>,
) -> Result<()> {
    let conn = &server.conn;
    let atoms = server.atoms;
    let deadline = Instant::now() + READ_TIMEOUT;

    loop {
        if buf.len() > MAX_MIME_BYTES {
            anyhow::bail!(
                "x11 INCR: payload exceeded {} bytes for target {}",
                MAX_MIME_BYTES,
                target
            );
        }

        let event = wait_for_event_filtered(
            server,
            deadline,
            |e| matches!(e, Event::PropertyNotify(_)),
            ctx,
        )?;
        let Event::PropertyNotify(PropertyNotifyEvent {
            window,
            atom,
            state,
            ..
        }) = event
        else {
            // Should be unreachable: filter only admits PropertyNotify.
            continue;
        };
        if window != server.window
            || atom != atoms.UC_CLIPBOARD_PROP
            || state != Property::NEW_VALUE
        {
            continue;
        }

        let reply = conn
            .get_property(
                true,
                server.window,
                atoms.UC_CLIPBOARD_PROP,
                AtomEnum::ANY,
                0,
                PROPERTY_CHUNK_LONGS,
            )
            .context("x11 INCR: get_property request failed")?
            .reply()
            .context("x11 INCR: get_property reply failed")?;

        if reply.value.is_empty() {
            // Empty chunk = EOF.
            return Ok(());
        }
        buf.extend_from_slice(&reply.value);
    }
}

/// Block until a `SelectionNotify` for `(selection, target)` addressed to
/// our requestor window arrives, or the deadline expires. Returns `None`
/// when the deadline expires (caller treats as "no usable reply").
fn wait_for_selection_notify(
    server: &X11Server,
    selection: Atom,
    target: Atom,
    ctx: &ReadContext<'_>,
) -> Result<Option<x11rb::protocol::xproto::SelectionNotifyEvent>> {
    let deadline = Instant::now() + READ_TIMEOUT;
    let event = wait_for_event_filtered(
        server,
        deadline,
        |e| match e {
            Event::SelectionNotify(n) => {
                n.requestor == server.window && n.selection == selection && n.target == target
            }
            _ => false,
        },
        ctx,
    );
    match event {
        Ok(Event::SelectionNotify(n)) => Ok(Some(n)),
        Ok(_) => Ok(None),
        Err(e) if e.to_string().contains("x11 wait: deadline expired") => {
            // A misbehaving / slow owner (Chromium via the XWayland bridge
            // is the known offender) — visible at warn so field logs can
            // tell "owner never answered" apart from "no event at all".
            warn!(
                target = %server.atom_name(target),
                timeout_ms = READ_TIMEOUT.as_millis() as u64,
                error_kind = "x11_selection_reply_timeout",
                retryable = true,
                "x11 read: selection owner did not reply before deadline"
            );
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

/// Wait for the next event matching `pred`, polling on the connection fd
/// with a deadline. Events not matching the predicate are still consumed
/// (and routed: `SelectionRequest` is handed to the writer; everything else
/// is dropped).
fn wait_for_event_filtered(
    server: &X11Server,
    deadline: Instant,
    pred: impl Fn(&Event) -> bool,
    ctx: &ReadContext<'_>,
) -> Result<Event> {
    let conn = &server.conn;
    loop {
        // Drain anything that's already buffered without blocking.
        while let Some(event) = conn
            .poll_for_event()
            .context("x11: poll_for_event failed")?
        {
            if pred(&event) {
                return Ok(event);
            }
            route_unrelated_event(server, &event, ctx);
        }

        let now = Instant::now();
        if now >= deadline {
            anyhow::bail!("x11 wait: deadline expired");
        }
        let remaining_ms: i32 = (deadline - now)
            .as_millis()
            .min(i32::MAX as u128)
            .try_into()
            .unwrap_or(i32::MAX);

        let stream = conn.stream().as_fd();
        let mut pfd = [PollFd::new(&stream, PollFlags::IN)];
        match poll(&mut pfd, remaining_ms) {
            Ok(0) => anyhow::bail!("x11 wait: deadline expired"),
            Ok(_) => {}
            Err(rustix::io::Errno::INTR) => continue,
            Err(e) => return Err(e.into()),
        }
        // Loop back to drain freshly arrived events.
    }
}

/// Hand a non-target event off to the right place. SelectionRequest goes to
/// the writer (if we have one) so external pasters don't stall while we're
/// reading. `XfixesSelectionNotify` is recorded on the context — the
/// watcher's change subscription delivers on this same connection, and
/// consuming one here without flagging it would lose that clipboard change
/// for good (issue #1029). Everything else is dropped — SelectionClear and
/// the like are picked up by the next handle_write / event loop iteration.
fn route_unrelated_event(server: &X11Server, event: &Event, ctx: &ReadContext<'_>) {
    match event {
        Event::SelectionRequest(req) => {
            if let Some(ws) = ctx.writer_state {
                if let Err(e) = super::writer::service_selection_request(server, ws, req.clone()) {
                    warn!(error = %e, "x11 read: inline service_selection_request failed");
                }
            }
        }
        Event::XfixesSelectionNotify(_) => {
            ctx.selection_changed.set(true);
            debug!("x11 read: selection changed mid-read; flagged for re-read");
        }
        _ => {}
    }
}

/// Parse an X11 ATOM property value (`format = 32`) into a `Vec<Atom>`.
/// X11 wire format puts these as native-endian u32s back-to-back.
pub(super) fn parse_atom_list(data: &[u8]) -> Vec<Atom> {
    data.chunks_exact(4)
        .map(|c| u32::from_ne_bytes([c[0], c[1], c[2], c[3]]) as Atom)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_atom_list_round_trip_native_endian() {
        let atoms: Vec<Atom> = vec![1, 42, 0xDEAD_BEEF, 0x1234_5678];
        let mut bytes = Vec::with_capacity(atoms.len() * 4);
        for a in &atoms {
            bytes.extend_from_slice(&(*a as u32).to_ne_bytes());
        }
        assert_eq!(parse_atom_list(&bytes), atoms);
    }

    #[test]
    fn parse_atom_list_drops_trailing_partial() {
        // 6 bytes = 1 full atom + 2 trailing — trailing should be ignored.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&7u32.to_ne_bytes());
        bytes.extend_from_slice(&[0xAA, 0xBB]);
        assert_eq!(parse_atom_list(&bytes), vec![7 as Atom]);
    }

    #[test]
    fn parse_atom_list_empty() {
        assert_eq!(parse_atom_list(&[]), Vec::<Atom>::new());
    }

    #[test]
    fn read_context_take_selection_changed_resets() {
        let ctx = ReadContext::new(None);
        assert!(!ctx.take_selection_changed());

        ctx.selection_changed.set(true);
        assert!(ctx.take_selection_changed());
        // Taking the flag must reset it.
        assert!(!ctx.take_selection_changed());
    }
}
