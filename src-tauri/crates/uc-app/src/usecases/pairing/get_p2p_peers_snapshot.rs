//! GetP2pPeersSnapshot use case - combines discovered, connected, and paired peers into a unified snapshot.

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use uc_core::ports::paired_device_repository::PairedDeviceRepositoryPort;
use uc_core::ports::PeerDirectoryPort;

/// Unified peer snapshot combining discovered, connected, and paired peer information.
#[derive(Debug, Clone)]
pub struct P2pPeerSnapshot {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub is_connected: bool,
    pub pairing_state: String,
    pub identity_fingerprint: String,
}

/// Use case that aggregates discovered peers, connected peers, and paired devices
/// into a single unified snapshot for both GUI and CLI consumption.
///
/// This consolidates the peer aggregation logic that was previously duplicated
/// in Tauri commands (`get_p2p_peers`, `get_paired_peers_with_status`).
pub struct GetP2pPeersSnapshot {
    peer_dir: Arc<dyn PeerDirectoryPort>,
    paired_repo: Arc<dyn PairedDeviceRepositoryPort>,
}

impl GetP2pPeersSnapshot {
    pub fn new(
        peer_dir: Arc<dyn PeerDirectoryPort>,
        paired_repo: Arc<dyn PairedDeviceRepositoryPort>,
    ) -> Self {
        Self {
            peer_dir,
            paired_repo,
        }
    }

