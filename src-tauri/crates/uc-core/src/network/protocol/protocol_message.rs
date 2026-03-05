use serde::{Deserialize, Serialize};

use super::{ClipboardMessage, DeviceAnnounceMessage, HeartbeatMessage, PairingMessage};

/// P2P protocol messages for UniClipboard
/// Based on decentpaste protocol with UniClipboard-specific adaptations
#[derive(Clone, Serialize, Deserialize)]
pub enum ProtocolMessage {
    Pairing(PairingMessage),
    Clipboard(ClipboardMessage),
    Heartbeat(HeartbeatMessage),
    /// Announces device name to all peers on the network.
    /// Used when device name is changed in settings.
    DeviceAnnounce(DeviceAnnounceMessage),
}

impl ProtocolMessage {
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }

    /// Produce two-segment wire format bytes.
    ///
    /// All messages get a 4-byte LE length prefix before the JSON header.
    /// If `trailing_payload` is `Some`, the raw bytes are appended after the JSON.
    /// Used for V2 clipboard messages where the binary payload follows the JSON header.
    pub fn frame_to_bytes(
        &self,
        trailing_payload: Option<&[u8]>,
    ) -> Result<Vec<u8>, serde_json::Error> {
        let json_bytes = serde_json::to_vec(self)?;
        let json_len = json_bytes.len() as u32;
        let trailing_len = trailing_payload.map_or(0, |p| p.len());
        let mut buf = Vec::with_capacity(4 + json_bytes.len() + trailing_len);
        buf.extend_from_slice(&json_len.to_le_bytes());
        buf.extend_from_slice(&json_bytes);
        if let Some(payload) = trailing_payload {
            buf.extend_from_slice(payload);
        }
        Ok(buf)
    }
}

// Custom Debug implementations to redact sensitive fields

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_device_announce() -> ProtocolMessage {
        ProtocolMessage::DeviceAnnounce(DeviceAnnounceMessage {
            peer_id: "peer-1".to_string(),
            device_name: "Test Device".to_string(),
            timestamp: Utc::now(),
        })
    }

    fn make_clipboard_header() -> ProtocolMessage {
        ProtocolMessage::Clipboard(ClipboardMessage {
            id: "msg-1".to_string(),
            content_hash: "hash-abc".to_string(),
            encrypted_content: vec![], // V3: empty in JSON header
            timestamp: Utc::now(),
            origin_device_id: "dev-1".to_string(),
            origin_device_name: "Test Device".to_string(),
            payload_version: super::super::ClipboardPayloadVersion::V3,
        })
    }

    #[test]
    fn frame_to_bytes_roundtrip_no_trailing() {
        let msg = make_device_announce();
        let framed = msg.frame_to_bytes(None).expect("frame_to_bytes");

        // First 4 bytes are the JSON length as u32 LE
        assert!(framed.len() >= 4);
        let json_len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;
        assert_eq!(framed.len(), 4 + json_len, "no trailing bytes expected");

        // JSON portion parses correctly
        let json_bytes = &framed[4..4 + json_len];
        let decoded = ProtocolMessage::from_bytes(json_bytes).expect("from_bytes on JSON portion");
        match decoded {
            ProtocolMessage::DeviceAnnounce(da) => {
                assert_eq!(da.peer_id, "peer-1");
                assert_eq!(da.device_name, "Test Device");
            }
            _ => panic!("expected DeviceAnnounce"),
        }
    }

    #[test]
    fn frame_to_bytes_with_trailing_payload() {
        let msg = make_clipboard_header();
        let raw_payload = b"raw-payload";
        let framed = msg
            .frame_to_bytes(Some(raw_payload))
            .expect("frame_to_bytes");

        // First 4 bytes are JSON length
        let json_len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;

        // Total length = 4 (prefix) + json_len + raw_payload.len()
        assert_eq!(framed.len(), 4 + json_len + raw_payload.len());

        // JSON portion parses as ClipboardMessage with empty encrypted_content
        let json_bytes = &framed[4..4 + json_len];
        let decoded = ProtocolMessage::from_bytes(json_bytes).expect("from_bytes");
        match decoded {
            ProtocolMessage::Clipboard(cm) => {
                assert!(
                    cm.encrypted_content.is_empty(),
                    "V2 header must have empty encrypted_content"
                );
                assert_eq!(cm.id, "msg-1");
            }
            _ => panic!("expected Clipboard"),
        }

        // Trailing bytes are the raw payload
        let trailing = &framed[4 + json_len..];
        assert_eq!(trailing, raw_payload);
    }

    #[test]
    fn frame_to_bytes_empty_trailing() {
        let msg = make_device_announce();
        let framed = msg
            .frame_to_bytes(Some(&[]))
            .expect("frame_to_bytes with empty trailing");

        let json_len = u32::from_le_bytes(framed[0..4].try_into().unwrap()) as usize;
        // No trailing data when trailing payload is empty
        assert_eq!(framed.len(), 4 + json_len);

        // Still parses correctly
        let json_bytes = &framed[4..4 + json_len];
        let decoded = ProtocolMessage::from_bytes(json_bytes).expect("from_bytes");
        match decoded {
            ProtocolMessage::DeviceAnnounce(_) => {} // ok
            _ => panic!("expected DeviceAnnounce"),
        }
    }
}

impl std::fmt::Debug for ProtocolMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pairing(msg) => f.debug_tuple("Pairing").field(msg).finish(),
            Self::Clipboard(msg) => f.debug_tuple("Clipboard").field(msg).finish(),
            Self::Heartbeat(msg) => f.debug_tuple("Heartbeat").field(msg).finish(),
            Self::DeviceAnnounce(msg) => f.debug_tuple("DeviceAnnounce").field(msg).finish(),
        }
    }
}
