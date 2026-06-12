use uc_core::{DeviceId, TrustAbortReason, TrustedPeer};

use super::challenge::TrustVerificationChallenge;

/// Application-layer state of the trust-establishment flow (DOMAIN §5.4).
///
/// Middle states (`EstablishingSession`, `AwaitingUserVerification`) live
/// only in memory; only the terminal `Trusted` reaches the repository via
/// `TrustedPeer`. `Aborted` is not persisted — hard-delete model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustState {
    Idle,
    EstablishingSession {
        peer_device_id: DeviceId,
    },
    AwaitingUserVerification {
        peer_device_id: DeviceId,
        challenge: TrustVerificationChallenge,
    },
    Trusted {
        trusted_peer: TrustedPeer,
    },
    Aborted {
        reason: TrustAbortReason,
    },
}

/// Inputs that drive the `TrustState` transitions.
///
/// These are *internal* to the orchestrator/state-machine: external callers
/// use the UseCase surface (`ConfirmPeerVerificationUseCase`, `CancelTrustingUseCase`, …)
/// and the orchestrator's `initiate` / `record_session_opened` methods,
/// which translate into these events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrustStateEvent {
    /// Start trusting a specific peer — leave `Idle`.
    Initiate { peer_device_id: DeviceId },

    /// The pairing session has exchanged fingerprints and produced a
    /// short-code challenge ready for user verification.
    SessionOpened {
        peer_device_id: DeviceId,
        challenge: TrustVerificationChallenge,
    },

    /// User has confirmed the peer identity; `trusted_peer` carries the
    /// aggregate that the orchestrator already persisted via
    /// `TrustPeerUseCase` so this transition is pure and deterministic.
    UserConfirmed { trusted_peer: TrustedPeer },

    /// User cancelled the flow explicitly.
    UserCancelled,

    /// Transport / protocol layer reported timeout.
    TimedOut,

    /// Transport / protocol layer reported a non-recoverable error.
    ProtocolError,
}
