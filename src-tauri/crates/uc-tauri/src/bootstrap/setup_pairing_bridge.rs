//! Daemon-backed setup pairing facade.
//!
//! This facade now consumes the shared realtime-fed setup pairing hub rather than
//! opening its own websocket subscription.

use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tracing::error;

use uc_app::realtime::SetupPairingEventHub;
use uc_app::usecases::pairing::PairingDomainEvent;
use uc_app::usecases::setup::SetupPairingFacadePort;
use uc_daemon_client::{http::DaemonPairingClient, DaemonConnectionState};

pub struct DaemonBackedSetupPairingFacade {
    connection_state: DaemonConnectionState,
    event_hub: Arc<SetupPairingEventHub>,
    participant_ready: bool,
}

impl DaemonBackedSetupPairingFacade {
    pub fn new(
        connection_state: DaemonConnectionState,
        event_hub: Arc<SetupPairingEventHub>,
    ) -> Self {
        Self {
            connection_state,
            event_hub,
            participant_ready: false,
        }
    }

    pub async fn subscribe(&self) -> Result<mpsc::Receiver<PairingDomainEvent>> {
        self.event_hub.subscribe().await
    }

    pub async fn initiate_pairing(&self, peer_id: String) -> Result<String> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        let response = client.initiate_pairing(peer_id).await?;
        Ok(response.session_id)
    }

    pub async fn accept_pairing(&self, session_id: &str) -> Result<()> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        client.accept_pairing(session_id).await?;
        Ok(())
    }

    pub async fn reject_pairing(&self, session_id: &str) -> Result<()> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        client.reject_pairing(session_id).await?;
        Ok(())
    }

    pub async fn cancel_pairing(&self, session_id: &str) -> Result<()> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        client.cancel_pairing(session_id).await?;
        Ok(())
    }

    pub async fn verify_pairing(&self, session_id: &str, pin_matches: bool) -> Result<()> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        client.verify_pairing(session_id, pin_matches).await?;
        Ok(())
    }

    pub async fn set_participant_ready(
        &mut self,
        ready: bool,
        lease_ttl_ms: Option<u64>,
    ) -> Result<()> {
        let client = DaemonPairingClient::new(self.connection_state.clone());
        client
            .set_pairing_participant_ready("setup", ready, lease_ttl_ms)
            .await?;
        self.participant_ready = ready;
        Ok(())
    }

    pub fn is_participant_ready(&self) -> bool {
        self.participant_ready
    }
}

impl Drop for DaemonBackedSetupPairingFacade {
    fn drop(&mut self) {
        if self.participant_ready {
            let connection_state = self.connection_state.clone();
            tokio::spawn(async move {
                let client = DaemonPairingClient::new(connection_state);
                if let Err(error) = client
                    .set_pairing_participant_ready("setup", false, None)
                    .await
                {
                    error!(error = %error, "failed to revoke participant-ready on facade drop");
                }
            });
        }
    }
}

#[async_trait::async_trait]
impl SetupPairingFacadePort for DaemonBackedSetupPairingFacade {
    async fn subscribe(&self) -> Result<mpsc::Receiver<PairingDomainEvent>> {
        self.subscribe().await
    }

    async fn initiate_pairing(&self, peer_id: String) -> Result<String> {
        self.initiate_pairing(peer_id).await
    }

    async fn accept_pairing(&self, session_id: &str) -> Result<()> {
        self.accept_pairing(session_id).await
    }

    async fn reject_pairing(&self, session_id: &str) -> Result<()> {
        self.reject_pairing(session_id).await
    }

    async fn cancel_pairing(&self, session_id: &str) -> Result<()> {
        self.cancel_pairing(session_id).await
    }

    async fn verify_pairing(&self, session_id: &str, pin_matches: bool) -> Result<()> {
        self.verify_pairing(session_id, pin_matches).await
    }
}

pub fn build_setup_pairing_facade(
    connection_state: DaemonConnectionState,
    event_hub: Arc<SetupPairingEventHub>,
) -> Arc<dyn SetupPairingFacadePort> {
    Arc::new(DaemonBackedSetupPairingFacade::new(
        connection_state,
        event_hub,
    ))
}
