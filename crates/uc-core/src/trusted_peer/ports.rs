use async_trait::async_trait;

use crate::ids::DeviceId;

use super::error::TrustedPeerError;
use super::peer::TrustedPeer;

/// Persistence port for trusted peers.
///
/// The port stays intentionally thin: uniqueness and existence semantics
/// (e.g. "already trusted", "cannot distrust a missing peer") are enforced
/// by the use cases in the application layer, not here.
#[async_trait]
pub trait TrustedPeerRepositoryPort: Send + Sync {
    /// Load a trusted peer by its device id. Returns `None` when no record exists.
    async fn get(&self, peer_device_id: &DeviceId)
        -> Result<Option<TrustedPeer>, TrustedPeerError>;

    /// List every trusted peer known locally.
    async fn list(&self) -> Result<Vec<TrustedPeer>, TrustedPeerError>;

    /// Create or replace a trusted-peer record (upsert).
    async fn save(&self, trusted_peer: &TrustedPeer) -> Result<(), TrustedPeerError>;

    /// Remove a trusted-peer record. Returns `true` when a record actually
    /// existed and was removed, `false` otherwise.
    async fn remove(&self, peer_device_id: &DeviceId) -> Result<bool, TrustedPeerError>;
}
