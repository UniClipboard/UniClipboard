mod clipboard;
mod clipboard_payload_v3;
mod device_announce;
mod heartbeat;
mod pairing;
mod protocol_message;

/// Standard MIME type constants used throughout the clipboard protocol.
pub const MIME_IMAGE_PREFIX: &str = "image/";
pub const MIME_TEXT_HTML: &str = "text/html";
pub const MIME_TEXT_RTF: &str = "text/rtf";
pub const MIME_TEXT_PLAIN: &str = "text/plain";

pub use clipboard::{ClipboardMessage, ClipboardPayloadVersion};
pub use clipboard_payload_v3::{BinaryRepresentation, ClipboardBinaryPayload};
pub use device_announce::DeviceAnnounceMessage;
pub use heartbeat::HeartbeatMessage;
pub use pairing::{
    PairingBusy, PairingCancel, PairingChallenge, PairingChallengeResponse, PairingConfirm,
    PairingKeyslotOffer, PairingMessage, PairingReject, PairingRequest, PairingResponse,
};
pub use protocol_message::ProtocolMessage;
