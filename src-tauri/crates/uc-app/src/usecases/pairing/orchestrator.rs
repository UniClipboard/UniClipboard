//! Pairing protocol orchestrator
//!
//! This module coordinates the pairing state machine by converting network events,
//! user inputs, and timer events into state machine events, then executing the
//! resulting actions.
//!
//! # Architecture
//!
//! ```text
//! Network/User/Timer Events
//!   |
//! PairingOrchestrator (thin coordinator)
//!   |--- PairingSessionManager (session lifecycle)
//!   |--- PairingProtocolHandler (action execution)
//!   |
//! PairingStateMachine (pure state transitions)
//!   |
//! PairingActions (executed by protocol handler)
//!   |
//! Network/User/Persistence side effects
//! ```

use anyhow::Result;

use super::{PairingDomainEvent, PairingEventPort, PairingFacade};
use crate::usecases::pairing::staged_paired_device_store::StagedPairedDeviceStore;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{info_span, Instrument};

use super::protocol_handler::PairingProtocolHandler;
use super::session_manager::{LocalDeviceInfo, PairingSessionContext, PairingSessionManager};

use uc_core::{
    network::{
        pairing_state_machine::{
            PairingAction, PairingEvent, PairingRole, PairingState, SessionId,
        },
        protocol::{
            PairingChallenge, PairingChallengeResponse, PairingConfirm, PairingKeyslotOffer,
            PairingRequest,
        },
    },
    ports::PairedDeviceRepositoryPort,
    settings::model::Settings,
};

/// Pairing orchestrator configuration
#[derive(Debug, Clone)]
pub struct PairingConfig {
    /// Step timeout (seconds)
    pub step_timeout_secs: i64,
    /// User verification timeout (seconds)
    pub user_verification_timeout_secs: i64,
    /// Session timeout (seconds)
    pub session_timeout_secs: i64,
    /// Max retries
    pub max_retries: u8,
    /// Protocol version
    pub protocol_version: String,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PairingConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        let pairing = &settings.pairing;
        let step = pairing.step_timeout.as_secs().min(i64::MAX as u64) as i64;
        let verify = pairing
            .user_verification_timeout
            .as_secs()
            .min(i64::MAX as u64) as i64;
        let session = pairing.session_timeout.as_secs().min(i64::MAX as u64) as i64;

        Self {
            step_timeout_secs: step.max(1),
            user_verification_timeout_secs: verify.max(1),
            session_timeout_secs: session.max(1),
            max_retries: pairing.max_retries.max(1),
            protocol_version: pairing.protocol_version.clone(),
        }
    }
}

/// Pairing orchestrator -- thin coordinator delegating to session manager and protocol handler.
#[derive(Clone)]
pub struct PairingOrchestrator {
    /// Session lifecycle manager
    session_manager: PairingSessionManager,
    /// Protocol action handler
    protocol_handler: PairingProtocolHandler,
}

/// Re-export PairingPeerInfo as a public type for API compatibility.
pub use super::session_manager::PairingPeerInfo;

impl PairingOrchestrator {
    /// Create a new pairing orchestrator.
    pub fn new(
        config: PairingConfig,
        device_repo: Arc<dyn PairedDeviceRepositoryPort + Send + Sync + 'static>,
        local_device_name: String,
        local_device_id: String,
        local_peer_id: String,
        local_identity_pubkey: Vec<u8>,
        staged_store: Arc<StagedPairedDeviceStore>,
    ) -> (Self, mpsc::Receiver<PairingAction>) {
        let (action_tx, action_rx) = mpsc::channel(100);
        let event_senders: Arc<Mutex<Vec<mpsc::Sender<PairingDomainEvent>>>> =
            Arc::new(Mutex::new(Vec::new()));

        let local_identity = LocalDeviceInfo {
            device_name: local_device_name,
            device_id: local_device_id,
            identity_pubkey: local_identity_pubkey,
            peer_id: local_peer_id,
        };

        let session_manager = PairingSessionManager::new(config, local_identity);
        let protocol_handler =
            PairingProtocolHandler::new(action_tx, device_repo, staged_store, event_senders);

        let orchestrator = Self {
            session_manager,
            protocol_handler,
        };

        (orchestrator, action_rx)
    }

