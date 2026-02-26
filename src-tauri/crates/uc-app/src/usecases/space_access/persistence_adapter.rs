use std::sync::Arc;

use async_trait::async_trait;
use tracing::info;

use crate::usecases::pairing::staged_paired_device_store;
use uc_core::ids::{PeerId, SpaceId};
use uc_core::network::PairingState;
use uc_core::ports::paired_device_repository::PairedDeviceRepositoryPort;
use uc_core::ports::security::encryption_state::EncryptionStatePort;
use uc_core::ports::space::PersistencePort;

pub struct SpaceAccessPersistenceAdapter {
    encryption_state: Arc<dyn EncryptionStatePort>,
    paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
}

impl SpaceAccessPersistenceAdapter {
    pub fn new(
        encryption_state: Arc<dyn EncryptionStatePort>,
        paired_device_repo: Arc<dyn PairedDeviceRepositoryPort>,
    ) -> Self {
        Self {
            encryption_state,
            paired_device_repo,
        }
    }
}

#[async_trait]
impl PersistencePort for SpaceAccessPersistenceAdapter {
    async fn persist_joiner_access(
        &mut self,
        _space_id: &SpaceId,
        peer_id: &str,
    ) -> anyhow::Result<()> {
        info!(peer_id = %peer_id, "Persisting joiner access and promoting peer trust");
        self.encryption_state.persist_initialized().await?;
        if let Some(mut staged_device) = staged_paired_device_store::take_by_peer_id(peer_id) {
            staged_device.pairing_state = PairingState::Trusted;
            self.paired_device_repo.upsert(staged_device).await?;
            info!(
                peer_id = %peer_id,
                source = "staged",
                target_state = "Trusted",
                "Joiner access persisted with staged paired device"
            );
            return Ok(());
        }

        self.paired_device_repo
            .set_state(&PeerId::from(peer_id), PairingState::Trusted)
            .await?;
        info!(
            peer_id = %peer_id,
            source = "repository",
            target_state = "Trusted",
            "Joiner access persisted with repository state update"
        );
        Ok(())
    }

    async fn persist_sponsor_access(
        &mut self,
        _space_id: &SpaceId,
        peer_id: &str,
    ) -> anyhow::Result<()> {
        info!(peer_id = %peer_id, "Persisting sponsor access and promoting peer trust");
        if let Some(mut staged_device) = staged_paired_device_store::take_by_peer_id(peer_id) {
            staged_device.pairing_state = PairingState::Trusted;
            self.paired_device_repo.upsert(staged_device).await?;
            info!(
                peer_id = %peer_id,
                source = "staged",
                target_state = "Trusted",
                "Sponsor access persisted with staged paired device"
            );
            return Ok(());
        }

        self.paired_device_repo
            .set_state(&PeerId::from(peer_id), PairingState::Trusted)
            .await?;
        info!(
            peer_id = %peer_id,
            source = "repository",
            target_state = "Trusted",
            "Sponsor access persisted with repository state update"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use chrono::Utc;
    use tokio::sync::Mutex;
    use uc_core::network::PairedDevice;
    use uc_core::ports::errors::PairedDeviceRepositoryError;
    use uc_core::security::state::{EncryptionState, EncryptionStateError};

    struct MockEncryptionState;

    #[async_trait]
    impl EncryptionStatePort for MockEncryptionState {
        async fn load_state(&self) -> Result<EncryptionState, EncryptionStateError> {
            Ok(EncryptionState::Initialized)
        }

        async fn persist_initialized(&self) -> Result<(), EncryptionStateError> {
            Ok(())
        }
    }

    #[derive(Default)]
    struct MockPairedDeviceRepo {
        devices: Mutex<HashMap<String, PairedDevice>>,
    }

    impl MockPairedDeviceRepo {
        async fn state_of(&self, peer_id: &str) -> Option<PairingState> {
            self.devices
                .lock()
                .await
                .get(peer_id)
                .map(|device| device.pairing_state.clone())
        }
    }

    #[async_trait]
    impl PairedDeviceRepositoryPort for MockPairedDeviceRepo {
        async fn get_by_peer_id(
            &self,
            peer_id: &PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(self.devices.lock().await.get(peer_id.as_str()).cloned())
        }

        async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(self.devices.lock().await.values().cloned().collect())
        }

        async fn upsert(&self, device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
            self.devices
                .lock()
                .await
                .insert(device.peer_id.to_string(), device);
            Ok(())
        }

        async fn set_state(
            &self,
            peer_id: &PeerId,
            state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            if let Some(existing) = self.devices.lock().await.get_mut(peer_id.as_str()) {
                existing.pairing_state = state;
            }
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(&self, _peer_id: &PeerId) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn pairing_deferred_persistence_promotes_to_trusted_on_proof_verified() {
        staged_paired_device_store::clear();
        let peer_id = PeerId::from("peer-1");
        let repo = Arc::new(MockPairedDeviceRepo::default());

        repo.upsert(PairedDevice {
            peer_id: peer_id.clone(),
            pairing_state: PairingState::Pending,
            identity_fingerprint: "fp-1".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Peer Device".to_string(),
        })
        .await
        .expect("seed pending paired device");

        let mut adapter =
            SpaceAccessPersistenceAdapter::new(Arc::new(MockEncryptionState), repo.clone());

        assert_eq!(
            repo.state_of(peer_id.as_str()).await,
            Some(PairingState::Pending)
        );

        adapter
            .persist_sponsor_access(&SpaceId::from("space-1"), peer_id.as_str())
            .await
            .expect("persist sponsor access");

        assert_eq!(
            repo.state_of(peer_id.as_str()).await,
            Some(PairingState::Trusted)
        );
    }

    #[tokio::test]
    async fn pairing_deferred_persistence_commits_staged_device_on_proof_verified() {
        staged_paired_device_store::clear();
        let peer_id = PeerId::from("peer-staged");
        staged_paired_device_store::stage(
            "session-staged",
            PairedDevice {
                peer_id: peer_id.clone(),
                pairing_state: PairingState::Pending,
                identity_fingerprint: "fp-staged".to_string(),
                paired_at: Utc::now(),
                last_seen_at: None,
                device_name: "Staged Device".to_string(),
            },
        );

        let repo = Arc::new(MockPairedDeviceRepo::default());
        let mut adapter =
            SpaceAccessPersistenceAdapter::new(Arc::new(MockEncryptionState), repo.clone());

        adapter
            .persist_sponsor_access(&SpaceId::from("space-1"), peer_id.as_str())
            .await
            .expect("persist sponsor access");

        assert_eq!(
            repo.state_of(peer_id.as_str()).await,
            Some(PairingState::Trusted)
        );
    }

    #[tokio::test]
    async fn joiner_persistence_promotes_peer_to_trusted() {
        staged_paired_device_store::clear();
        let peer_id = PeerId::from("peer-joiner");
        let repo = Arc::new(MockPairedDeviceRepo::default());
        repo.upsert(PairedDevice {
            peer_id: peer_id.clone(),
            pairing_state: PairingState::Pending,
            identity_fingerprint: "fp-joiner".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Joiner Peer".to_string(),
        })
        .await
        .expect("seed pending paired device");

        let mut adapter =
            SpaceAccessPersistenceAdapter::new(Arc::new(MockEncryptionState), repo.clone());

        adapter
            .persist_joiner_access(&SpaceId::from("space-1"), peer_id.as_str())
            .await
            .expect("persist joiner access");

        assert_eq!(
            repo.state_of(peer_id.as_str()).await,
            Some(PairingState::Trusted)
        );
    }
}
