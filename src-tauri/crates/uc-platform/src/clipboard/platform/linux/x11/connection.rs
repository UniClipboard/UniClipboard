//! Shared X11 connection wrapper.
//!
//! Each consumer (`X11EventLoop` watcher, `X11Clipboard` worker) holds its
//! own `X11Server`. ICCCM doesn't preclude multiplexing reads + ownership +
//! XFIXES notifications on a single connection, but separating them lets
//! each subsystem block on its own fd without juggling foreign event types.
//! Mirrors the two-connection split `clipboard_rs` used internally.

use anyhow::{Context, Result};
use x11rb::connection::Connection;
use x11rb::protocol::xfixes::ConnectionExt as _;
use x11rb::protocol::xproto::{
    ConnectionExt as _, CreateWindowAux, EventMask, Window, WindowClass,
};
use x11rb::rust_connection::RustConnection;
use x11rb::COPY_DEPTH_FROM_PARENT;

use super::atoms::Atoms;

/// XFIXES major version we ask for. 5.0 is what `clipboard_rs` and most
/// reference clients pick; nothing past 5.0 is needed for selection-input
/// notification.
const XFIXES_MAJOR: u32 = 5;
const XFIXES_MINOR: u32 = 0;

pub(super) struct X11Server {
    pub(super) conn: RustConnection,
    /// Hidden 1×1 window used both as the selection-request requestor and
    /// (when the owner side uses it) the selection owner. `PROPERTY_CHANGE`
    /// is enabled so INCR transfers can deliver `PropertyNotify` events.
    pub(super) window: Window,
    pub(super) screen_root: Window,
    pub(super) atoms: Atoms,
}

impl X11Server {
    /// Open a connection to `$DISPLAY`, intern atoms, create a hidden window,
    /// and verify XFIXES is available. Anything that fails surfaces as
    /// `Err` so the caller can fall back to the legacy adapter.
    pub(super) fn connect() -> Result<Self> {
        let (conn, screen_num) =
            x11rb::connect(None).context("x11rb::connect failed (no X display reachable?)")?;

        // Pull screen / root before we move conn into Self.
        let screen_root = conn
            .setup()
            .roots
            .get(screen_num)
            .context("x11rb::connect returned an invalid screen index")?
            .root;

        // XFIXES is non-core; query_version doubles as the availability
        // probe. Most modern X servers (and XWayland) ship with it.
        conn.xfixes_query_version(XFIXES_MAJOR, XFIXES_MINOR)
            .context("xfixes query_version request failed")?
            .reply()
            .context("xfixes query_version reply failed — extension unavailable?")?;

        let window = conn
            .generate_id()
            .context("x11: failed to allocate window id")?;

        // Hidden 1×1 InputOutput window. PROPERTY_CHANGE is required so
        // INCR transfers deliver `PropertyNotify` events to us;
        // STRUCTURE_NOTIFY lets us catch UnmapNotify if the server tears
        // down the window unexpectedly.
        let screen = conn
            .setup()
            .roots
            .get(screen_num)
            .context("x11: screen disappeared between probe and create_window")?;
        conn.create_window(
            COPY_DEPTH_FROM_PARENT,
            window,
            screen.root,
            0,
            0,
            1,
            1,
            0,
            WindowClass::INPUT_OUTPUT,
            screen.root_visual,
            &CreateWindowAux::new()
                .event_mask(EventMask::STRUCTURE_NOTIFY | EventMask::PROPERTY_CHANGE),
        )
        .context("x11: create_window request failed")?
        .check()
        .context("x11: create_window check failed")?;

        let atoms = Atoms::new(&conn)
            .context("x11: failed to issue atom intern requests")?
            .reply()
            .context("x11: failed to intern atoms (server closed connection?)")?;

        Ok(Self {
            conn,
            window,
            screen_root,
            atoms,
        })
    }

    /// Intern a single atom by name (used when we encounter a mime atom
    /// returned from `TARGETS` that isn't in our static `Atoms` set — e.g.
    /// `image/jpeg`, `image/webp`).
    pub(super) fn intern_atom(&self, name: &str) -> Result<x11rb::protocol::xproto::Atom> {
        let reply = self
            .conn
            .intern_atom(false, name.as_bytes())
            .with_context(|| format!("x11: intern_atom request failed for {name:?}"))?
            .reply()
            .with_context(|| format!("x11: intern_atom reply failed for {name:?}"))?;
        Ok(reply.atom)
    }

    /// Reverse lookup an atom's printable name. Returns `"<unknown>"` if the
    /// reply fails — caller already knows it's a transient identifier so
    /// surfacing the underlying error here would just add noise.
    pub(super) fn atom_name(&self, atom: x11rb::protocol::xproto::Atom) -> String {
        let cookie = match self.conn.get_atom_name(atom) {
            Ok(c) => c,
            Err(_) => return "<unknown>".into(),
        };
        match cookie.reply() {
            Ok(reply) => String::from_utf8_lossy(&reply.name).into_owned(),
            Err(_) => "<unknown>".into(),
        }
    }
}

impl Drop for X11Server {
    fn drop(&mut self) {
        // Best-effort. If the server already tore us down (display
        // disconnect), these will error and we just swallow it.
        let _ = self.conn.destroy_window(self.window);
        let _ = self.conn.flush();
    }
}
