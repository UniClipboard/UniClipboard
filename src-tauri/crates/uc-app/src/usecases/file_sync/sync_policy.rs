use std::sync::Arc;

use tracing::{debug, info, warn};
use uc_core::network::paired_device::resolve_sync_settings;
use uc_core::network::DiscoveredPeer;
use uc_core::ports::{PairedDeviceRepositoryPort, SettingsPort};
use uc_core::settings::content_type_filter::{is_content_type_allowed, ContentTypeCategory};
use uc_core::PeerId;

/// Filter peers by sync policy for file content.
///
/// Checks global auto_sync, per-device auto_sync, and file content type toggle.
/// Peers not found in the paired device table are kept (safety fallback).
/// Errors from settings/repo loads are logged and the peer is kept.
pub async fn apply_file_sync_policy(
    settings: &Arc<dyn SettingsPort>,
    paired_device_repo: &Arc<dyn PairedDeviceRepositoryPort>,
    peers: &[DiscoveredPeer],
) -> Vec<DiscoveredPeer> {
    // Load global settings
    let global_settings = match settings.load().await {
        Ok(s) => Some(s),
        Err(err) => {
            warn!(
                error = %err,
                "Failed to load settings for file sync policy; proceeding with all peers"
            );
            None
        }
    };

    // Global master toggle check
    if let Some(ref gs) = global_settings {
        if !gs.sync.auto_sync {
            info!("Global auto_sync disabled; skipping file sync");
            return vec![];
        }
        if !gs.file_sync.file_sync_enabled {
            info!("Global file_sync disabled; skipping file sync policy");
            return vec![];
        }
    }

    let mut result = Vec::with_capacity(peers.len());
    for peer in peers {
        let peer_id = PeerId::from(peer.peer_id.as_str());
        match paired_device_repo.get_by_peer_id(&peer_id).await {
            Ok(Some(device)) => {
                if let Some(ref gs) = global_settings {
                    let effective = resolve_sync_settings(&device, &gs.sync);
                    if !effective.auto_sync {
                        debug!(
                            peer_id = %peer.peer_id,
                            "Skipping file sync: auto_sync disabled"
                        );
                        continue;
                    }
                    // Check file content type toggle
                    if !is_content_type_allowed(ContentTypeCategory::File, &effective.content_types)
                    {
                        debug!(
                            peer_id = %peer.peer_id,
                            "Skipping file sync: file content type disabled"
                        );
                        continue;
                    }
                }
                result.push(peer.clone());
            }
            Ok(None) => result.push(peer.clone()),
            Err(err) => {
                warn!(
                    peer_id = %peer.peer_id,
                    error = %err,
                    "Failed to load device; proceeding with sync"
                );
                result.push(peer.clone());
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::sync::Arc;
    use uc_core::network::{DiscoveredPeer, PairedDevice, PairingState};
    use uc_core::ports::errors::PairedDeviceRepositoryError;
    use uc_core::settings::model::{ContentTypes, Settings, SyncFrequency, SyncSettings};

    // --- Mock types ---

    struct MockSettings {
        settings: Option<Settings>,
    }

    #[async_trait::async_trait]
    impl SettingsPort for MockSettings {
        async fn load(&self) -> anyhow::Result<Settings> {
            match &self.settings {
                Some(s) => Ok(s.clone()),
                None => Err(anyhow::anyhow!("settings load error")),
            }
        }
        async fn save(&self, _settings: &Settings) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockPairedDeviceRepo {
        devices: Vec<PairedDevice>,
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for MockPairedDeviceRepo {
        async fn get_by_peer_id(
            &self,
            peer_id: &PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(self.devices.iter().find(|d| d.peer_id == *peer_id).cloned())
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
            _settings: Option<SyncSettings>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    fn make_peer(id: &str) -> DiscoveredPeer {
        DiscoveredPeer {
            peer_id: id.to_string(),
            device_name: Some(format!("Device {}", id)),
            device_id: None,
            addresses: vec![],
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: true,
        }
    }

    fn make_settings_with_auto_sync(auto_sync: bool) -> Settings {
        let mut s = Settings::default();
        s.sync.auto_sync = auto_sync;
        s
    }

    fn make_paired_device(peer_id: &str, sync_settings: Option<SyncSettings>) -> PairedDevice {
        PairedDevice {
            peer_id: PeerId::from(peer_id),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: format!("Device {}", peer_id),
            sync_settings,
        }
    }

    #[tokio::test]
    async fn test_file_policy_global_off_returns_empty() {
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(make_settings_with_auto_sync(false)),
        });
        let repo: Arc<dyn PairedDeviceRepositoryPort> =
            Arc::new(MockPairedDeviceRepo { devices: vec![] });
        let peers = vec![make_peer("peer-1"), make_peer("peer-2")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_policy_peer_file_disabled_filtered() {
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(make_settings_with_auto_sync(true)),
        });
        let device_sync = SyncSettings {
            auto_sync: true,
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes {
                text: true,
                image: true,
                link: true,
                file: false, // file disabled
                code_snippet: true,
                rich_text: true,
            },
            max_file_size_mb: 10,
        };
        let repo: Arc<dyn PairedDeviceRepositoryPort> = Arc::new(MockPairedDeviceRepo {
            devices: vec![make_paired_device("peer-1", Some(device_sync))],
        });
        let peers = vec![make_peer("peer-1")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_policy_peer_auto_sync_disabled_filtered() {
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(make_settings_with_auto_sync(true)),
        });
        let device_sync = SyncSettings {
            auto_sync: false, // auto_sync off for this device
            sync_frequency: SyncFrequency::Realtime,
            content_types: ContentTypes::default(),
            max_file_size_mb: 10,
        };
        let repo: Arc<dyn PairedDeviceRepositoryPort> = Arc::new(MockPairedDeviceRepo {
            devices: vec![make_paired_device("peer-1", Some(device_sync))],
        });
        let peers = vec![make_peer("peer-1")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_policy_settings_error_keeps_all_peers() {
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings { settings: None });
        let repo: Arc<dyn PairedDeviceRepositoryPort> =
            Arc::new(MockPairedDeviceRepo { devices: vec![] });
        let peers = vec![make_peer("peer-1"), make_peer("peer-2")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_file_policy_unknown_peer_kept() {
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(make_settings_with_auto_sync(true)),
        });
        // No devices in repo -- unknown peer
        let repo: Arc<dyn PairedDeviceRepositoryPort> =
            Arc::new(MockPairedDeviceRepo { devices: vec![] });
        let peers = vec![make_peer("peer-unknown")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].peer_id, "peer-unknown");
    }

    #[tokio::test]
    async fn test_file_policy_global_file_sync_disabled_returns_empty() {
        let mut settings = make_settings_with_auto_sync(true);
        settings.file_sync.file_sync_enabled = false;
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(settings),
        });
        let repo: Arc<dyn PairedDeviceRepositoryPort> =
            Arc::new(MockPairedDeviceRepo { devices: vec![] });
        let peers = vec![make_peer("peer-1"), make_peer("peer-2")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_file_policy_global_file_sync_enabled_keeps_eligible() {
        let mut settings = make_settings_with_auto_sync(true);
        settings.file_sync.file_sync_enabled = true;
        let settings: Arc<dyn SettingsPort> = Arc::new(MockSettings {
            settings: Some(settings),
        });
        let repo: Arc<dyn PairedDeviceRepositoryPort> =
            Arc::new(MockPairedDeviceRepo { devices: vec![] });
        let peers = vec![make_peer("peer-1")];

        let result = apply_file_sync_policy(&settings, &repo, &peers).await;
        assert_eq!(result.len(), 1);
    }
}
