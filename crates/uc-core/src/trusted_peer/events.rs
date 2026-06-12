use serde::{Deserialize, Serialize};

use crate::ids::DeviceId;
use crate::security::IdentityFingerprint;

use super::peer::TrustedPeer;

/// Reason a trust-establishment flow was abandoned before reaching `Trusted`.
///
/// Categories only: specific protocol error details stay in the application
/// or network layer so the core domain is not polluted with transport-level
/// concerns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustAbortReason {
    UserCancelled,
    Timeout,
    ProtocolError,
}

/// Past-tense domain events emitted by the trusted-peer lifecycle.
///
/// `PeerVerificationRequired` carries only the fingerprint; presentation
/// artefacts such as short codes or QR payloads are derived by the
/// application layer from the fingerprint (see DOMAIN §5.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustedPeerEvent {
    PeerVerificationRequired {
        peer_fingerprint: IdentityFingerprint,
    },
    PeerTrusted {
        trusted_peer: TrustedPeer,
    },
    PeerDistrusted {
        peer_device_id: DeviceId,
    },
    PeerTrustAborted {
        reason: TrustAbortReason,
    },
}
