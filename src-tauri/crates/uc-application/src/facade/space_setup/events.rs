//! Sponsor-side pairing lifecycle events broadcast by
//! [`SpaceSetupFacade::subscribe_pairing_completion`].
//!
//! A single `PairingOutcome` fires per matched invitation: either `Success`
//! after admit + trust + Confirm landed, or `Failure` if any post-match step
//! fails (proof mismatch, persistence failure, Confirm send failure, clock
//! out of range, holder invariant break). Stray connections carrying an
//! unknown or expired code do **not** produce an outcome — the listener
//! should remain valid until their own invitation resolves.

use uc_core::ids::DeviceId;
use uc_core::security::IdentityFingerprint;

pub use uc_observability::analytics::PairingFailureReason;

/// Terminal result of a sponsor-side inbound pairing handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PairingOutcome {
    /// Handshake completed and both `SpaceMember` + `TrustedPeer` rows
    /// landed locally; the joiner received the `Confirm` frame.
    Success {
        peer_device_id: DeviceId,
        peer_device_name: String,
        peer_fingerprint: IdentityFingerprint,
    },
    /// Handshake started (invitation matched) but failed before reaching
    /// a committed confirm. `reason` is a structured `PairingFailureReason`
    /// — `Display` produces the stable `snake_case` identifier suitable
    /// for both human-facing display and dashboard funnel matching.
    Failure { reason: PairingFailureReason },
}
