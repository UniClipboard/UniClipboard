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

/// Fallback priority for clipboard formats when MIME is missing or unknown.
/// Higher number means higher priority.
pub fn fallback_priority_from_format_id(format_id: &str) -> u8 {
    if format_id.eq_ignore_ascii_case("public.png")
        || format_id.eq_ignore_ascii_case("public.jpeg")
        || format_id.eq_ignore_ascii_case("public.jpg")
        || format_id.eq_ignore_ascii_case("public.tiff")
        || format_id.eq_ignore_ascii_case("public.gif")
        || format_id.eq_ignore_ascii_case("public.webp")
        || format_id.eq_ignore_ascii_case("image/png")
        || format_id.eq_ignore_ascii_case("image/jpeg")
        || format_id.eq_ignore_ascii_case("image/jpg")
        || format_id.eq_ignore_ascii_case("image/gif")
        || format_id.eq_ignore_ascii_case("image/webp")
    {
        4
    } else if format_id.eq_ignore_ascii_case("public.html")
        || format_id.eq_ignore_ascii_case("html")
        || format_id.eq_ignore_ascii_case(MIME_TEXT_HTML)
    {
        3
    } else if format_id.eq_ignore_ascii_case("public.rtf")
        || format_id.eq_ignore_ascii_case("rtf")
        || format_id.eq_ignore_ascii_case(MIME_TEXT_RTF)
    {
        2
    } else if format_id.eq_ignore_ascii_case("text")
        || format_id.eq_ignore_ascii_case("public.utf8-plain-text")
        || format_id.eq_ignore_ascii_case("public.text")
        || format_id.eq_ignore_ascii_case("NSStringPboardType")
        || format_id.eq_ignore_ascii_case(MIME_TEXT_PLAIN)
    {
        1
    } else {
        0
    }
}

pub use clipboard::{ClipboardMessage, ClipboardPayloadVersion};
pub use clipboard_payload_v3::{BinaryRepresentation, ClipboardBinaryPayload};
pub use device_announce::DeviceAnnounceMessage;
pub use heartbeat::HeartbeatMessage;
pub use pairing::{
    PairingBusy, PairingCancel, PairingChallenge, PairingChallengeResponse, PairingConfirm,
    PairingKeyslotOffer, PairingMessage, PairingReject, PairingRequest, PairingResponse,
};
pub use protocol_message::ProtocolMessage;
