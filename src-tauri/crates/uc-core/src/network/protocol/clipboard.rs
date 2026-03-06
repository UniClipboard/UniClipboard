use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_with::{base64::Base64, serde_as};

/// Payload version for ClipboardMessage.encrypted_content.
/// V3 is the only supported version. V1/V2 have been removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "u8", try_from = "u8")]
pub enum ClipboardPayloadVersion {
    /// V3: Binary multi-representation payload (V3 chunked AEAD with optional zstd compression)
    V3 = 3,
}

impl Default for ClipboardPayloadVersion {
    fn default() -> Self {
        Self::V3
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
            3 => Ok(Self::V3),
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
    /// Binary payload. For V3: binary chunked format (UC3 header + compressed chunks).
    /// Uses base64 encoding in JSON for compact representation.
    #[serde_as(as = "Base64")]
    pub encrypted_content: Vec<u8>,
    pub timestamp: DateTime<Utc>,
    pub origin_device_id: String,
    pub origin_device_name: String,
    /// Payload format version. Required in deserialization to reject messages with missing version.
    pub payload_version: ClipboardPayloadVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn payload_version_v3_deserializes_correctly() {
        let json = r#"{
            "id": "msg-3",
            "content_hash": "ghi",
            "encrypted_content": "aGVsbG8=",
            "timestamp": "2024-01-01T00:00:00Z",
            "origin_device_id": "dev-3",
            "origin_device_name": "Device",
            "payload_version": 3
        }"#;

        let message: ClipboardMessage = serde_json::from_str(json).expect("deserialize V3 message");
        assert_eq!(message.payload_version, ClipboardPayloadVersion::V3);
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
            payload_version: ClipboardPayloadVersion::V3,
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
            payload_version: ClipboardPayloadVersion::V3,
        };

        let json_str = serde_json::to_string(&message).expect("serialize message");
        assert!(
            json_str.contains("AQID"),
            "encrypted_content should be base64-encoded: {json_str}"
        );
        assert!(
            !json_str.contains("[1,2,3]"),
            "encrypted_content should not be integer array: {json_str}"
        );
    }

    #[test]
    fn clipboard_payload_version_try_from_u8() {
        assert_eq!(
            ClipboardPayloadVersion::try_from(3u8),
            Ok(ClipboardPayloadVersion::V3)
        );
        assert!(ClipboardPayloadVersion::try_from(0u8).is_err());
        assert!(ClipboardPayloadVersion::try_from(1u8).is_err());
        assert!(ClipboardPayloadVersion::try_from(2u8).is_err());
        assert!(ClipboardPayloadVersion::try_from(255u8).is_err());
    }

    #[test]
    fn clipboard_payload_version_into_u8() {
        let v3: u8 = ClipboardPayloadVersion::V3.into();
        assert_eq!(v3, 3u8);
    }

    #[test]
    fn default_version_is_v3() {
        assert_eq!(
            ClipboardPayloadVersion::default(),
            ClipboardPayloadVersion::V3
        );
    }

    #[test]
    fn missing_payload_version_returns_error() {
        let json = r#"{
            "id": "msg-no-ver",
            "content_hash": "abc",
            "encrypted_content": "aGVsbG8=",
            "timestamp": "2024-01-01T00:00:00Z",
            "origin_device_id": "dev-1",
            "origin_device_name": "Device"
        }"#;

        let result: Result<ClipboardMessage, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "missing payload_version should fail deserialization"
        );
    }

    #[test]
    fn unknown_version_returns_error() {
        let json = r#"{
            "id": "msg-1",
            "content_hash": "abc",
            "encrypted_content": "aGVsbG8=",
            "timestamp": "2024-01-01T00:00:00Z",
            "origin_device_id": "dev-1",
            "origin_device_name": "Device",
            "payload_version": 1
        }"#;

        let result: Result<ClipboardMessage, _> = serde_json::from_str(json);
        assert!(
            result.is_err(),
            "V1 payload_version should fail deserialization"
        );
    }
}
