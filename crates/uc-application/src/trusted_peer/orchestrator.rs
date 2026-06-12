use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use uc_core::{DeviceId, TrustedPeerRepositoryPort};

use super::challenge::TrustVerificationChallenge;
use super::errors::TrustedPeerApplicationError;
use super::state::{TrustState, TrustStateEvent};
use super::state_machine::transition;
use super::usecases::trust_peer::{TrustPeer, TrustPeerUseCase};

/// Drives the trust-establishment flow's in-memory state machine and
/// bridges it to the `TrustedPeerRepositoryPort` when the flow reaches
/// `Trusted` (DOMAIN §5.4).
///
/// Wiring (pairing-protocol events, network callbacks) happens in phase 0.4 —
/// at that point the orchestrator is spawned once per local device and
/// exposes `record_session_opened` / `record_timeout` / `record_protocol_error`
/// for the protocol handler to drive, plus `initiate` / `confirm` / `cancel`
/// for user-initiated actions.
pub struct TrustPeerOrchestrator<R: ?Sized> {
    state: Mutex<TrustState>,
    trust_peer: TrustPeerUseCase<R>,
    local_device_id: DeviceId,
}

impl<R> TrustPeerOrchestrator<R>
where
    R: TrustedPeerRepositoryPort + ?Sized,
{
    pub fn new(repository: Arc<R>, local_device_id: DeviceId) -> Self {
        Self {
            state: Mutex::new(TrustState::Idle),
            trust_peer: TrustPeerUseCase::new(repository),
            local_device_id,
        }
    }

    pub async fn current_state(&self) -> TrustState {
        self.state.lock().await.clone()
    }

    /// Reset the orchestrator back to `Idle` regardless of current state.
    ///
    /// Intended to be called at the start of a fresh trust-establishment
    /// flow when the orchestrator is wired as a process-wide singleton
    /// (D19). Terminal states (`Trusted`, `Aborted`) reject all further
    /// events by design, so without an explicit reset the second flow
    /// would always fail with `IllegalTransition`.
    ///
    /// Intentionally infallible: the caller's contract is "I own the
    /// orchestrator for the next flow"; mid-flow state is discarded.
    pub async fn reset(&self) -> TrustState {
        let mut guard = self.state.lock().await;
        *guard = TrustState::Idle;
        TrustState::Idle
    }

    /// External entry point: start trusting a specific peer.
    pub async fn initiate(
        &self,
        peer_device_id: DeviceId,
    ) -> Result<TrustState, TrustedPeerApplicationError> {
        self.drive(TrustStateEvent::Initiate { peer_device_id })
            .await
    }

    /// Pairing protocol reports that the session opened and produced a
    /// short-code challenge.
    pub async fn record_session_opened(
        &self,
        peer_device_id: DeviceId,
        challenge: TrustVerificationChallenge,
    ) -> Result<TrustState, TrustedPeerApplicationError> {
        self.drive(TrustStateEvent::SessionOpened {
            peer_device_id,
            challenge,
        })
        .await
    }

    /// User confirmed the peer identity — persist the `TrustedPeer` and
    /// transition to the terminal `Trusted` state.
    pub async fn confirm_verification(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        let mut guard = self.state.lock().await;
        let (peer_device_id, peer_fingerprint) = match &*guard {
            TrustState::AwaitingUserVerification {
                peer_device_id,
                challenge,
            } => (peer_device_id.clone(), challenge.peer_fingerprint.clone()),
            other => {
                return Err(TrustedPeerApplicationError::IllegalTransition(format!(
                    "confirm_verification only valid in AwaitingUserVerification, was {other:?}"
                )));
            }
        };

        let trusted_peer = self
            .trust_peer
            .execute(TrustPeer {
                local_device_id: self.local_device_id.clone(),
                peer_device_id,
                peer_fingerprint,
                trusted_at: Utc::now(),
            })
            .await?;

        let next = transition(
            guard.clone(),
            TrustStateEvent::UserConfirmed { trusted_peer },
        )?;
        *guard = next.clone();
        Ok(next)
    }

    /// User cancelled the flow.
    pub async fn cancel(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        self.drive(TrustStateEvent::UserCancelled).await
    }

    /// Pairing protocol / timer reports a timeout.
    pub async fn record_timeout(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        self.drive(TrustStateEvent::TimedOut).await
    }

    /// Pairing protocol reports a non-recoverable error.
    pub async fn record_protocol_error(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        self.drive(TrustStateEvent::ProtocolError).await
    }

    async fn drive(
        &self,
        event: TrustStateEvent,
    ) -> Result<TrustState, TrustedPeerApplicationError> {
        let mut guard = self.state.lock().await;
        let next = transition(guard.clone(), event)?;
        *guard = next.clone();
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trusted_peer::testing::InMemoryTrustedPeerRepository;
    use uc_core::security::IdentityFingerprint;
    use uc_core::TrustAbortReason;

    fn build(
        local: &str,
    ) -> (
        Arc<InMemoryTrustedPeerRepository>,
        TrustPeerOrchestrator<InMemoryTrustedPeerRepository>,
    ) {
        let repo = Arc::new(InMemoryTrustedPeerRepository::new());
        let orch = TrustPeerOrchestrator::new(repo.clone(), DeviceId::new(local));
        (repo, orch)
    }

    /// Pad a short seed into a valid 16-char alphanumeric fingerprint.
    fn fp_of(seed: &str) -> IdentityFingerprint {
        let mut raw: String = seed.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
        raw.make_ascii_uppercase();
        while raw.len() < 16 {
            raw.push('A');
        }
        IdentityFingerprint::from_raw_string(&raw[..16]).unwrap()
    }

    fn challenge(fp_seed: &str, code: &str) -> TrustVerificationChallenge {
        TrustVerificationChallenge {
            peer_fingerprint: fp_of(fp_seed),
            short_code: code.into(),
        }
    }

    #[tokio::test]
    async fn happy_path_persists_trusted_peer_and_transitions_to_trusted() {
        let (repo, orch) = build("local-1");

        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-a"), challenge("FPA", "123"))
            .await
            .unwrap();

        let state = orch.confirm_verification().await.unwrap();
        match state {
            TrustState::Trusted { trusted_peer } => {
                assert_eq!(trusted_peer.peer_device_id.as_str(), "peer-a");
                assert_eq!(trusted_peer.local_device_id.as_str(), "local-1");
                assert_eq!(trusted_peer.peer_fingerprint, fp_of("FPA"));
            }
            other => panic!("expected Trusted, got {other:?}"),
        }

        let saved = repo.get(&DeviceId::new("peer-a")).await.unwrap();
        assert!(saved.is_some());
        assert_eq!(saved.unwrap().peer_fingerprint, fp_of("FPA"));
    }

    #[tokio::test]
    async fn cancel_from_awaiting_goes_to_aborted_user_cancelled_and_does_not_persist() {
        let (repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-a"), challenge("FPA", "123"))
            .await
            .unwrap();

        let state = orch.cancel().await.unwrap();
        assert_eq!(
            state,
            TrustState::Aborted {
                reason: TrustAbortReason::UserCancelled
            }
        );

        assert!(repo.get(&DeviceId::new("peer-a")).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn timeout_from_establishing_goes_to_aborted_timeout() {
        let (_repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();

        let state = orch.record_timeout().await.unwrap();
        assert_eq!(
            state,
            TrustState::Aborted {
                reason: TrustAbortReason::Timeout
            }
        );
    }

    #[tokio::test]
    async fn protocol_error_from_awaiting_goes_to_aborted_protocol_error() {
        let (_repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-a"), challenge("FPA", "123"))
            .await
            .unwrap();

        let state = orch.record_protocol_error().await.unwrap();
        assert_eq!(
            state,
            TrustState::Aborted {
                reason: TrustAbortReason::ProtocolError
            }
        );
    }

    #[tokio::test]
    async fn confirm_without_awaiting_verification_fails() {
        let (_repo, orch) = build("local-1");

        // Idle → confirm directly
        let err = orch.confirm_verification().await.unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));

        // After initiate (EstablishingSession) → confirm still invalid
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        let err = orch.confirm_verification().await.unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[tokio::test]
    async fn session_opened_with_mismatched_peer_rejected() {
        let (_repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();

        let err = orch
            .record_session_opened(DeviceId::new("different"), challenge("FP", "code"))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[tokio::test]
    async fn terminal_state_rejects_further_events() {
        let (_repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-a"), challenge("FPA", "123"))
            .await
            .unwrap();
        orch.confirm_verification().await.unwrap();

        // Trusted terminal — any further event errors
        let err = orch.initiate(DeviceId::new("peer-b")).await.unwrap_err();
        assert!(matches!(
            err,
            TrustedPeerApplicationError::IllegalTransition(_)
        ));
    }

    #[tokio::test]
    async fn reset_returns_to_idle_from_trusted_and_allows_new_flow() {
        let (repo, orch) = build("local-1");

        // First flow: reach Trusted terminal.
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-a"), challenge("FPA", "123"))
            .await
            .unwrap();
        orch.confirm_verification().await.unwrap();
        assert!(matches!(
            orch.current_state().await,
            TrustState::Trusted { .. }
        ));

        // Reset, then start a brand new flow with a different peer.
        let after_reset = orch.reset().await;
        assert_eq!(after_reset, TrustState::Idle);

        // Remove peer-a from repo so peer-b is just a regular new trust.
        // (Not required — orchestrator reset is orthogonal to persistence —
        // but keeps the second flow's assertion unambiguous.)
        repo.remove(&DeviceId::new("peer-a")).await.unwrap();

        orch.initiate(DeviceId::new("peer-b")).await.unwrap();
        orch.record_session_opened(DeviceId::new("peer-b"), challenge("FPB", "456"))
            .await
            .unwrap();
        let final_state = orch.confirm_verification().await.unwrap();
        match final_state {
            TrustState::Trusted { trusted_peer } => {
                assert_eq!(trusted_peer.peer_device_id.as_str(), "peer-b");
            }
            other => panic!("expected Trusted after reset+new flow, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn reset_from_aborted_returns_to_idle() {
        let (_repo, orch) = build("local-1");
        orch.initiate(DeviceId::new("peer-a")).await.unwrap();
        orch.cancel().await.unwrap();
        assert!(matches!(
            orch.current_state().await,
            TrustState::Aborted { .. }
        ));

        let after_reset = orch.reset().await;
        assert_eq!(after_reset, TrustState::Idle);
    }

    #[tokio::test]
    async fn reset_from_idle_is_noop() {
        let (_repo, orch) = build("local-1");
        let after_reset = orch.reset().await;
        assert_eq!(after_reset, TrustState::Idle);
    }
}
