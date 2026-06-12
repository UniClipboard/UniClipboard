//! Domain errors for [`PairingInvitation`](super::PairingInvitation) state
//! transitions.

use thiserror::Error;

/// Reasons a consume attempt (joiner-redeemed code) can fail.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConsumeError {
    /// Incoming code does not match the invitation's code.
    #[error("invitation code does not match the pending invitation")]
    CodeMismatch,

    /// Invitation has already expired relative to the consume instant.
    #[error("invitation expired")]
    Expired,

    /// Invitation is not in `Pending` state (already consumed / revoked /
    /// expired).
    #[error("invitation is not pending")]
    NotPending,
}

/// Reasons a revoke attempt can fail.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RevokeError {
    /// Invitation is not in `Pending` state — revoking already-settled
    /// invitations is a no-op that callers must distinguish (so it is
    /// surfaced as an error rather than silently accepted).
    #[error("invitation is not pending")]
    NotPending,
}
