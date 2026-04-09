pub use uc_daemon_contract::api::types::*;

use uc_app::usecases::pairing::get_p2p_peers_snapshot::P2pPeerSnapshot;
use uc_core::network::{PairedDevice, PairingState};

impl From<P2pPeerSnapshot> for PeerSnapshotDto {
    /// Convert a p2p peer snapshot into its DTO representation.
    ///
    /// Maps the snapshot's fields into a `PeerSnapshotDto` with corresponding values copied directly from the source.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// // Given a `P2pPeerSnapshot` value `snapshot`, convert it into the DTO:
    /// let dto: PeerSnapshotDto = snapshot.into();
    /// ```
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
    /// Converts a `PairedDevice` into a `PairedDeviceDto`.
    ///
    /// The DTO's fields are populated from the source: `peer_id` is converted to a `String`,
    /// `device_name` is copied, `pairing_state` is converted to its string representation,
    /// `last_seen_at_ms` is set to the source timestamp in milliseconds if present, and
    /// `connected` is set to `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// // Given a `PairedDevice` value `pd`, convert it to the DTO:
    /// let dto: PairedDeviceDto = pd.into();
    /// assert_eq!(dto.connected, false);
    /// ```
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

/// Convert a PairingState into its capitalized string representation.
///
/// Returns the state's name as a `String`: `"Pending"`, `"Trusted"`, or `"Revoked"`.
///
/// # Examples
///
/// ```
/// use uc_core::network::PairingState;
///
/// assert_eq!(super::pairing_state_to_string(&PairingState::Pending), "Pending");
/// assert_eq!(super::pairing_state_to_string(&PairingState::Trusted), "Trusted");
/// assert_eq!(super::pairing_state_to_string(&PairingState::Revoked), "Revoked");
/// ```
fn pairing_state_to_string(state: &PairingState) -> String {
    match state {
        PairingState::Pending => "Pending".to_string(),
        PairingState::Trusted => "Trusted".to_string(),
        PairingState::Revoked => "Revoked".to_string(),
    }
}