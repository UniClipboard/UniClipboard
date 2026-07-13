//! Decorator that fans out successful register advances to subscribers.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tracing::debug;
use uc_core::clipboard::ActiveClipboardState;
use uc_core::ports::clipboard::{ActiveClipboardRegisterError, AdvanceActiveClipboardPort};

/// Wraps an [`AdvanceActiveClipboardPort`] implementation and broadcasts the
/// new state whenever a call actually advances the register.
pub struct BroadcastingAdvance {
    inner: Arc<dyn AdvanceActiveClipboardPort>,
    tx: broadcast::Sender<ActiveClipboardState>,
}

impl BroadcastingAdvance {
    pub fn new(
        inner: Arc<dyn AdvanceActiveClipboardPort>,
        tx: broadcast::Sender<ActiveClipboardState>,
    ) -> Self {
        Self { inner, tx }
    }
}

#[async_trait]
impl AdvanceActiveClipboardPort for BroadcastingAdvance {
    async fn advance(
        &self,
        state: &ActiveClipboardState,
        mobile_consumable: bool,
    ) -> Result<bool, ActiveClipboardRegisterError> {
        let advanced = self.inner.advance(state, mobile_consumable).await?;
        if advanced {
            // Fire-and-forget: no subscribers is a normal, expected state
            // (no SSE clients connected), not a failure of `advance` itself.
            let _ = self.tx.send(state.clone());
            debug!(
                snapshot_hash = %state.snapshot_hash,
                "published active-clipboard register advance to SSE subscribers"
            );
        }
        Ok(advanced)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ids::{DeviceId, EntryId};

    struct StubAdvance {
        result: Result<bool, ActiveClipboardRegisterError>,
    }

    #[async_trait]
    impl AdvanceActiveClipboardPort for StubAdvance {
        async fn advance(
            &self,
            _state: &ActiveClipboardState,
            _mobile_consumable: bool,
        ) -> Result<bool, ActiveClipboardRegisterError> {
            match &self.result {
                Ok(v) => Ok(*v),
                Err(e) => Err(ActiveClipboardRegisterError::Storage(e.to_string())),
            }
        }
    }

    fn state() -> ActiveClipboardState {
        ActiveClipboardState::new("blake3v1:aa", EntryId::new(), 100, DeviceId::new("dev-a"))
    }

    #[tokio::test]
    async fn advance_true_broadcasts_the_state() {
        let (tx, mut rx) = broadcast::channel(64);
        let decorator = BroadcastingAdvance::new(Arc::new(StubAdvance { result: Ok(true) }), tx);

        let published_state = state();
        let advanced = decorator.advance(&published_state, true).await.unwrap();
        assert!(advanced);
        let published = rx.try_recv().expect("state must be published");
        assert_eq!(published, published_state);
    }

    #[tokio::test]
    async fn advance_false_does_not_broadcast() {
        let (tx, mut rx) = broadcast::channel(64);
        let decorator = BroadcastingAdvance::new(Arc::new(StubAdvance { result: Ok(false) }), tx);

        let advanced = decorator.advance(&state(), true).await.unwrap();
        assert!(!advanced);
        assert!(rx.try_recv().is_err(), "no publish on a no-op advance");
    }

    #[tokio::test]
    async fn advance_err_does_not_broadcast_and_propagates() {
        let (tx, mut rx) = broadcast::channel(64);
        let decorator = BroadcastingAdvance::new(
            Arc::new(StubAdvance {
                result: Err(ActiveClipboardRegisterError::Storage("boom".into())),
            }),
            tx,
        );

        let result = decorator.advance(&state(), true).await;
        assert!(result.is_err());
        assert!(rx.try_recv().is_err(), "no publish on an error");
    }

    #[tokio::test]
    async fn no_subscribers_does_not_affect_advance_result() {
        let (tx, rx) = broadcast::channel(64);
        drop(rx);
        let decorator = BroadcastingAdvance::new(Arc::new(StubAdvance { result: Ok(true) }), tx);

        let advanced = decorator.advance(&state(), true).await.unwrap();
        assert!(
            advanced,
            "send failure with no subscribers must not fail advance"
        );
    }
}
