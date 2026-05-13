//! Pairing invitation port.
//!
//! Sponsor-side capability for issuing and consuming a short-lived
//! invitation credential that a joiner can redeem to find and dial the
//! sponsor. The concrete adapter (Slice 1: rendezvous HTTP client) owns
//! the TTL policy, the transport to the rendezvous service, and the
//! on-wire code format.
//!
//! `issue_invitation` is the sponsor's display-time call; `consume_invitation`
//! is the post-handshake bookkeeping call that tells the rendezvous service
//! the code has been redeemed so other joiners can't race on it. Joiner-side
//! dial lives on [`PairingSessionPort`](crate::ports::pairing::PairingSessionPort).

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

pub use crate::pairing::invitation::InvitationCode;

/// Successfully issued invitation.
#[derive(Debug, Clone)]
pub struct IssuedInvitation {
    /// Code the joiner enters.
    pub code: InvitationCode,
    /// Server-authoritative expiry (decision Q-B1-1).
    pub expires_at: DateTime<Utc>,
}

/// A local address the sponsor could publish in a pairing ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PairingInvitationAddressCandidate {
    /// Address the remote peer would dial.
    pub ip: IpAddr,
    /// Port associated with the address.
    pub port: u16,
}

/// Errors produced while issuing an invitation.
#[derive(Debug, Error)]
pub enum InvitationError {
    /// Adapter couldn't reach its transport (e.g. iroh endpoint not started).
    /// Surfaced to UI as "start network first".
    #[error("network is not started")]
    NetworkNotStarted,

    /// Rendezvous service unreachable / returned a transient failure.
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,

    /// The caller-selected address is not currently available for issuance —
    /// either it never appeared in the candidate set, or it was dropped by
    /// the address filter (overlay-network rules, link-local, fake-ip).
    #[error("requested address is not available: {0}")]
    AddressNotAvailable(IpAddr),

    /// Unexpected adapter-side failure; message is for logs only.
    #[error("internal invitation error: {0}")]
    Internal(String),
}

/// Errors produced while reporting a successful consume to rendezvous.
///
/// Semantically "best-effort": the sponsor has already validated the code
/// against its local holder before calling `consume_invitation`, so these
/// errors are informational — the local handshake continues regardless.
/// Callers log and move on.
#[derive(Debug, Error)]
pub enum ConsumeInvitationError {
    /// Rendezvous entry is gone (already expired or already consumed).
    /// Benign — the code's lifecycle on the server is already terminal.
    #[error("invitation not found on rendezvous")]
    NotFound,

    /// Rendezvous entry exists but is past its TTL. Benign for the same
    /// reason as `NotFound` — kept distinct so logs can distinguish
    /// "never existed" from "raced against TTL".
    #[error("invitation already expired on rendezvous")]
    Expired,

    /// Rendezvous service unreachable / transient failure. Sponsor
    /// orchestrator logs and continues — the code TTL will reap the
    /// server-side entry anyway.
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,

    /// Adapter-side failure; message is for logs only.
    #[error("internal consume error: {0}")]
    Internal(String),
}

/// Sponsor-side invitation lifecycle.
#[async_trait]
pub trait PairingInvitationPort: Send + Sync {
    /// Request a fresh invitation code. The adapter decides TTL and code
    /// format; callers treat the returned `expires_at` as ground truth.
    async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError>;

    /// Notify the rendezvous service that the sponsor has accepted an
    /// inbound joiner carrying this code. The call is best-effort — failures
    /// do not invalidate the local handshake (the sponsor has already moved
    /// the local aggregate to `Consumed`). Concrete adapter contract:
    /// idempotent on the server side (repeated calls for the same code
    /// return `NotFound` once the entry is reaped, not an error).
    async fn consume_invitation(&self, code: &InvitationCode)
        -> Result<(), ConsumeInvitationError>;
}

/// Query capability for listing the local addresses currently eligible to
/// appear in a pairing ticket.
#[async_trait]
pub trait PairingInvitationAddressQueryPort: Send + Sync {
    /// List the candidate local addresses currently eligible for inclusion
    /// in a pairing ticket. Returns an empty `Ok` vector is **not** the
    /// expected shape — adapters return `NetworkNotStarted` when no
    /// candidate is available.
    async fn list_invitation_addresses(
        &self,
    ) -> Result<Vec<PairingInvitationAddressCandidate>, InvitationError>;
}

/// Issue an invitation restricted to a single caller-selected local
/// address. Companion to [`PairingInvitationPort::issue_invitation`] —
/// kept on a separate trait because this is a diagnostic / multi-NIC
/// override capability, not part of the standard sponsor lifecycle.
///
/// Adapters apply the same address filter as `issue_invitation` before
/// honouring the selection: an IP dropped by the filter (overlay, fake-ip,
/// link-local) yields `AddressNotAvailable` rather than bypassing the
/// rule. Callers that want the unfiltered list must change the filter
/// configuration, not work around it through this port.
#[async_trait]
pub trait PairingInvitationByAddressPort: Send + Sync {
    /// Issue an invitation whose ticket only carries the address matching
    /// `selected_ip`. Returns `AddressNotAvailable` when the IP is not in
    /// the current filtered candidate set.
    async fn issue_invitation_for_address(
        &self,
        selected_ip: IpAddr,
    ) -> Result<IssuedInvitation, InvitationError>;
}
