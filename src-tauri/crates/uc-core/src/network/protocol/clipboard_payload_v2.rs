//! V2 multi-representation clipboard payload.
//!
//! This is the plaintext structure that gets packed, chunked, and encrypted
//! for network transfer. It carries all clipboard representations in a single
//! atomic bundle.
//!
//! Serialized with serde_json before chunking — the JSON overhead is negligible
//! relative to binary payloads (images, etc.).

use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

/// Pre-encryption envelope for V2 clipboard transfers.
/// Contains all representations from a SystemClipboardSnapshot.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ClipboardMultiRepPayloadV2 {
    /// Timestamp from SystemClipboardSnapshot.ts_ms
    pub ts_ms: i64,
    /// All clipboard representations bundled together.
    pub representations: Vec<WireRepresentation>,
}

/// A single clipboard representation over the wire.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WireRepresentation {
    /// MIME type string (e.g., "image/png", "text/html", "text/plain").
    /// None if MIME is unknown.
    pub mime: Option<String>,
    /// Platform format identifier (e.g., "public.png", "text/html").
    pub format_id: String,
    /// Raw bytes of this representation.
    /// Uses base64 encoding in JSON for compact representation.
    #[serde_as(as = "Base64")]
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_multi_rep_payload_v2_roundtrip() {
        let payload = ClipboardMultiRepPayloadV2 {
            ts_ms: 1_713_000_000_000,
            representations: vec![
                WireRepresentation {
                    mime: Some("text/plain".to_string()),
                    format_id: "public.utf8-plain-text".to_string(),
                    bytes: b"hello world".to_vec(),
                },
                WireRepresentation {
                    mime: Some("image/png".to_string()),
                    format_id: "public.png".to_string(),
                    bytes: vec![0x89, 0x50, 0x4E, 0x47],
                },
            ],
        };

        let json = serde_json::to_vec(&payload).expect("serialize V2 payload");
        let decoded: ClipboardMultiRepPayloadV2 =
            serde_json::from_slice(&json).expect("deserialize V2 payload");
        assert_eq!(payload, decoded);
    }

    #[test]
    fn wire_representation_with_none_mime_roundtrips() {
        let rep = WireRepresentation {
            mime: None,
            format_id: "com.example.unknown".to_string(),
            bytes: vec![1, 2, 3, 4],
        };

        let json = serde_json::to_vec(&rep).expect("serialize WireRepresentation");
        let decoded: WireRepresentation =
            serde_json::from_slice(&json).expect("deserialize WireRepresentation");
        assert_eq!(rep, decoded);
    }

    #[test]
    fn wire_representation_bytes_encode_as_base64_in_json() {
        let rep = WireRepresentation {
            mime: Some("text/plain".to_string()),
            format_id: "text".to_string(),
            bytes: vec![0x01, 0x02, 0x03],
        };

        let json_str = serde_json::to_string(&rep).expect("serialize");
        // serde_with Base64 encodes as base64 in JSON, not as integer array
        // base64 of [0x01, 0x02, 0x03] is "AQID"
        assert!(
            json_str.contains("AQID"),
            "bytes should be base64-encoded in JSON, got: {json_str}"
        );
        // Should NOT be an integer array like [1,2,3]
        assert!(
            !json_str.contains("[1,2,3]"),
            "bytes should not be integer array, got: {json_str}"
        );
    }

    #[test]
    fn empty_representations_roundtrips() {
        let payload = ClipboardMultiRepPayloadV2 {
            ts_ms: 0,
            representations: vec![],
        };

        let json = serde_json::to_vec(&payload).expect("serialize empty payload");
        let decoded: ClipboardMultiRepPayloadV2 =
            serde_json::from_slice(&json).expect("deserialize empty payload");
        assert_eq!(payload, decoded);
    }
}
