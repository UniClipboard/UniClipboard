use serde::{Deserialize, Serialize};

/// Standard MIME type constants used throughout the clipboard protocol.
pub const MIME_IMAGE_PREFIX: &str = "image/";
pub const MIME_TEXT_HTML: &str = "text/html";
pub const MIME_TEXT_RTF: &str = "text/rtf";
pub const MIME_TEXT_PLAIN: &str = "text/plain";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardTextPayloadV1 {
    pub text: String,
    pub mime: String,
    pub ts_ms: i64,
}

impl ClipboardTextPayloadV1 {
    pub const MIME_TEXT_PLAIN: &'static str = "text/plain";

    pub fn new(text: String, ts_ms: i64) -> Self {
        Self {
            text,
            mime: Self::MIME_TEXT_PLAIN.to_string(),
            ts_ms,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClipboardTextPayloadV1;

    #[test]
    fn clipboard_text_payload_v1_serializes_and_deserializes() {
        let payload = ClipboardTextPayloadV1::new("hello".to_string(), 1_713_000_000_000);

        let json = serde_json::to_string(&payload).unwrap();
        let decoded: ClipboardTextPayloadV1 = serde_json::from_str(&json).unwrap();

        assert_eq!(decoded, payload);
    }

    #[test]
    fn clipboard_text_payload_v1_new_sets_text_plain_mime() {
        let payload = ClipboardTextPayloadV1::new("hello".to_string(), 42);

        assert_eq!(payload.mime, ClipboardTextPayloadV1::MIME_TEXT_PLAIN);
    }
}
