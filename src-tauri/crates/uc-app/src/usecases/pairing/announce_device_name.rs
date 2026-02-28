use anyhow::Result;
use std::sync::Arc;
use tracing::{info_span, Instrument};
use uc_core::ports::PeerDirectoryPort;

/// Use case for announcing the local device name over the network.
pub struct AnnounceDeviceName {
    network: Arc<dyn PeerDirectoryPort>,
}

impl AnnounceDeviceName {
    /// Create a new AnnounceDeviceName use case.
    pub fn new(network: Arc<dyn PeerDirectoryPort>) -> Self {
        Self { network }
    }

    /// Execute the use case.
    pub async fn execute(&self, device_name: String) -> Result<()> {
        let span = info_span!("usecase.announce_device_name.execute");

        async { self.network.announce_device_name(device_name).await }
            .instrument(span)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::AnnounceDeviceName;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};
    use uc_core::network::{ConnectedPeer, DiscoveredPeer};
    use uc_core::ports::PeerDirectoryPort;

    struct TestNetwork {
        called: Arc<Mutex<Vec<String>>>,
        result: anyhow::Result<()>,
    }

    #[async_trait]
    impl PeerDirectoryPort for TestNetwork {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(vec![])
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "local-peer".to_string()
        }

        async fn announce_device_name(&self, device_name: String) -> anyhow::Result<()> {
            let mut called = self.called.lock().expect("called lock");
            called.push(device_name);
            match &self.result {
                Ok(()) => Ok(()),
                Err(error) => Err(anyhow::anyhow!(error.to_string())),
            }
        }
    }

    #[tokio::test]
    async fn announce_device_name_invokes_network_port() {
        let called = Arc::new(Mutex::new(Vec::new()));
        let network = Arc::new(TestNetwork {
            called: called.clone(),
            result: Ok(()),
        });
        let uc = AnnounceDeviceName::new(network);

        uc.execute("Desk".to_string())
            .await
            .expect("announce device name");

        let called = called.lock().expect("called lock");
        assert_eq!(called.as_slice(), ["Desk".to_string()]);
    }

    #[tokio::test]
    async fn announce_device_name_propagates_error() {
        let called = Arc::new(Mutex::new(Vec::new()));
        let network = Arc::new(TestNetwork {
            called: called.clone(),
            result: Err(anyhow::anyhow!("announce failed")),
        });
        let uc = AnnounceDeviceName::new(network);

        let result = uc.execute("Desk".to_string()).await;

        assert!(result.is_err());
    }
}
