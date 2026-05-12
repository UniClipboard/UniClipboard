//! Protocol-agnostic abstractions over wlr / ext data-control.
//!
//! The two compositor protocols `wlr-data-control-unstable-v1` and
//! `ext-data-control-v1` are bit-identical in shape (same events, same
//! request opcodes, same arg orders) but use distinct concrete types in
//! `wayland-rs`'s generated bindings. This module gives us a small surface
//! that the protocol-agnostic helpers ([`super::transfer`], [`super::snapshot`])
//! can call without knowing which protocol we're on, and that the per-protocol
//! modules (`super::protocol::wlr`, `super::protocol::ext`) implement against
//! their concrete types.
//!
//! Keep the trait surface small: only operations the helpers actually need.
//! Anything protocol-specific that *also* needs to flow through the
//! `wayland-client::Dispatch` machinery (event matching, child creation) lives
//! in the per-protocol module — wayland-rs's `Dispatch` impls have to be
//! concrete on the interface type and can't be generic.

use std::os::fd::BorrowedFd;

/// Methods we need on a `*_data_control_offer_v1` proxy when reading payloads.
pub(super) trait OfferLike {
    /// Tell the compositor to write the bytes for `mime` into `fd`.
    fn receive_to(&self, mime: &str, fd: BorrowedFd<'_>);
}
