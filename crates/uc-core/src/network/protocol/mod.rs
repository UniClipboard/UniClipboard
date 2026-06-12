mod clipboard;
mod clipboard_payload_v3;

/// Standard MIME type constants used throughout the clipboard protocol.
pub const MIME_IMAGE_PREFIX: &str = "image/";
pub const MIME_TEXT_HTML: &str = "text/html";
pub const MIME_TEXT_RTF: &str = "text/rtf";
pub const MIME_TEXT_PLAIN: &str = "text/plain";

pub use clipboard::FileTransferMapping;
pub use clipboard_payload_v3::{BinaryRepresentation, ClipboardBinaryPayload};
