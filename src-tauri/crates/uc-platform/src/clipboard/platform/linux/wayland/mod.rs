//! Native Wayland clipboard backend for Linux.
//!
//! ## Scope
//!
//! Phase 2a (this module's initial cut): event-driven clipboard *watcher* on
//! the `wlr-data-control-unstable-v1` protocol, exposed via
//! [`event_loop::WaylandEventLoop`]. This is the part that lights up the
//! "compositor told me the clipboard changed" path and was the original
//! motivation for the rewrite — Linux X11 clients (the previous default) are
//! invisible to native Wayland app copies.
//!
//! Phase 2b: read/write `SystemClipboardPort` impl (`WaylandClipboard`).
//! Phase 5 (follow-up): `ext-data-control-v1` (GNOME mutter 47+) on top of
//! the same offer/source state machines.
//!
//! ## Compositor support matrix (wlr-data-control v1)
//!
//! - niri ≥ 0.1, sway ≥ 1.6, Hyprland, KDE Plasma 5/6 — full support
//! - wlroots-based compositors generally — full support
//! - GNOME mutter — **not supported** (only ext-data-control v1, see Phase 5)
//!
//! On a session whose compositor doesn't advertise the manager global,
//! [`event_loop::WaylandEventLoop::try_new`] returns `Ok(None)` and
//! [`super::super::build_event_loop`] falls back to the legacy `clipboard_rs`
//! adapter.

mod event_loop;
mod snapshot;
mod state;
mod transfer;

pub(super) use event_loop::WaylandEventLoop;
