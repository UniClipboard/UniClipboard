//! Network protocol types.

pub mod address_registry;
pub mod connection_policy;
pub mod daemon_api_strings;
pub mod events;
pub mod paired_device;
pub mod pairing_state_machine;
pub mod protocol;
pub mod protocol_ids;

pub use address_registry::{AddressRecord, AddressRegistry, AddressScope, AddressSource};
pub use connection_policy::{
    AllowedProtocols, ConnectionPolicy, ProtocolKind, ResolvedConnectionPolicy,
};
pub use events::{
    ConnectedPeer, DiscoveredPeer, NetworkEvent, NetworkStatus, ProtocolDenyReason,
    ProtocolDirection,
};
pub use paired_device::{PairedDevice, PairingState};
pub use pairing_state_machine::{
    CancellationBy, FailureReason, PairingAction, PairingEvent, PairingRole, PairingStateMachine,
    SessionId, TimeoutKind,
};
pub use protocol::{BinaryRepresentation, ClipboardBinaryPayload};
pub use protocol::{
    ClipboardMessage, DeviceAnnounceMessage, HeartbeatMessage, PairingBusy, PairingCancel,
    PairingChallenge, PairingChallengeResponse, PairingConfirm, PairingKeyslotOffer,
    PairingMessage, PairingReject, PairingRequest, PairingResponse, ProtocolMessage,
    MIME_IMAGE_PREFIX, MIME_TEXT_HTML, MIME_TEXT_PLAIN, MIME_TEXT_RTF,
};
pub use protocol_ids::ProtocolId;
