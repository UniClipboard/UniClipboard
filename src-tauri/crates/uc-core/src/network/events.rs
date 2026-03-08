use super::protocol::{ClipboardMessage, PairingMessage, PairingRequest, PairingResponse};
use crate::ports::transfer_progress::TransferProgress;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Network status for P2P connection
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NetworkStatus {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProtocolDirection {
    Inbound,
    Outbound,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProtocolDenyReason {
    NotTrusted,
    Blocked,
    RepoError,
    NotSupported,
}

/// A peer discovered via mDNS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredPeer {
    pub peer_id: String,
    pub device_name: Option<String>,
    /// 6-digit device ID (from Identify agent_version)
    pub device_id: Option<String>,
    pub addresses: Vec<String>,
    pub discovered_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub is_paired: bool,
}

/// A peer we have an active connection with
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectedPeer {
    pub peer_id: String,
    pub device_name: String,
    pub connected_at: DateTime<Utc>,
}

/// Core network events (domain layer)
/// Infrastructure-specific events should extend this
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkEvent {
    // Discovery events
    PeerDiscovered(DiscoveredPeer),
    PeerLost(String), // peer_id
    /// A peer's device name was updated (via DeviceAnnounce message or Identify)
    PeerNameUpdated {
        peer_id: String,
        device_name: String,
    },

    // Connection events
    PeerConnected(ConnectedPeer),
    PeerDisconnected(String), // peer_id

    // Readiness events (protocol-agnostic)
    /// A peer is now ready to receive broadcast messages
    PeerReady {
        peer_id: String,
    },
    /// A peer is no longer ready to receive broadcast messages
    PeerNotReady {
        peer_id: String,
    },

    // Pairing events
    PairingMessageReceived {
        peer_id: String,
        message: PairingMessage,
    },
    PairingRequestReceived {
        session_id: String,
        peer_id: String,
        request: PairingRequest,
    },
    PairingPinReady {
        session_id: String,
        pin: String,
        peer_device_name: String, // Responder's device name (for initiator to display)
        peer_device_id: String,   // Responder's 6-digit device ID
    },
    PairingResponseReceived {
        session_id: String,
        peer_id: String,
        response: PairingResponse,
    },
    PairingComplete {
        session_id: String,
        peer_id: String,
        /// Peer's 6-digit device ID (stable identifier from database)
        peer_device_id: String,
        /// Peer device name (the other device's name, not this device's name)
        peer_device_name: String,
    },
    PairingFailed {
        session_id: String,
        peer_id: String,
        error: String,
    },

    // Clipboard events
    ClipboardReceived(ClipboardMessage),
    ClipboardSent {
        id: String,
        peer_count: usize,
    },

    // Status events
    StatusChanged(NetworkStatus),
    ProtocolDenied {
        peer_id: String,
        protocol_id: String,
        pairing_state: crate::network::PairingState,
        direction: ProtocolDirection,
        reason: ProtocolDenyReason,
    },
    // Transfer progress events
    TransferProgress(TransferProgress),

    #[allow(dead_code)]
    Error(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_status_serialization() {
        let status = NetworkStatus::Connected;
        let json = serde_json::to_string(&status).unwrap();
        let deserialized: NetworkStatus = serde_json::from_str(&json).unwrap();
        assert_eq!(status, deserialized);
    }

    #[test]
    fn test_discovered_peer_serialization() {
        let peer = DiscoveredPeer {
            peer_id: "12D3KooW...".to_string(),
            device_name: Some("Test Device".to_string()),
            device_id: Some("ABC123".to_string()),
            addresses: vec!["/ip4/192.168.1.100/tcp/8000".to_string()],
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: false,
        };

        let json = serde_json::to_string(&peer).unwrap();
        let deserialized: DiscoveredPeer = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.peer_id, peer.peer_id);
        assert_eq!(deserialized.device_name, peer.device_name);
        assert_eq!(deserialized.last_seen, peer.last_seen);
        assert!(!deserialized.is_paired);
    }

    #[test]
    fn transfer_progress_event_serializes_round_trip() {
        let progress = TransferProgress {
            transfer_id: "xfer-1".to_string(),
            peer_id: "peer-abc".to_string(),
            direction: crate::ports::transfer_progress::TransferDirection::Sending,
            chunks_completed: 2,
            total_chunks: 4,
            bytes_transferred: 524288,
            total_bytes: Some(1048576),
        };
        let event = NetworkEvent::TransferProgress(progress);
        let json = serde_json::to_string(&event).unwrap();
        let restored: NetworkEvent = serde_json::from_str(&json).unwrap();
        match restored {
            NetworkEvent::TransferProgress(p) => {
                assert_eq!(p.transfer_id, "xfer-1");
                assert_eq!(p.chunks_completed, 2);
            }
            _ => panic!("expected TransferProgress"),
        }
    }

    #[test]
    fn protocol_denied_event_serializes() {
        use crate::network::PairingState;

        let event = NetworkEvent::ProtocolDenied {
            peer_id: "peer-1".to_string(),
            protocol_id: "/uniclipboard/business/1.0.0".to_string(),
            pairing_state: PairingState::Pending,
            direction: ProtocolDirection::Inbound,
            reason: ProtocolDenyReason::NotTrusted,
        };

        let json = serde_json::to_string(&event).unwrap();
        let restored: NetworkEvent = serde_json::from_str(&json).unwrap();
        match restored {
            NetworkEvent::ProtocolDenied { .. } => {}
            _ => panic!("expected ProtocolDenied"),
        }
    }
}
