#[cfg(test)]
mod tests {
    use crate::testing::NoopPairedDeviceRepository;
    use crate::usecases::pairing::orchestrator::{PairingConfig, PairingOrchestrator};
    use crate::usecases::pairing::staged_paired_device_store::StagedPairedDeviceStore;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::time::timeout;
    use uc_core::network::pairing_state_machine::PairingAction;
    use uc_core::network::protocol::PairingChallenge;

    #[tokio::test]
    async fn transport_error_aborts_waiting_confirm() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
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
            assert!(error.unwrap().contains("Connection reset"));
        } else {
            panic!("Expected EmitResult, got {:?}", action);
        }
    }
}
