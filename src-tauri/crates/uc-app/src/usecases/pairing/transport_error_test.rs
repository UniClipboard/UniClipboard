#[cfg(test)]
mod tests {
    use crate::usecases::pairing::orchestrator::{PairingConfig, PairingOrchestrator};
    use chrono::Utc;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;
    use uc_core::network::paired_device::{PairedDevice, PairingState};
    use uc_core::network::pairing_state_machine::PairingAction;
    use uc_core::network::protocol::PairingChallenge;
    use uc_core::ports::errors::PairedDeviceRepositoryError;
    use uc_core::ports::PairedDeviceRepositoryPort;

    struct MockDeviceRepository;

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for MockDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(None)
        }

        async fn list_all(&self) -> Result<Vec<PairedDevice>, PairedDeviceRepositoryError> {
            Ok(vec![])
        }

        async fn upsert(&self, _device: PairedDevice) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<Utc>,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn transport_error_aborts_waiting_confirm() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        // 1. Initiate pairing
        let session_id = orchestrator
            .initiate_pairing("peer-remote".to_string())
            .await
            .expect("initiate pairing");

        // Consume Request action
        let _ = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("timeout 1")
            .expect("action 1");

        // 2. Simulate receiving Challenge
        let challenge = PairingChallenge {
            session_id: session_id.clone(),
            pin: "123456".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };
        orchestrator
            .handle_challenge(&session_id, "peer-remote", challenge)
            .await
            .expect("handle challenge");

        // Consume ShowVerification action
        let _ = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("timeout challenge action")
            .expect("action");

        // 3. User accepts
        orchestrator
            .user_accept_pairing(&session_id)
            .await
            .expect("user accept");

        // Consume Send Response action
        let _ = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("timeout accept action")
            .expect("action");

        // 4. Simulate Transport Error
        orchestrator
            .handle_transport_error(&session_id, "peer-remote", "Connection reset".to_string())
            .await
            .expect("handle transport error");

        // 5. Verify EmitResult with failure
        let action = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("timeout error action")
            .expect("action");

        if let PairingAction::EmitResult {
            session_id: res_session_id,
            success,
            error,
        } = action
        {
            assert_eq!(res_session_id, session_id);
            assert!(!success);
            assert!(error.unwrap().contains("TransportError"));
        } else {
            panic!("Expected EmitResult, got {:?}", action);
        }
    }
}
