//! Placeholder network port implementation
//! 占位符网络端口实现

use anyhow::Result;
use async_trait::async_trait;
use libp2p::PeerId;
use uc_core::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage,
};
use uc_core::ports::IdentityStorePort;
use uc_core::ports::{
    ClipboardTransportPort, NetworkControlPort, NetworkEventPort, PairingTransportPort,
    PeerDirectoryPort,
};

use crate::identity_store::load_or_create_identity;

/// Placeholder network port implementation
/// 占位符网络端口实现
#[derive(Debug, Clone)]
pub struct PlaceholderNetworkPort {
    local_peer_id: PeerId,
}

impl PlaceholderNetworkPort {
    pub fn new(identity_store: std::sync::Arc<dyn IdentityStorePort>) -> Result<Self> {
        let keypair = load_or_create_identity(identity_store.as_ref())
            .map_err(|e| anyhow::anyhow!("failed to load libp2p identity: {e}"))?;
        let local_peer_id = PeerId::from(keypair.public());
        Ok(Self { local_peer_id })
    }

    pub fn local_peer_id(&self) -> &PeerId {
        &self.local_peer_id
    }
}

#[async_trait]
impl ClipboardTransportPort for PlaceholderNetworkPort {
    async fn send_clipboard(&self, _peer_id: &str, _encrypted_data: Vec<u8>) -> Result<()> {
        Err(anyhow::anyhow!(
            "ClipboardTransportPort::send_clipboard not implemented yet"
        ))
    }

    async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> Result<()> {
        Err(anyhow::anyhow!(
            "ClipboardTransportPort::broadcast_clipboard not implemented yet"
        ))
    }

    async fn subscribe_clipboard(&self) -> Result<tokio::sync::mpsc::Receiver<ClipboardMessage>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }
}

#[async_trait]
impl PeerDirectoryPort for PlaceholderNetworkPort {
    // === Peer operations ===

    async fn get_discovered_peers(&self) -> Result<Vec<DiscoveredPeer>> {
        Ok(Vec::new())
    }

    async fn get_connected_peers(&self) -> Result<Vec<ConnectedPeer>> {
        Ok(Vec::new())
    }

    fn local_peer_id(&self) -> String {
        self.local_peer_id.to_string()
    }

    async fn announce_device_name(&self, _device_name: String) -> Result<()> {
        Err(anyhow::anyhow!(
            "PeerDirectoryPort::announce_device_name not implemented yet"
        ))
    }
}

#[async_trait]
impl PairingTransportPort for PlaceholderNetworkPort {
    async fn open_pairing_session(&self, _peer_id: String, _session_id: String) -> Result<()> {
        Err(anyhow::anyhow!(
            "PairingTransportPort::open_pairing_session not implemented yet"
        ))
    }

    async fn send_pairing_on_session(&self, _message: PairingMessage) -> Result<()> {
        Err(anyhow::anyhow!(
            "PairingTransportPort::send_pairing_on_session not implemented yet"
        ))
    }

    async fn close_pairing_session(
        &self,
        _session_id: String,
        _reason: Option<String>,
    ) -> Result<()> {
        Err(anyhow::anyhow!(
            "PairingTransportPort::close_pairing_session not implemented yet"
        ))
    }

    async fn unpair_device(&self, _peer_id: String) -> Result<()> {
        Err(anyhow::anyhow!(
            "PairingTransportPort::unpair_device not implemented yet"
        ))
    }
}

#[async_trait]
impl NetworkEventPort for PlaceholderNetworkPort {
    async fn subscribe_events(&self) -> Result<tokio::sync::mpsc::Receiver<NetworkEvent>> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(rx)
    }
}

#[async_trait]
impl NetworkControlPort for PlaceholderNetworkPort {
    async fn start_network(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct TestIdentityStore {
        data: Mutex<Option<Vec<u8>>>,
    }

    impl IdentityStorePort for TestIdentityStore {
        fn load_identity(&self) -> Result<Option<Vec<u8>>, uc_core::ports::IdentityStoreError> {
            let guard = self.data.lock().expect("lock test identity store");
            Ok(guard.clone())
        }

        fn store_identity(
            &self,
            identity: &[u8],
        ) -> Result<(), uc_core::ports::IdentityStoreError> {
            let mut guard = self.data.lock().expect("lock test identity store");
            *guard = Some(identity.to_vec());
            Ok(())
        }
    }

    #[test]
    fn local_peer_id_returns_typed_peer_id() {
        let adapter = PlaceholderNetworkPort::new(Arc::new(TestIdentityStore::default()))
            .expect("create placeholder network port");

        let peer_id: &PeerId = adapter.local_peer_id();

        assert!(!peer_id.to_string().is_empty());
    }
}
