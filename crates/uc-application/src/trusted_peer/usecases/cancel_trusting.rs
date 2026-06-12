use std::sync::Arc;

use uc_core::TrustedPeerRepositoryPort;

use crate::trusted_peer::errors::TrustedPeerApplicationError;
use crate::trusted_peer::orchestrator::TrustPeerOrchestrator;
use crate::trusted_peer::state::TrustState;

/// UI-facing entry for "user cancelled the trust flow" (DOMAIN §5.1).
///
/// Thin wrapper around `TrustPeerOrchestrator::cancel`. The flow must
/// still be in a non-terminal state (`Idle` is rejected — there is
/// nothing to cancel); callers receive `IllegalTransition` otherwise.
pub struct CancelTrustingUseCase<R: ?Sized> {
    orchestrator: Arc<TrustPeerOrchestrator<R>>,
}

impl<R> CancelTrustingUseCase<R>
where
    R: TrustedPeerRepositoryPort + ?Sized,
{
    pub fn new(orchestrator: Arc<TrustPeerOrchestrator<R>>) -> Self {
        Self { orchestrator }
    }

    pub async fn execute(&self) -> Result<TrustState, TrustedPeerApplicationError> {
        self.orchestrator.cancel().await
    }
}
