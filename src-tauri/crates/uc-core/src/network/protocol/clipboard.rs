use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

/// Payload version for ClipboardMessage.encrypted_content.
/// Default is V1 for backward compatibility with old senders that do not
/// include this field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum ClipboardPayloadVersion {
    /// V1: JSON-serialized EncryptedBlob wrapping ClipboardTextPayloadV1
    V1 = 1,
    /// V2: Binary chunked multi-representation payload (XChaCha20-Poly1305 per chunk)
    V2 = 2,
}

impl Default for ClipboardPayloadVersion {
    fn default() -> Self {
        Self::V1
    }
}

impl From<ClipboardPayloadVersion> for u8 {
    fn from(v: ClipboardPayloadVersion) -> u8 {
        v as u8
    }
}

impl TryFrom<u8> for ClipboardPayloadVersion {
    type Error = String;
    fn try_from(v: u8) -> Result<Self, Self::Error> {
        match v {
            1 => Ok(Self::V1),
            2 => Ok(Self::V2),
            other => Err(format!("unknown ClipboardPayloadVersion: {other}")),
        }
    }
}

/// Clipboard content broadcast via network.
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardMessage {
    pub id: String,
    pub content_hash: String,
    /// Binary payload. For V1: JSON-serialized EncryptedBlob.
    /// For V2: binary chunked format (magic + header + chunks).
    /// Uses base64 encoding in JSON for compact representation.
    #[serde_as(as = "Base64")]
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
    /// Payload format version. Defaults to V1 when absent (old senders).
    #[serde(default)]
    pub payload_version: ClipboardPayloadVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn payload_version_defaults_to_v1_when_absent_in_json() {
        // Simulate a V1 message (old sender — no payload_version field)
        // base64 of b"hello" is "aGVsbG8="
        let json = r#"{
            "id": "msg-1",
            "content_hash": "abc",
            "encrypted_content": "aGVsbG8=",
            "timestamp": "2024-01-01T00:00:00Z",
            "origin_device_id": "dev-1",
            "origin_device_name": "Device"
        }"#;

        let message: ClipboardMessage = serde_json::from_str(json).expect("deserialize V1 message");
        assert_eq!(message.payload_version, ClipboardPayloadVersion::V1);
    }

    #[test]
    fn payload_version_v2_deserializes_correctly() {
        let json = r#"{
            "id": "msg-2",
            "content_hash": "def",
            "encrypted_content": "aGVsbG8=",
            "timestamp": "2024-01-01T00:00:00Z",
            "origin_device_id": "dev-2",
            "origin_device_name": "Device",
            "payload_version": 2
        }"#;

        let message: ClipboardMessage = serde_json::from_str(json).expect("deserialize V2 message");
        assert_eq!(message.payload_version, ClipboardPayloadVersion::V2);
    }

    #[test]
    fn encrypted_content_roundtrips_binary_bytes_through_json() {
        let original_bytes: Vec<u8> = vec![0x01, 0x02, 0xFF, 0xFE, 0x00, 0xAB];
        let message = ClipboardMessage {
            id: "test-id".to_string(),
            content_hash: "hash".to_string(),
            encrypted_content: original_bytes.clone(),
            timestamp: Utc::now(),
            origin_device_id: "dev-1".to_string(),
            origin_device_name: "Test Device".to_string(),
            payload_version: ClipboardPayloadVersion::V1,
        };

        let json = serde_json::to_string(&message).expect("serialize message");
        let decoded: ClipboardMessage = serde_json::from_str(&json).expect("deserialize message");
        assert_eq!(decoded.encrypted_content, original_bytes);
    }

    #[test]
    fn encrypted_content_serializes_as_base64_not_integer_array() {
        let message = ClipboardMessage {
            id: "test-id".to_string(),
            content_hash: "hash".to_string(),
            encrypted_content: vec![0x01, 0x02, 0x03],
            timestamp: Utc::now(),
            origin_device_id: "dev-1".to_string(),
            origin_device_name: "Test Device".to_string(),
            payload_version: ClipboardPayloadVersion::V1,
        };

        let json_str = serde_json::to_string(&message).expect("serialize message");
        // base64 of [0x01, 0x02, 0x03] is "AQID"
        assert!(
            json_str.contains("AQID"),
            "encrypted_content should be base64-encoded: {json_str}"
        );
        // Should NOT be [1,2,3] as integer array
        assert!(
            !json_str.contains("[1,2,3]"),
            "encrypted_content should not be integer array: {json_str}"
        );
    }

    #[test]
    fn v2_message_deserializes_without_panic_on_v1_receiver() {
        // Simulate a V2 message received by a V1 receiver:
        // ClipboardMessage deserialization should succeed even if payload_version is unknown
        // The encrypted_content will differ but the struct itself parses fine.
        let json = r#"{
            "id": "msg-v2",
            "content_hash": "xyz",
            "encrypted_content": "dGVzdA==",
            "timestamp": "2024-06-01T12:00:00Z",
            "origin_device_id": "dev-v2",
            "origin_device_name": "New Device",
            "payload_version": 2
        }"#;

        // V1 receiver still deserializes the message (no panic)
        let result: Result<ClipboardMessage, _> = serde_json::from_str(json);
        assert!(
            result.is_ok(),
            "V2 message should deserialize without panic on V1 receiver"
        );
        let message = result.unwrap();
        assert_eq!(message.payload_version, ClipboardPayloadVersion::V2);
    }

    #[test]
    fn clipboard_payload_version_try_from_u8() {
        assert_eq!(
            ClipboardPayloadVersion::try_from(1u8),
            Ok(ClipboardPayloadVersion::V1)
        );
        assert_eq!(
            ClipboardPayloadVersion::try_from(2u8),
            Ok(ClipboardPayloadVersion::V2)
        );
        assert!(ClipboardPayloadVersion::try_from(0u8).is_err());
        assert!(ClipboardPayloadVersion::try_from(3u8).is_err());
        assert!(ClipboardPayloadVersion::try_from(255u8).is_err());
    }

    #[test]
    fn clipboard_payload_version_into_u8() {
        let v1: u8 = ClipboardPayloadVersion::V1.into();
        let v2: u8 = ClipboardPayloadVersion::V2.into();
        assert_eq!(v1, 1u8);
        assert_eq!(v2, 2u8);
    }
}
