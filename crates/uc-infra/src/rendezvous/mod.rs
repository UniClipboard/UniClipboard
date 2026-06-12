//! Rendezvous service client (Slice 1 · P7a).
//!
//! Layered as:
//!
//! * [`client`] — pure HTTP gateway ([`RendezvousClient`]). Owns the
//!   `reqwest::Client`, User-Agent, timeout, and HTTP-level error model.
//!   Shared by every rendezvous adapter; no business semantics live here.
//! * [`invitation_adapter`] — sponsor-side port adapter implementing
//!   [`uc_core::ports::PairingInvitationPort`]. Maps [`RendezvousHttpError`]
//!   onto domain errors (`InvitationError`, `ConsumeInvitationError`).
//!
//! Joiner-side dial flow lives in `crate::pairing::session`, which also
//! consumes [`RendezvousClient`] (for the `/v1/pairings/resolve` call).

pub mod client;
pub mod invitation_adapter;

pub use client::{
    CreatePairingRequest, CreatePairingResponse, RendezvousClient, RendezvousHttpError,
    ResolvePairingResponse, RENDEZVOUS_BASE_URL,
};
pub use invitation_adapter::RendezvousPairingInvitationAdapter;
