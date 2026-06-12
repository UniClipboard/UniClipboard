use std::sync::Arc;

use uc_core::TrustedPeerRepositoryPort;

use crate::trusted_peer::errors::TrustedPeerApplicationError;
use crate::trusted_peer::orchestrator::TrustPeerOrchestrator;
use crate::trusted_peer::state::TrustState;

/// UI-facing entry for "user confirmed the peer is genuine" (DOMAIN §5.1).
///
/// Thin wrapper around `TrustPeerOrchestrator::confirm_verification` so
/// callers (setup facade, daemon command handler) need not know the
/// orchestrator exists. The orchestrator is the single source of truth for
/// the state machine and the persistence call.
pub struct ConfirmPeerVerificationUseCase<R: ?Sized> {
    orchestrator: Arc<TrustPeerOrchestrator<R>>,
}

impl<R> ConfirmPeerVerificationUseCase<R>
where
    R: TrustedPeerRepositoryPort + ?Sized,
{
    pub fn new(orchestrator: Arc<TrustPeerOrchestrator<R>>) -> Self {
        Self { orchestrator }
    }

    pub async fn execute(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        self.orchestrator.confirm_verification().await
    }
}