    /// Produce a merged snapshot of discovered, connected, and paired peers.
    ///
    /// The result contains one entry per relevant peer (excluding the local peer).
    /// - Discovered peers appear with their discovered addresses and `is_connected` reflecting the connected list.
    /// - If a paired record exists for a discovered peer, the snapshot uses the paired `device_name` when non-empty and includes the paired `pairing_state` and `identity_fingerprint`.
    /// - Peers present only in the paired repository are included with an empty address list, `is_paired = true`, and `is_connected = false`.
    /// The `pairing_state` field is emitted as one of the stable strings `"Pending"`, `"Trusted"`, `"Revoked"`, or `"NotPaired"` when no paired record exists.
    ///
    /// # Returns
    ///
    /// `Ok(Vec<P2pPeerSnapshot>)` with the merged snapshots; `Err` if listing discovered peers, connected peers, or paired devices fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use futures::executor::block_on;
    /// // `svc` must be a constructed GetP2pPeersSnapshot instance
    /// # let svc: GetP2pPeersSnapshot = unimplemented!();
    /// let snapshots = block_on(svc.execute()).unwrap();
    /// println!("Found {} peers", snapshots.len());
    /// ```
    pub async fn execute(&self) -> Result<Vec<P2pPeerSnapshot>> {
        let local_id = self.peer_dir.local_peer_id();

        // 1. List discovered peers
        let discovered = self
            .peer_dir
            .get_discovered_peers()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list discovered peers: {}", e))?;
        // Defense-in-depth: exclude local peer even if adapter missed it
        let discovered: Vec<_> = discovered
            .into_iter()
            .filter(|p| p.peer_id != local_id)
            .collect();

        // 2. List connected peers
        let connected = self
            .peer_dir
            .get_connected_peers()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list connected peers: {}", e))?;
        let connected_ids: HashSet<_> = connected.iter().map(|p| p.peer_id.clone()).collect();

        // 3. List paired devices
        let paired = self
            .paired_repo
            .list_all()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to list paired devices: {}", e))?;
        let paired_map: HashMap<_, _> = paired.iter().map(|p| (p.peer_id.to_string(), p)).collect();

        // 4. Merge into unified snapshot
        let mut snapshots = Vec::new();
        let discovered_ids: HashSet<_> = discovered.iter().map(|p| p.peer_id.clone()).collect();

        // Add discovered peers
        for peer in discovered {
            let peer_id = peer.peer_id.clone();
            let paired_dev = paired_map.get(&peer_id);
            snapshots.push(P2pPeerSnapshot {
                peer_id: peer_id.clone(),
                device_name: paired_dev
                    .and_then(|p| {
                        if p.device_name.is_empty() {
                            None
                        } else {
                            Some(p.device_name.clone())
                        }
                    })
                    .or(peer.device_name),
                addresses: peer.addresses,
                is_paired: peer.is_paired,
                is_connected: connected_ids.contains(&peer_id),
                pairing_state: paired_dev
                    .map(|p| p.pairing_state.to_string())
                    .unwrap_or_else(|| "NotPaired".to_string()),
                identity_fingerprint: paired_dev
                    .map(|p| p.identity_fingerprint.clone())
                    .unwrap_or_default(),
            });
        }

        // Add paired but not discovered peers
        for (peer_id, dev) in paired_map {
            if !connected_ids.contains(&peer_id) && !discovered_ids.contains(&peer_id) {
                snapshots.push(P2pPeerSnapshot {
                    peer_id: peer_id.clone(),
                    device_name: {
                        let name = &dev.device_name;
                        if name.is_empty() {
                            None
                        } else {
                            Some(name.clone())
                        }
                    },
                    addresses: vec![],
                    is_paired: true,
                    is_connected: false,
                    pairing_state: dev.pairing_state.to_string(),
                    identity_fingerprint: dev.identity_fingerprint.clone(),
                });
            }
        }

        Ok(snapshots)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use chrono::Utc;
    use uc_core::network::{ConnectedPeer, DiscoveredPeer, PairedDevice, PairingState};
    use uc_core::ports::{PairedDeviceRepositoryError, PairedDeviceRepositoryPort};
    use uc_core::PeerId;

    struct MockPeerDirectory {
        discovered: Vec<DiscoveredPeer>,
        connected: Vec<ConnectedPeer>,
    }

    #[async_trait]
    impl PeerDirectoryPort for MockPeerDirectory {
        async fn get_discovered_peers(&self) -> anyhow::Result<Vec<DiscoveredPeer>> {
            Ok(self.discovered.clone())
        }

        async fn get_connected_peers(&self) -> anyhow::Result<Vec<ConnectedPeer>> {
            Ok(self.connected.clone())
        }

        fn local_peer_id(&self) -> String {
            "local-peer".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockPairedRepo {
        devices: Vec<PairedDevice>,
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for MockPairedRepo {
        async fn get_by_peer_id(
            &self,
            _peer_id: &PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(self.devices.clone())
        }

        async fn upsert(&self, _device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &PeerId,
            _state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &PeerId,
            _last_seen_at: chrono::DateTime<Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(&self, _peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_empty_snapshot() {
        let peer_dir = Arc::new(MockPeerDirectory {
            discovered: vec![],
            connected: vec![],
        });
        let paired_repo = Arc::new(MockPairedRepo { devices: vec![] });

        let use_case = GetP2pPeersSnapshot::new(peer_dir, paired_repo);
        let result = use_case.execute().await.unwrap();

        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_discovered_peer_marked_paired() {
        let peer_dir = Arc::new(MockPeerDirectory {
            discovered: vec![DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: Some("Discovered Device".to_string()),
                device_id: Some("abc123".to_string()),
                addresses: vec!["/ip4/127.0.0.1".to_string()],
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: true,
            }],
            connected: vec![],
        });
        let paired_repo = Arc::new(MockPairedRepo {
            devices: vec![PairedDevice {
                peer_id: PeerId::from("peer-1"),
                device_name: "Paired Device Name".to_string(),
                pairing_state: PairingState::Trusted,
                identity_fingerprint: "fp123".to_string(),
                paired_at: Utc::now(),
                last_seen_at: Some(Utc::now()),
                sync_settings: None,
            }],
        });

        let use_case = GetP2pPeersSnapshot::new(peer_dir, paired_repo);
        let result = use_case.execute().await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_paired);
        assert_eq!(result[0].pairing_state, "Trusted");
        assert_eq!(result[0].identity_fingerprint, "fp123");
    }

    #[tokio::test]
    async fn test_connected_status_reflected() {
        let peer_dir = Arc::new(MockPeerDirectory {
            discovered: vec![DiscoveredPeer {
                peer_id: "peer-1".to_string(),
                device_name: Some("Device".to_string()),
                device_id: Some("abc".to_string()),
                addresses: vec![],
                discovered_at: Utc::now(),
                last_seen: Utc::now(),
                is_paired: false,
            }],
            connected: vec![ConnectedPeer {
                peer_id: "peer-1".to_string(),
                device_name: "Connected Device".to_string(),
                connected_at: Utc::now(),
            }],
        });
        let paired_repo = Arc::new(MockPairedRepo { devices: vec![] });

        let use_case = GetP2pPeersSnapshot::new(peer_dir, paired_repo);
        let result = use_case.execute().await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_connected);
    }

    #[tokio::test]
    async fn test_snapshot_excludes_local_peer() {
        // local_peer_id() returns "local-peer" per MockPeerDirectory
        let peer_dir = Arc::new(MockPeerDirectory {
            discovered: vec![
                DiscoveredPeer {
                    peer_id: "local-peer".to_string(), // This is the local peer — must be excluded
                    device_name: Some("Self".to_string()),
                    device_id: None,
                    addresses: vec!["/ip4/127.0.0.1/tcp/4001".to_string()],
                    discovered_at: Utc::now(),
                    last_seen: Utc::now(),
                    is_paired: false,
                },
                DiscoveredPeer {
                    peer_id: "remote-peer".to_string(), // This is a remote peer — must be included
                    device_name: Some("Remote".to_string()),
                    device_id: None,
                    addresses: vec!["/ip4/192.168.1.2/tcp/4001".to_string()],
                    discovered_at: Utc::now(),
                    last_seen: Utc::now(),
                    is_paired: false,
                },
            ],
            connected: vec![],
        });
        let paired_repo = Arc::new(MockPairedRepo { devices: vec![] });

        let use_case = GetP2pPeersSnapshot::new(peer_dir, paired_repo);
        let result = use_case.execute().await.unwrap();

        // local-peer must be excluded
        assert_eq!(result.len(), 1, "only remote-peer should be in snapshot");
        assert_eq!(result[0].peer_id, "remote-peer");
        assert!(
            result.iter().all(|p| p.peer_id != "local-peer"),
            "local-peer must not appear in snapshot"
        );
    }

    #[tokio::test]
    async fn test_paired_not_discovered_still_shown() {
        let peer_dir = Arc::new(MockPeerDirectory {
            discovered: vec![],
            connected: vec![],
        });
        let paired_repo = Arc::new(MockPairedRepo {
            devices: vec![PairedDevice {
                peer_id: PeerId::from("paired-only-peer"),
                device_name: "Paired Only Device".to_string(),
                pairing_state: PairingState::Trusted,
                identity_fingerprint: "fp456".to_string(),
                paired_at: Utc::now(),
                last_seen_at: Some(Utc::now()),
                sync_settings: None,
            }],
        });

        let use_case = GetP2pPeersSnapshot::new(peer_dir, paired_repo);
        let result = use_case.execute().await.unwrap();

        assert_eq!(result.len(), 1);
        assert!(result[0].is_paired);
        assert!(!result[0].is_connected);
        assert_eq!(result[0].pairing_state, "Trusted");
    }
}
