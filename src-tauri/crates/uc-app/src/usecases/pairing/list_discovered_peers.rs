use anyhow::Result;
use std::sync::Arc;

use uc_core::network::DiscoveredPeer;
use uc_core::ports::PeerDirectoryPort;

pub struct ListDiscoveredPeers {
    network: Arc<dyn PeerDirectoryPort>,
}

impl ListDiscoveredPeers {
    pub fn new(network: Arc<dyn PeerDirectoryPort>) -> Self {
        Self { network }
    }

    pub async fn execute(&self) -> Result<Vec<DiscoveredPeer>> {
        self.network
            .get_discovered_peers()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list discovered peers: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use uc_core::network::ConnectedPeer;
    use uc_core::ports::PeerDirectoryPort;

    enum DiscoveredOutcome {
        Ok(Vec<DiscoveredPeer>),
        Err(String),
    }

    struct TestNetwork {
        outcome: DiscoveredOutcome,
    }

    #[async_trait]
    impl PeerDirectoryPort for TestNetwork {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            match &self.outcome {
                DiscoveredOutcome::Ok(peers) => Ok(peers.clone()),
                DiscoveredOutcome::Err(message) => Err(anyhow::anyhow!(message.clone())),
            }
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(Vec::new())
        }

        fn local_peer_id(&self) -> String {
            "peer-local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn returns_discovered_peers_on_success() {
        let peers = vec![DiscoveredPeer {
            peer_id: "peer-1".to_string(),
            device_name: Some("Desk".to_string()),
            device_id: Some("123456".to_string()),
            addresses: vec!["/ip4/127.0.0.1".to_string()],
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: false,
        }];

        let usecase = ListDiscoveredPeers::new(Arc::new(TestNetwork {
            outcome: DiscoveredOutcome::Ok(peers.clone()),
        }));

        let result = usecase.execute().await.expect("list discovered peers");
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].peer_id, peers[0].peer_id);
        assert_eq!(result[0].device_name, peers[0].device_name);
    }

    #[tokio::test]
    async fn wraps_errors_with_context() {
        let usecase = ListDiscoveredPeers::new(Arc::new(TestNetwork {
            outcome: DiscoveredOutcome::Err("boom".to_string()),
        }));

        let err = usecase.execute().await.expect_err("expected error");
        let message = err.to_string();
        assert!(message.contains("Failed to list discovered peers"));
        assert!(message.contains("boom"));
    }
}
