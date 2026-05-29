//! Pairing invitation port.
//!
//! Sponsor-side capability for issuing and consuming a short-lived
//! invitation credential that a joiner can redeem to find and dial the
//! sponsor. Adapters own the TTL policy, the publishing transport(s), and
//! the on-wire code format.
//!
//! An adapter may publish a single invitation through more than one
//! discovery channel concurrently. `issue_invitation` returns success as
//! long as the invitation is locally minted and at least one channel is
//! initiated; per-channel publish outcomes are surfaced through a separate
//! observability surface, not through this port's return value.
//!
//! `issue_invitation` is the sponsor's display-time call; `consume_invitation`
//! is the post-handshake bookkeeping call that marks the code consumed on
//! every discovery channel that holds a record, so other joiners can't race
//! on it. Joiner-side dial lives on
//! [`PairingSessionPort`](crate::ports::pairing::PairingSessionPort).

use std::net::IpAddr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::Serialize;
use thiserror::Error;

pub use crate::pairing::invitation::InvitationCode;

/// Provenance of an issued invitation code, and — when minted locally —
/// the reason the directory was not used.
///
/// The distinction is observable to the joiner: a locally-minted code is
/// only resolvable by joiners on the same local network (the directory
/// holds no record of it), whereas a directory-issued code is resolvable
/// across networks. The two locally-minted reasons differ in intent: a
/// LAN-only sponsor deliberately never contacts the directory, whereas a
/// fallback mint reflects a transient directory outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CodeOrigin {
    /// The directory service assigned the code — the sponsor had directory
    /// reachability at issue time.
    DirectoryIssued,
    /// Minted locally because the sponsor is configured for LAN-only
    /// pairing and skips the directory entirely.
    LocallyMintedLanOnly,
    /// Minted locally because the directory was unreachable at issue time
    /// (transient outage); the sponsor would otherwise have used it.
    LocallyMintedDirectoryUnreachable,
}

/// Successfully issued invitation.
///
/// "Issued" means the invitation has been locally minted and parked for the
/// sponsor's own lifecycle bookkeeping. Per-channel publish outcomes may
/// still be in flight when this struct is returned — observers consult the
/// adapter's status surface for that, not this struct.
#[derive(Debug, Clone)]
pub struct IssuedInvitation {
    /// Code the joiner enters.
    pub code: InvitationCode,
    /// Adapter-decided expiry. Treated as ground truth by callers; the
    /// adapter is responsible for keeping all publish channels and the
    /// local aggregate aligned on the same instant.
    pub expires_at: DateTime<Utc>,
    /// Provenance of the code — see [`CodeOrigin`]. A locally-minted code is
    /// only resolvable on the local network; callers may surface that
    /// distinction (e.g. "this code only works on your LAN").
    pub code_origin: CodeOrigin,
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
    /// Adapter couldn't reach its transport (e.g. local network endpoint
    /// not started). Surfaced to UI as "start network first".
    #[error("network is not started")]
    NetworkNotStarted,

    /// Every discovery channel the adapter could publish through is
    /// currently unable to accept an announcement. For adapters that
    /// publish through more than one channel, this is returned **only**
    /// when every channel has failed; if at least one channel can be
    /// initiated, `issue_invitation` returns `Ok` and per-channel
    /// degradation is reported on the observability surface instead.
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

/// Errors produced while marking an invitation consumed on its discovery
/// channels.
///
/// Semantically "best-effort": the sponsor has already validated the code
/// against its local holder before calling `consume_invitation`, so these
/// errors are informational — the local handshake continues regardless.
/// Callers log and move on.
///
/// For adapters that publish through multiple channels, the variants describe
/// the **aggregate** outcome (e.g. `NotFound` means no channel still held a
/// queryable record — including the case where some channels never published
/// because they were unreachable at issue time).
#[derive(Debug, Error)]
pub enum ConsumeInvitationError {
    /// No discovery channel held a queryable record for this code (already
    /// expired, already consumed, or never successfully published).
    /// Benign — the code's lifecycle is already terminal.
    #[error("invitation not found on any discovery channel")]
    NotFound,

    /// A discovery channel held a record but it is past its TTL. Benign for
    /// the same reason as `NotFound` — kept distinct so logs can distinguish
    /// "never existed" from "raced against TTL".
    #[error("invitation already expired on discovery channel")]
    Expired,

    /// Every discovery channel the adapter would mark consumed is
    /// unreachable. Sponsor orchestrator logs and continues — the code TTL
    /// will reap stale entries anyway.
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
    ///
    /// Returning `Ok` means the invitation is locally minted and at least
    /// one publish channel has been initiated. Per-channel publish outcomes
    /// may still be in flight or partially failed; observers consult the
    /// adapter's status surface for that signal.
    async fn issue_invitation(&self) -> Result<IssuedInvitation, InvitationError>;

    /// Mark the invitation consumed on every discovery channel that holds a
    /// record. The call is best-effort — failures do not invalidate the
    /// local handshake (the sponsor has already moved the local aggregate
    /// to `Consumed`). Idempotent: repeated calls for the same code after
    /// the entries are reaped return `NotFound`, not an error.
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
