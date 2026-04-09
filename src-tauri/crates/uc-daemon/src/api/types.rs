pub use uc_daemon_contract::api::types::*;

use uc_app::usecases::pairing::get_p2p_peers_snapshot::P2pPeerSnapshot;
use uc_core::network::{PairedDevice, PairingState};

impl From<P2pPeerSnapshot> for PeerSnapshotDto {
    fn from(value: P2pPeerSnapshot) -> Self {
        Self {
            peer_id: value.peer_id,
            device_name: value.device_name,
            addresses: value.addresses,
            is_paired: value.is_paired,
            connected: value.is_connected,
            pairing_state: value.pairing_state,
        }
    }
}

impl From<PairedDevice> for PairedDeviceDto {
    fn from(value: PairedDevice) -> Self {
        Self {
            peer_id: value.peer_id.to_string(),
            device_name: value.device_name,
            pairing_state: pairing_state_to_string(&value.pairing_state),
            last_seen_at_ms: value
                .last_seen_at
                .map(|timestamp| timestamp.timestamp_millis()),
            connected: false,
        }
    }
}

fn pairing_state_to_string(state: &PairingState) -> String {
    match state {
        PairingState::Pending => "Pending".to_string(),
        PairingState::Trusted => "Trusted".to_string(),
        PairingState::Revoked => "Revoked".to_string(),
    }
}