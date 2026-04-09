use uc_app::usecases::pairing::get_p2p_peers_snapshot::P2pPeerSnapshot;
use uc_core::network::PairedDevice;

use crate::api::types::{PairedDeviceDto, PeerSnapshotDto};

pub trait IntoApiDto<T> {
    fn into_api_dto(self) -> T;
}

impl IntoApiDto<PeerSnapshotDto> for P2pPeerSnapshot {
    fn into_api_dto(self) -> PeerSnapshotDto {
        PeerSnapshotDto {
            peer_id: self.peer_id,
            device_name: self.device_name,
            addresses: self.addresses,
            is_paired: self.is_paired,
            connected: self.is_connected,
            pairing_state: self.pairing_state,
        }
    }
}

impl IntoApiDto<PairedDeviceDto> for PairedDevice {
    fn into_api_dto(self) -> PairedDeviceDto {
        PairedDeviceDto {
            peer_id: self.peer_id.to_string(),
            device_name: self.device_name,
            pairing_state: self.pairing_state.to_string(),
            last_seen_at_ms: self
                .last_seen_at
                .map(|timestamp| timestamp.timestamp_millis()),
            connected: false,
        }
    }
}