    /// Initiate pairing (Initiator role).
    pub async fn initiate_pairing(&self, peer_id: String) -> Result<SessionId> {
        let span = info_span!("pairing.initiate", peer_id = %peer_id);
        async {
            let mut state_machine = self.session_manager.new_state_machine();
            let (state, actions) = state_machine.handle_event(
                PairingEvent::StartPairing {
                    role: PairingRole::Initiator,
                    peer_id: peer_id.clone(),
                },
                Utc::now(),
            );

            let session_id = match state {
                PairingState::RequestSent { session_id } => session_id,
                _ => {
                    return Err(anyhow::anyhow!(
                        "unexpected state after StartPairing: {:?}",
                        state
                    ))
                }
            };
            self.session_manager
                .record_session_peer(&session_id, peer_id.clone(), None)
                .await;

            let context = PairingSessionContext {
                state_machine,
                created_at: Utc::now(),
                timers: tokio::sync::Mutex::new(HashMap::new()),
            };

            self.session_manager
                .insert_session(session_id.clone(), context)
                .await;

            for action in actions {
                self.execute_action(&session_id, &peer_id, action).await?;
            }

            Ok(session_id)
        }
        .instrument(span)
        .await
    }

    /// Handle incoming pairing request (Responder role).
    pub async fn handle_incoming_request(
        &self,
        peer_id: String,
        request: PairingRequest,
    ) -> Result<()> {
        let expected_local_peer_id = self.session_manager.local_peer_id().to_string();
        if request.peer_id != expected_local_peer_id {
            tracing::warn!(
                session_id = %request.session_id,
                sender_peer_id = %peer_id,
                request_target_peer_id = %request.peer_id,
                expected_local_peer_id = %expected_local_peer_id,
                request_device_id = %request.device_id,
                request_device_name = %request.device_name,
                "incoming pairing request target peer_id mismatch"
            );
            return Err(anyhow::anyhow!(
                "Request target peer_id mismatch: expected {}, got {}",
                expected_local_peer_id,
                request.peer_id
            ));
        }

        let session_id = request.session_id.clone();
        let span = info_span!(
            "pairing.handle_request",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            tracing::info!(
                session_id = %session_id,
                sender_peer_id = %peer_id,
                request_target_peer_id = %request.peer_id,
                expected_local_peer_id = %expected_local_peer_id,
                request_device_id = %request.device_id,
                request_device_name = %request.device_name,
                "validated inbound pairing request target peer_id"
            );
            self.session_manager
                .record_session_peer(
                    &session_id,
                    peer_id.clone(),
                    Some(request.device_name.clone()),
                )
                .await;

            let mut state_machine = self.session_manager.new_state_machine();
            let (_state, actions) = state_machine.handle_event(
                PairingEvent::RecvRequest {
                    session_id: session_id.clone(),
                    sender_peer_id: peer_id.clone(),
                    request,
                },
                Utc::now(),
            );

            let context = PairingSessionContext {
                state_machine,
                created_at: Utc::now(),
                timers: tokio::sync::Mutex::new(HashMap::new()),
            };

            self.session_manager
                .insert_session(session_id.clone(), context)
                .await;

            for action in actions {
                self.execute_action(&session_id, &peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Challenge (Initiator).
    pub async fn handle_challenge(
        &self,
        session_id: &str,
        peer_id: &str,
        challenge: PairingChallenge,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_challenge",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            self.session_manager
                .record_session_peer(
                    session_id,
                    peer_id.to_string(),
                    Some(challenge.device_name.clone()),
                )
                .await;
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvChallenge {
                        session_id: session_id.to_string(),
                        challenge,
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received KeyslotOffer (Initiator).
    pub async fn handle_keyslot_offer(
        &self,
        session_id: &str,
        peer_id: &str,
        offer: PairingKeyslotOffer,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_keyslot_offer",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let has_keyslot = offer.keyslot_file.as_ref().is_some();
            let has_challenge = offer.challenge.as_ref().is_some();
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                has_keyslot,
                has_challenge,
                "Handling pairing keyslot offer"
            );
            let keyslot_file = match offer.keyslot_file {
                Some(keyslot_file) => keyslot_file,
                None => {
                    tracing::warn!(
                        session_id = %session_id,
                        peer_id = %peer_id,
                        "Keyslot offer missing keyslot file"
                    );
                    return Ok(());
                }
            };
            let challenge = match offer.challenge {
                Some(challenge) => challenge,
                None => {
                    tracing::warn!(
                        session_id = %session_id,
                        peer_id = %peer_id,
                        "Keyslot offer missing challenge"
                    );
                    return Ok(());
                }
            };
            self.protocol_handler
                .emit_event(PairingDomainEvent::KeyslotReceived {
                    session_id: session_id.to_string(),
                    peer_id: peer_id.to_string(),
                    keyslot_file,
                    challenge,
                })
                .await;
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received ChallengeResponse (Responder).
    pub async fn handle_challenge_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: PairingChallengeResponse,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_challenge_response",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let has_encrypted_challenge = response.encrypted_challenge.as_ref().is_some();
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                has_encrypted_challenge,
                "Handling pairing challenge response"
            );
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Response (Responder).
    pub async fn handle_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: uc_core::network::protocol::PairingResponse,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_response",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                accepted = %response.accepted,
                "Handling pairing response from initiator"
            );
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvResponse {
                        session_id: session_id.to_string(),
                        response,
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// User accepts pairing (verification short code match).
    pub async fn user_accept_pairing(&self, session_id: &str) -> Result<()> {
        let span = info_span!("pairing.user_accept", session_id = %session_id);
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::UserAccept {
                        session_id: session_id.to_string(),
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, "", action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// User rejects pairing.
    pub async fn user_reject_pairing(&self, session_id: &str) -> Result<()> {
        let span = info_span!("pairing.user_reject", session_id = %session_id);
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::UserReject {
                        session_id: session_id.to_string(),
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, "", action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// User cancels pairing.
    pub async fn user_cancel_pairing(&self, session_id: &str) -> Result<()> {
        let span = info_span!("pairing.user_cancel", session_id = %session_id);
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::UserCancel {
                        session_id: session_id.to_string(),
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, "", action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Confirm.
    pub async fn handle_confirm(
        &self,
        session_id: &str,
        peer_id: &str,
        confirm: PairingConfirm,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_confirm",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                success = %confirm.success,
                error = ?confirm.error,
                "Handling pairing confirm message"
            );
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvConfirm {
                        session_id: session_id.to_string(),
                        confirm,
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Reject.
    pub async fn handle_reject(&self, session_id: &str, peer_id: &str) -> Result<()> {
        let span = info_span!(
            "pairing.handle_reject",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvReject {
                        session_id: session_id.to_string(),
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Cancel.
    pub async fn handle_cancel(&self, session_id: &str, peer_id: &str) -> Result<()> {
        let span = info_span!(
            "pairing.handle_cancel",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvCancel {
                        session_id: session_id.to_string(),
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle received Busy.
    pub async fn handle_busy(
        &self,
        session_id: &str,
        peer_id: &str,
        reason: Option<String>,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_busy",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = self
                .session_manager
                .process_event(
                    session_id,
                    PairingEvent::RecvBusy {
                        session_id: session_id.to_string(),
                        reason,
                    },
                )
                .await?;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Handle transport error.
    pub async fn handle_transport_error(
        &self,
        session_id: &str,
        peer_id: &str,
        error: String,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_transport_error",
            session_id = %session_id,
            peer_id = %peer_id,
            error = %error
        );
        async {
            let actions = self
                .session_manager
                .process_event_if_exists(
                    session_id,
                    PairingEvent::TransportError {
                        session_id: session_id.to_string(),
                        error: error.clone(),
                    },
                )
                .await;

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// Get peer info for a session.
    pub async fn get_session_peer(
        &self,
        session_id: &str,
    ) -> Option<super::session_manager::PairingPeerInfo> {
        self.session_manager.get_session_peer(session_id).await
    }

    /// Get role for a session.
    pub async fn get_session_role(&self, session_id: &str) -> Option<PairingRole> {
        self.session_manager.get_session_role(session_id).await
    }

    /// Return whether a session currently exists in the orchestrator.
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_manager.has_session(session_id).await
    }

    /// Return whether a session exists **and** is still in a non-terminal state.
    pub async fn has_active_session(&self, session_id: &str) -> bool {
        self.session_manager.has_active_session(session_id).await
    }

    /// Cleanup expired sessions.
    pub async fn cleanup_expired_sessions(&self) {
        self.session_manager.cleanup_expired_sessions().await
    }

    /// Execute a single action (delegates to protocol handler).
    async fn execute_action(
        &self,
        session_id: &str,
        peer_id: &str,
        action: PairingAction,
    ) -> Result<()> {
        self.protocol_handler
            .execute_action(
                session_id,
                peer_id,
                action,
                self.session_manager.sessions(),
                self.session_manager.session_peers(),
            )
            .await
    }
}

#[async_trait::async_trait]
impl PairingFacade for PairingOrchestrator {
    async fn initiate_pairing(&self, peer_id: String) -> anyhow::Result<SessionId> {
        Self::initiate_pairing(self, peer_id).await
    }

    async fn user_accept_pairing(&self, session_id: &str) -> anyhow::Result<()> {
        Self::user_accept_pairing(self, session_id).await
    }

    async fn user_reject_pairing(&self, session_id: &str) -> anyhow::Result<()> {
        Self::user_reject_pairing(self, session_id).await
    }

    async fn handle_challenge_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: PairingChallengeResponse,
    ) -> anyhow::Result<()> {
        Self::handle_challenge_response(self, session_id, peer_id, response).await
    }
}

#[async_trait::async_trait]
impl PairingEventPort for PairingOrchestrator {
    async fn subscribe(&self) -> anyhow::Result<mpsc::Receiver<PairingDomainEvent>> {
        let (event_tx, event_rx) = mpsc::channel(100);
        let mut senders = self.protocol_handler.event_senders().lock().await;
        senders.push(event_tx);
        Ok(event_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::NoopPairedDeviceRepository;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;
    use uc_core::crypto::pin_hash::hash_pin;
    use uc_core::network::paired_device::{PairedDevice, PairingState};
    use uc_core::network::pairing_state_machine::{FailureReason, TimeoutKind};
    use uc_core::network::protocol::{PairingRequest, PairingResponse};
    use uc_core::network::PairingMessage;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KdfAlgorithm, KdfParams,
        KdfParamsV1, KeyScope, KeySlotFile, KeySlotVersion,
    };

    #[derive(Default)]
    struct CountingDeviceRepository {
        upsert_calls: AtomicUsize,
    }

    struct FailingDeviceRepository;

    impl CountingDeviceRepository {
        fn upsert_calls(&self) -> usize {
            self.upsert_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for CountingDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            self.upsert_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for FailingDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Err(
                uc_core::ports::errors::PairedDeviceRepositoryError::Storage(
                    "injected upsert failure".to_string(),
                ),
            )
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_sync_settings(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _settings: Option<uc_core::settings::model::SyncSettings>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn pairing_persists_device_before_marking_persist_ok() {
        let staged_store = Arc::new(StagedPairedDeviceStore::new());
        let device_repo = Arc::new(CountingDeviceRepository::default());
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo.clone(),
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
            staged_store.clone(),
        );

        orchestrator
            .execute_action(
                "session-staged",
                "peer-remote",
                PairingAction::PersistPairedDevice {
                    session_id: "session-staged".to_string(),
                    device: PairedDevice {
                        peer_id: uc_core::ids::PeerId::from("peer-remote"),
                        pairing_state: PairingState::Pending,
                        identity_fingerprint: "fp-remote".to_string(),
                        paired_at: Utc::now(),
                        last_seen_at: None,
                        device_name: "Remote Device".to_string(),
                        sync_settings: None,
                    },
                },
            )
            .await
            .expect("stage paired device");

        assert_eq!(device_repo.upsert_calls(), 1);

        let staged = staged_store.take_by_peer_id("peer-remote");
        assert!(staged.is_some());
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (_orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );
    }

    #[tokio::test]
    async fn test_initiate_pairing() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let session_id = orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .unwrap();
        assert!(!session_id.is_empty());
    }

    #[tokio::test]
    async fn initiate_pairing_emits_request_action() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let _session_id = orchestrator
            .initiate_pairing("peer-remote".to_string())
            .await
            .expect("initiate pairing");

        let action = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        assert!(matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Request(_),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_cleanup_uses_configured_session_timeout() {
        let config = PairingConfig {
            step_timeout_secs: 1,
            user_verification_timeout_secs: 1,
            session_timeout_secs: 1,
            max_retries: 1,
            protocol_version: "1.0.0".to_string(),
        };
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        orchestrator.cleanup_expired_sessions().await;

        let sessions = orchestrator.session_manager.sessions().read().await;
        assert!(sessions.is_empty());

        let peers = orchestrator.session_manager.session_peers().read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_aborts_expired_session_timers() {
        let config = PairingConfig {
            step_timeout_secs: 1,
            user_verification_timeout_secs: 1,
            session_timeout_secs: 1,
            max_retries: 1,
            protocol_version: "1.0.0".to_string(),
        };
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let session_id = orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .expect("initiate pairing");

        let join_handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        let abort_handle = join_handle.abort_handle();

        {
            let mut sessions = orchestrator.session_manager.sessions().write().await;
            let context = sessions.get_mut(&session_id).expect("session context");
            let mut timers = context.timers.lock().await;
            timers.insert(TimeoutKind::Persist, abort_handle);
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        orchestrator.cleanup_expired_sessions().await;

        let join_result = join_handle.await;
        assert!(join_result.is_err());
    }

    #[test]
    fn test_pairing_config_from_settings() {
        let mut settings = Settings::default();
        settings.pairing.step_timeout = std::time::Duration::from_secs(20);
        settings.pairing.user_verification_timeout = std::time::Duration::from_secs(90);
        settings.pairing.session_timeout = std::time::Duration::from_secs(400);
        settings.pairing.max_retries = 5;
        settings.pairing.protocol_version = "2.0.0".to_string();

        let config = PairingConfig::from_settings(&settings);

        assert_eq!(config.step_timeout_secs, 20);
        assert_eq!(config.user_verification_timeout_secs, 90);
        assert_eq!(config.session_timeout_secs, 400);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.protocol_version, "2.0.0");
    }

    #[tokio::test]
    async fn test_handle_response_emits_confirm_action() {
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

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };

        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle request");
        orchestrator
            .user_accept_pairing("session-1")
            .await
            .expect("accept pairing");

        let challenge = loop {
            let challenge_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("challenge action timeout")
                .expect("challenge action missing");
            if let PairingAction::Send {
                message: PairingMessage::Challenge(challenge),
                ..
            } = challenge_action
            {
                break challenge;
            }
        };

        let pin_hash = hash_pin(&challenge.pin).expect("hash pin");
        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash,
            accepted: true,
        };

        orchestrator
            .handle_response(&challenge.session_id, "peer-remote", response)
            .await
            .expect("handle response");

        let confirm = loop {
            let confirm_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("confirm action timeout")
                .expect("confirm action missing");
            if let PairingAction::Send {
                message: PairingMessage::Confirm(confirm),
                ..
            } = confirm_action
            {
                break confirm;
            }
        };

        assert!(confirm.success);
        assert_eq!(confirm.sender_device_name, "LocalDevice");
        assert_eq!(confirm.device_id, "device-123");
    }

    #[tokio::test]
    async fn pairing_emits_failed_event_when_persist_upsert_fails() {
        let staged_store = Arc::new(StagedPairedDeviceStore::new());
        let config = PairingConfig::default();
        let device_repo = Arc::new(FailingDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
            staged_store,
        );

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        let request = PairingRequest {
            session_id: "session-persist-fail".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };

        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle request");
        orchestrator
            .user_accept_pairing("session-persist-fail")
            .await
            .expect("accept pairing");

        let challenge = loop {
            let challenge_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("challenge action timeout")
                .expect("challenge action missing");
            if let PairingAction::Send {
                message: PairingMessage::Challenge(challenge),
                ..
            } = challenge_action
            {
                break challenge;
            }
        };

        let pin_hash = hash_pin(&challenge.pin).expect("hash pin");
        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash,
            accepted: true,
        };

        orchestrator
            .handle_response(&challenge.session_id, "peer-remote", response)
            .await
            .expect("handle response");

        let mut reason: Option<FailureReason> = None;
        for _ in 0..4 {
            let event = timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .expect("event timeout")
                .expect("event missing");
            if let crate::usecases::pairing::PairingDomainEvent::PairingFailed {
                reason: failed_reason,
                ..
            } = event
            {
                reason = Some(failed_reason);
                break;
            }
        }

        let reason = reason.expect("expected PairingFailed event");
        match reason {
            FailureReason::Other(message) => {
                assert!(message.contains("injected upsert failure"));
            }
            other => panic!("expected FailureReason::Other, got {:?}", other),
        }

        for _ in 0..2 {
            let maybe_event = timeout(Duration::from_millis(200), event_rx.recv()).await;
            match maybe_event {
                Ok(Some(crate::usecases::pairing::PairingDomainEvent::PairingSucceeded {
                    ..
                })) => {
                    panic!("unexpected PairingSucceeded event after persist failure");
                }
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => break,
            }
        }
    }

    #[tokio::test]
    async fn test_show_verification_is_forwarded_to_action_channel() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::ShowVerification {
                    session_id: "session-1".to_string(),
                    short_code: "ABC123".to_string(),
                    local_fingerprint: "LOCAL".to_string(),
                    peer_fingerprint: "PEER".to_string(),
                    peer_display_name: "Peer".to_string(),
                },
            )
            .await
            .expect("execute action");

        let action = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        assert!(matches!(action, PairingAction::ShowVerification { .. }));
    }

    #[tokio::test]
    async fn test_start_timer_records_handle() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-1".to_string(),
            vec![1; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let session_id = orchestrator
            .initiate_pairing("peer-2".to_string())
            .await
            .unwrap();
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::StartTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                    deadline: Utc::now() + chrono::Duration::seconds(1),
                },
            )
            .await
            .unwrap();

        let sessions = orchestrator.session_manager.sessions().read().await;
        let context = sessions.get(&session_id).expect("session");
        let timers = context.timers.lock().await;
        assert!(timers.contains_key(&TimeoutKind::WaitingChallenge));
    }

    #[tokio::test]
    async fn test_cancel_timer_removes_handle() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-1".to_string(),
            vec![1; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let session_id = orchestrator
            .initiate_pairing("peer-2".to_string())
            .await
            .unwrap();
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::StartTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                    deadline: Utc::now() + chrono::Duration::seconds(1),
                },
            )
            .await
            .unwrap();

        {
            let sessions = orchestrator.session_manager.sessions().read().await;
            let context = sessions.get(&session_id).expect("session");
            let timers = context.timers.lock().await;
            assert!(timers.contains_key(&TimeoutKind::WaitingChallenge));
        }
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::CancelTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                },
            )
            .await
            .unwrap();

        let sessions = orchestrator.session_manager.sessions().read().await;
        let context = sessions.get(&session_id).expect("session");
        let timers = context.timers.lock().await;
        assert!(!timers.contains_key(&TimeoutKind::WaitingChallenge));
    }

    #[tokio::test]
    async fn test_handle_incoming_request_validates_target_peer_id() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "Peer".to_string(),
            device_id: "device-2".to_string(),
            peer_id: "wrong-peer-id".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![3; 16],
        };

        let result = orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Request target peer_id mismatch: expected peer-local, got wrong-peer-id"
        );
    }

    fn sample_keyslot_file() -> KeySlotFile {
        KeySlotFile {
            version: KeySlotVersion::V1,
            scope: KeyScope {
                profile_id: "profile-1".to_string(),
            },
            kdf: KdfParams {
                alg: KdfAlgorithm::Argon2id,
                params: KdfParamsV1 {
                    mem_kib: 1024,
                    iters: 2,
                    parallelism: 1,
                },
            },
            salt: vec![1, 2, 3],
            wrapped_master_key: EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![9, 8, 7],
                ciphertext: vec![6, 5, 4],
                aad_fingerprint: None,
            },
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_keyslot_received_event() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");
        let offer = PairingKeyslotOffer {
            session_id: "session-1".to_string(),
            keyslot_file: Some(sample_keyslot_file()),
            challenge: Some(vec![9, 9, 9]),
        };

        orchestrator
            .handle_keyslot_offer("session-1", "peer-remote", offer)
            .await
            .expect("handle keyslot offer");

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");

        assert!(matches!(
            event,
            crate::usecases::pairing::PairingDomainEvent::KeyslotReceived { .. }
        ));
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_pairing_result_events() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(NoopPairedDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
            Arc::new(StagedPairedDeviceStore::new()),
        );

        orchestrator
            .session_manager
            .record_session_peer(
                "session-1",
                "peer-remote".to_string(),
                Some("Remote".to_string()),
            )
            .await;

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::EmitResult {
                    session_id: "session-1".to_string(),
                    success: true,
                    error: None,
                },
            )
            .await
            .expect("emit success result");

        let success_event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");
        assert!(matches!(
            success_event,
            crate::usecases::pairing::PairingDomainEvent::PairingSucceeded { .. }
        ));

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::EmitResult {
                    session_id: "session-1".to_string(),
                    success: false,
                    error: Some("failed".to_string()),
                },
            )
            .await
            .expect("emit failure result");

        let failed_event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");
        match failed_event {
            crate::usecases::pairing::PairingDomainEvent::PairingFailed { reason, .. } => {
                assert!(matches!(reason, FailureReason::Other(_)));
            }
            _ => panic!("expected PairingFailed event"),
        }
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_verification_required_event() {
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

        orchestrator
            .session_manager
            .record_session_peer(
                "session-verify",
                "peer-remote".to_string(),
                Some("Remote".to_string()),
            )
            .await;

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        orchestrator
            .execute_action(
                "session-verify",
                "peer-remote",
                PairingAction::ShowVerification {
                    session_id: "session-verify".to_string(),
                    short_code: "111-222".to_string(),
                    local_fingerprint: "LOCAL".to_string(),
                    peer_fingerprint: "PEER".to_string(),
                    peer_display_name: "Remote".to_string(),
                },
            )
            .await
            .expect("execute action");

        // consume action forwarded to UI channel
        timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");

        match event {
            crate::usecases::pairing::PairingDomainEvent::PairingVerificationRequired {
                session_id,
                peer_id,
                short_code,
                local_fingerprint,
                peer_fingerprint,
            } => {
                assert_eq!(session_id, "session-verify");
                assert_eq!(peer_id, "peer-remote");
                assert_eq!(short_code, "111-222");
                assert_eq!(local_fingerprint, "LOCAL");
                assert_eq!(peer_fingerprint, "PEER");
            }
            _ => panic!("expected PairingVerificationRequired event"),
        }
    }

    #[tokio::test]
    async fn pairing_orchestrator_preserves_busy_reason_in_failure_event() {
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
        let mut event_rx = PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe to pairing events");

        let session_id = orchestrator
            .initiate_pairing("peer-busy".to_string())
            .await
            .expect("initiate pairing");

        let _ = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("send action timeout")
            .expect("send action missing");

        orchestrator
            .handle_busy(
                &session_id,
                "peer-busy",
                Some("no_local_pairing_participant_ready".to_string()),
            )
            .await
            .expect("handle busy with reason");

        let failed_event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");
        match failed_event {
            crate::usecases::pairing::PairingDomainEvent::PairingFailed { reason, .. } => {
                assert_eq!(
                    reason,
                    FailureReason::Other("no_local_pairing_participant_ready".to_string())
                );
            }
            _ => panic!("expected PairingFailed event"),
        }
    }
}
