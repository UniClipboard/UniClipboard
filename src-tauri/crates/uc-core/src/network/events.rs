use std::path::PathBuf;

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

/// Live per-peer runtime state driven by the recovery coordinator.
///
/// `PeerRuntimeState` is the user-facing three-state model defined in the
/// Connection Stability Recovery PRD
/// (`docs/p2p/2026-04-11-connection-stability-recovery-prd.md`). It is kept
/// distinct from [`crate::device::DeviceStatus`], which is a database-adjacent
/// DTO.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PeerRuntimeState {
    Online,
    Recovering,
    Offline,
}

/// What triggered a recovery cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryTrigger {
    /// mDNS record for a paired peer expired.
    MdnsExpired,
    /// Several consecutive dial failures to the same paired peer.
    DialFailureStreak,
    /// First outbound attempt after a sustained idle window.
    FirstAttemptAfterIdle,
    /// Local device has just resumed from sleep.
    WakeFromSleep,
    /// Local network interface or IP address changed.
    NetworkInterfaceChanged,
}

/// Transport-level proof that justified closing a recovery cycle as recovered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryProof {
    /// A recovery probe's business-stream open call returned success.
    BusinessStreamOpen,
    /// A fresh libp2p `ConnectionEstablished` event arrived from the swarm.
    ConnectionEstablished,
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

    // Recovery events (Connection Stability Recovery wave 1).
    // See docs/p2p/2026-04-11-connection-stability-recovery-prd.md.
    /// Per-peer runtime state changed. The coordinator is the only producer.
    PeerStateChanged {
        peer_id: String,
        state: PeerRuntimeState,
        /// Present while a recovery cycle is active.
        cycle_id: Option<String>,
    },
    /// A recovery cycle has begun. User-facing state may still be `Online`
    /// during the initial silent phase.
    PeerRecoveryStarted {
        peer_id: String,
        cycle_id: String,
        trigger: RecoveryTrigger,
    },
    /// A recovery cycle ended successfully with transport-level proof.
    PeerRecovered {
        peer_id: String,
        cycle_id: String,
        elapsed_ms: u64,
        proof: RecoveryProof,
    },
    /// A recovery cycle exhausted its escalation ladder without restoring the
    /// peer. The user-facing state transitions to `Offline` at this point.
    PeerRecoveryFailed {
        peer_id: String,
        cycle_id: String,
        elapsed_ms: u64,
        /// Last escalation level reached (1, 2, or 3).
        last_escalation: u8,
    },

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
    // File transfer lifecycle events
    FileTransferStarted {
        transfer_id: String,
        peer_id: String,
        filename: String,
        file_size: u64,
    },
    FileTransferCompleted {
        transfer_id: String,
        peer_id: String,
        filename: String,
        file_path: PathBuf,
        batch_id: Option<String>,
        batch_total: Option<u32>,
    },
    FileTransferFailed {
        transfer_id: String,
        peer_id: String,
        error: String,
    },
    FileTransferCancelled {
        transfer_id: String,
        peer_id: String,
        reason: String,
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
    fn file_transfer_events_serialize_round_trip() {
        let events = vec![
            NetworkEvent::FileTransferStarted {
                transfer_id: "xfer-1".to_string(),
                peer_id: "peer-abc".to_string(),
                filename: "report.pdf".to_string(),
                file_size: 1_048_576,
            },
            NetworkEvent::FileTransferCompleted {
                transfer_id: "xfer-1".to_string(),
                peer_id: "peer-abc".to_string(),
                filename: "report.pdf".to_string(),
                file_path: PathBuf::from("/tmp/file-cache/xfer-1_report.pdf"),
                batch_id: None,
                batch_total: None,
            },
            NetworkEvent::FileTransferFailed {
                transfer_id: "xfer-2".to_string(),
                peer_id: "peer-xyz".to_string(),
                error: "connection lost".to_string(),
            },
            NetworkEvent::FileTransferCancelled {
                transfer_id: "xfer-3".to_string(),
                peer_id: "peer-def".to_string(),
                reason: "user cancelled".to_string(),
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let restored: NetworkEvent = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&restored).unwrap();
            assert_eq!(json, json2);
        }
    }

    #[test]
    fn recovery_events_round_trip_through_serde() {
        let events = vec![
            NetworkEvent::PeerStateChanged {
                peer_id: "peer-1".to_string(),
                state: PeerRuntimeState::Recovering,
                cycle_id: Some("cycle-abc".to_string()),
            },
            NetworkEvent::PeerStateChanged {
                peer_id: "peer-1".to_string(),
                state: PeerRuntimeState::Online,
                cycle_id: None,
            },
            NetworkEvent::PeerRecoveryStarted {
                peer_id: "peer-1".to_string(),
                cycle_id: "cycle-abc".to_string(),
                trigger: RecoveryTrigger::MdnsExpired,
            },
            NetworkEvent::PeerRecoveryStarted {
                peer_id: "peer-2".to_string(),
                cycle_id: "cycle-def".to_string(),
                trigger: RecoveryTrigger::WakeFromSleep,
            },
            NetworkEvent::PeerRecovered {
                peer_id: "peer-1".to_string(),
                cycle_id: "cycle-abc".to_string(),
                elapsed_ms: 7200,
                proof: RecoveryProof::BusinessStreamOpen,
            },
            NetworkEvent::PeerRecovered {
                peer_id: "peer-1".to_string(),
                cycle_id: "cycle-abc".to_string(),
                elapsed_ms: 42_000,
                proof: RecoveryProof::ConnectionEstablished,
            },
            NetworkEvent::PeerRecoveryFailed {
                peer_id: "peer-3".to_string(),
                cycle_id: "cycle-xyz".to_string(),
                elapsed_ms: 120_000,
                last_escalation: 3,
            },
        ];

        for event in &events {
            let json = serde_json::to_string(event).unwrap();
            let restored: NetworkEvent = serde_json::from_str(&json).unwrap();
            let json2 = serde_json::to_string(&restored).unwrap();
            assert_eq!(json, json2, "round-trip mismatch for {event:?}");
        }
    }

    #[test]
    fn peer_runtime_state_serializes_as_snake_case() {
        let json = serde_json::to_string(&PeerRuntimeState::Recovering).unwrap();
        assert_eq!(json, "\"recovering\"");
    }

    #[test]
    fn recovery_trigger_serializes_as_snake_case() {
        let json = serde_json::to_string(&RecoveryTrigger::NetworkInterfaceChanged).unwrap();
        assert_eq!(json, "\"network_interface_changed\"");
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
