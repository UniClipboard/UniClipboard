//! Native Wayland clipboard backend for Linux.
//!
//! Two compositor protocols implement clipboard control: `wlr-data-control-v1`
//! (niri / sway / hyprland / KDE / wlroots) and `ext-data-control-v1` (GNOME
//! mutter ≥ 47, KDE Plasma 6, recent wlroots compositors). They are
//! bit-identical in shape, so the structural code is shared via small traits
//! ([`backend::OfferLike`]) and protocol-agnostic helpers
//! ([`transfer`], [`snapshot`], [`write_payload`]). The per-protocol pieces
//! that wayland-rs's `Dispatch` machinery requires to be concrete (event
//! pattern matching, `event_created_child!` invocations, manager bindings)
//! live in [`protocol::wlr`] and [`protocol::ext`].
//!
//! Selection happens at construction time via [`protocol::try_new_event_loop`]
//! / [`protocol::try_new_clipboard`]:
//!
//! 1. `UC_FORCE_DATA_CONTROL=ext|wlr` overrides the default — useful for local
//!    verification on compositors (like niri) that advertise both protocols.
//! 2. Otherwise prefer `ext-data-control` (the standardized future-proof
//!    choice), fall back to `wlr-data-control` (broader real-world coverage
//!    today).
//!
//! On a session whose compositor advertises neither manager,
//! [`event_loop::WaylandEventLoop::try_new`] / [`clipboard::WaylandClipboard::try_new`]
//! return `Ok(None)` and [`super::super::build_event_loop`] / `LinuxClipboard::new`
//! fall back to the legacy `clipboard_rs` adapter.

mod backend;
mod clipboard;
mod event_loop;
mod protocol;
mod snapshot;
mod transfer;
mod write_payload;

pub(super) use clipboard::WaylandClipboard;
pub(super) use event_loop::WaylandEventLoop;
