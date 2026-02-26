use std::sync::Arc;

use tokio::sync::Mutex;

use uc_core::ids::SpaceId;
use uc_core::ports::space::{CryptoPort, PersistencePort, ProofPort, SpaceAccessTransportPort};
use uc_core::ports::{NetworkPort, TimerPort};
use uc_core::security::space_access::state::SpaceAccessState;
use uc_core::security::SecretString;

use super::executor::SpaceAccessExecutor;
use super::orchestrator::{SpaceAccessError, SpaceAccessOrchestrator};

#[derive(Debug, thiserror::Error)]
pub enum StartSponsorAuthorizationError {
    #[error("space access failed: {0}")]
    SpaceAccess(#[from] SpaceAccessError),
}

pub trait SpaceAccessCryptoFactory: Send + Sync {
    fn build(&self, passphrase: SecretString) -> Box<dyn CryptoPort>;
}

pub struct StartSponsorAuthorization {
    orchestrator: Arc<SpaceAccessOrchestrator>,
    crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
    network: Arc<dyn NetworkPort>,
    transport: Arc<Mutex<dyn SpaceAccessTransportPort>>,
    proof: Arc<dyn ProofPort>,
    timer: Arc<Mutex<dyn TimerPort>>,
    store: Arc<Mutex<dyn PersistencePort>>,
    ttl_secs: u64,
}

impl StartSponsorAuthorization {
    pub fn new(
        orchestrator: Arc<SpaceAccessOrchestrator>,
        crypto_factory: Arc<dyn SpaceAccessCryptoFactory>,
        network: Arc<dyn NetworkPort>,
        transport: Arc<Mutex<dyn SpaceAccessTransportPort>>,
        proof: Arc<dyn ProofPort>,
        timer: Arc<Mutex<dyn TimerPort>>,
        store: Arc<Mutex<dyn PersistencePort>>,
    ) -> Self {
        Self {
            orchestrator,
            crypto_factory,
            network,
            transport,
            proof,
            timer,
            store,
            ttl_secs: 0,
        }
    }

    pub async fn execute(
        &self,
        passphrase: SecretString,
    ) -> Result<SpaceAccessState, StartSponsorAuthorizationError> {
        let space_id = SpaceId::new();
        let pairing_session_id = format!("setup-{}", uuid::Uuid::new_v4());
        let crypto = self.crypto_factory.build(passphrase);
        let mut timer = self.timer.lock().await;
        let mut store = self.store.lock().await;
        let mut transport = self.transport.lock().await;
        let mut executor = SpaceAccessExecutor {
            crypto: crypto.as_ref(),
            net: self.network.as_ref(),
            transport: &mut *transport,
            proof: self.proof.as_ref(),
            timer: &mut *timer,
            store: &mut *store,
        };

        let state = self
            .orchestrator
            .start_sponsor_authorization(&mut executor, pairing_session_id, space_id, self.ttl_secs)
            .await?;

        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicBool, Ordering};
    use uc_core::ids::SessionId as CoreSessionId;
    use uc_core::network::SessionId as NetworkSessionId;
    use uc_core::ports::space::{ProofPort, SpaceAccessTransportPort};
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KeyScope, KeySlot, WrappedMasterKey,
    };
    use uc_core::security::space_access::SpaceAccessProofArtifact;
    use uc_core::security::{MasterKey, SecretString};

    struct MockCryptoPort {
        exported: Arc<AtomicBool>,
    }

    impl MockCryptoPort {
        fn new(exported: Arc<AtomicBool>) -> Self {
            Self { exported }
        }
    }

    struct MockCryptoFactory {
        exported: Arc<AtomicBool>,
    }

    impl SpaceAccessCryptoFactory for MockCryptoFactory {
        fn build(&self, _passphrase: SecretString) -> Box<dyn CryptoPort> {
            Box::new(MockCryptoPort::new(self.exported.clone()))
        }
    }

    #[async_trait]
    impl CryptoPort for MockCryptoPort {
        async fn generate_nonce32(&self) -> [u8; 32] {
            [7u8; 32]
        }

        async fn export_keyslot_blob(&self, _space_id: &SpaceId) -> anyhow::Result<KeySlot> {
            self.exported.store(true, Ordering::SeqCst);
            let draft = KeySlot::draft_v1(KeyScope {
                profile_id: "test".to_string(),
            })?;
            Ok(draft.finalize(WrappedMasterKey {
                blob: EncryptedBlob {
                    version: EncryptionFormatVersion::V1,
                    aead: EncryptionAlgo::XChaCha20Poly1305,
                    nonce: vec![0u8; 24],
                    ciphertext: vec![1u8; 32],
                    aad_fingerprint: None,
                },
            }))
        }

