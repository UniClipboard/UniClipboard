use std::sync::Arc;

use uc_core::ports::PeerDirectoryPort;

pub struct GetLocalPeerId {
    network: Arc<dyn PeerDirectoryPort>,
}

impl GetLocalPeerId {
    pub fn new(network: Arc<dyn PeerDirectoryPort>) -> Self {
        Self { network }
    }

    pub fn execute(&self) -> String {
        self.network.local_peer_id()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use uc_core::network::{ConnectedPeer, DiscoveredPeer};
    use uc_core::ports::PeerDirectoryPort;

    struct TestNetwork {
        peer_id: String,
    }

    #[async_trait]
    impl PeerDirectoryPort for TestNetwork {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(Vec::new())
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(Vec::new())
        }

        fn local_peer_id(&self) -> String {
            self.peer_id.clone()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn returns_local_peer_id_from_network() {
        let usecase = GetLocalPeerId::new(Arc::new(TestNetwork {
            peer_id: "peer-123".to_string(),
        }));

        let peer_id = usecase.execute();
        assert_eq!(peer_id, "peer-123");
    }
}