        async fn derive_master_key_from_keyslot(
            &self,
            _keyslot_blob: &[u8],
            _passphrase: SecretString,
        ) -> anyhow::Result<MasterKey> {
            MasterKey::from_bytes(&[0u8; 32]).map_err(|e| anyhow::anyhow!(e))
        }
    }

    struct MockNetworkPort;

    #[async_trait]
    impl NetworkPort for MockNetworkPort {
        async fn send_clipboard(
            &self,
            _peer_id: &str,
            _encrypted_data: Vec<u8>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn broadcast_clipboard(&self, _encrypted_data: Vec<u8>) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_clipboard(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::ClipboardMessage>>
        {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }

        async fn get_discovered_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::DiscoveredPeer>> {
            Ok(vec![])
        }

        async fn get_connected_peers(
            &self,
        ) -> anyhow::Result<Vec<uc_core::network::ConnectedPeer>> {
            Ok(vec![])
        }

        fn local_peer_id(&self) -> String {
            "local".to_string()
        }

        async fn announce_device_name(&self, _device_name: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn open_pairing_session(
            &self,
            _peer_id: String,
            _session_id: String,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_pairing_on_session(
            &self,
            _session_id: String,
            _message: uc_core::network::PairingMessage,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn close_pairing_session(
            &self,
            _session_id: String,
            _reason: Option<String>,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn unpair_device(&self, _peer_id: String) -> anyhow::Result<()> {
            Ok(())
        }

        async fn subscribe_events(
            &self,
        ) -> anyhow::Result<tokio::sync::mpsc::Receiver<uc_core::network::NetworkEvent>> {
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(rx)
        }
    }

    struct MockTimerPort;

    #[async_trait]
    impl TimerPort for MockTimerPort {
        async fn start(
            &mut self,
            _session_id: &CoreSessionId,
            _ttl_secs: u64,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn stop(&mut self, _session_id: &CoreSessionId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockStorePort;

    #[async_trait]
    impl PersistencePort for MockStorePort {
        async fn persist_joiner_access(
            &mut self,
            _space_id: &SpaceId,
            _peer_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        async fn persist_sponsor_access(
            &mut self,
            _space_id: &SpaceId,
            _peer_id: &str,
        ) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn start_sponsor_authorization_exports_keyslot() {
        let exported = Arc::new(AtomicBool::new(false));
        let crypto_factory = Arc::new(MockCryptoFactory {
            exported: exported.clone(),
        });
        let network = Arc::new(MockNetworkPort);
        let transport = Arc::new(Mutex::new(MockTransportPort));
        let proof = Arc::new(MockProofPort);
        let timer = Arc::new(Mutex::new(MockTimerPort));
        let store = Arc::new(Mutex::new(MockStorePort));
        let orchestrator = Arc::new(SpaceAccessOrchestrator::new());

        let uc = StartSponsorAuthorization::new(
            orchestrator,
            crypto_factory,
            network,
            transport,
            proof,
            timer,
            store,
        );

        let result = uc.execute(SecretString::from("passphrase")).await;

        assert!(
            result.is_ok(),
            "expected sponsor authorization start to succeed"
        );
        assert!(exported.load(Ordering::SeqCst));
    }

    struct MockTransportPort;

    #[async_trait]
    impl SpaceAccessTransportPort for MockTransportPort {
        async fn send_offer(&mut self, _session_id: &NetworkSessionId) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_proof(&mut self, _session_id: &NetworkSessionId) -> anyhow::Result<()> {
            Ok(())
        }

        async fn send_result(&mut self, _session_id: &NetworkSessionId) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockProofPort;

    #[async_trait]
    impl ProofPort for MockProofPort {
        async fn build_proof(
            &self,
            pairing_session_id: &CoreSessionId,
            space_id: &SpaceId,
            challenge_nonce: [u8; 32],
            _master_key: &MasterKey,
        ) -> anyhow::Result<SpaceAccessProofArtifact> {
            Ok(SpaceAccessProofArtifact {
                pairing_session_id: pairing_session_id.clone(),
                space_id: space_id.clone(),
                challenge_nonce,
                proof_bytes: vec![],
            })
        }

        async fn verify_proof(
            &self,
            _proof: &SpaceAccessProofArtifact,
            _expected_nonce: [u8; 32],
        ) -> anyhow::Result<bool> {
            Ok(true)
        }
    }
}
