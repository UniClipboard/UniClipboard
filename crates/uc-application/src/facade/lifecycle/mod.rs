use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tracing::instrument;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleStateView {
    Idle,
    Pending,
    Ready,
    NetworkFailed,
}

impl LifecycleStateView {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Pending => "Pending",
            Self::Ready => "Ready",
            Self::NetworkFailed => "NetworkFailed",
        }
    }
}

#[async_trait]
pub trait LifecycleStatusGateway: Send + Sync {
    async fn set_state(&self, state: LifecycleStateView) -> Result<()>;
    async fn get_state(&self) -> LifecycleStateView;
}

pub struct InMemoryLifecycleStatus {
    state: tokio::sync::Mutex<LifecycleStateView>,
}

impl InMemoryLifecycleStatus {
    pub fn new() -> Self {
        Self {
            state: tokio::sync::Mutex::new(LifecycleStateView::Idle),
        }
    }
}

impl Default for InMemoryLifecycleStatus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl LifecycleStatusGateway for InMemoryLifecycleStatus {
    async fn set_state(&self, state: LifecycleStateView) -> Result<()> {
        *self.state.lock().await = state;
        Ok(())
    }

    async fn get_state(&self) -> LifecycleStateView {
        self.state.lock().await.clone()
    }
}

#[derive(Clone)]
pub struct LifecycleFacadeDeps {
    pub status: Arc<dyn LifecycleStatusGateway>,
}

#[derive(Debug, thiserror::Error)]
pub enum LifecycleFacadeError {
    #[error("failed to update lifecycle state: {0}")]
    UpdateState(String),
}

pub struct LifecycleFacade {
    deps: LifecycleFacadeDeps,
}

impl LifecycleFacade {
    pub fn new(deps: LifecycleFacadeDeps) -> Self {
        Self { deps }
    }

    #[instrument(skip_all)]
    pub async fn status(&self) -> LifecycleStateView {
        self.deps.status.get_state().await
    }

    #[instrument(skip_all)]
    pub async fn retry_to_ready(&self) -> Result<LifecycleStateView, LifecycleFacadeError> {
        if self.deps.status.get_state().await == LifecycleStateView::Ready {
            tracing::info!("lifecycle facade: already ready; skip retry");
            return Ok(LifecycleStateView::Ready);
        }

        self.deps
            .status
            .set_state(LifecycleStateView::Pending)
            .await
            .map_err(|err| {
                tracing::warn!(error = %err, "lifecycle facade: failed to set pending");
                LifecycleFacadeError::UpdateState(err.to_string())
            })?;
        self.deps
            .status
            .set_state(LifecycleStateView::Ready)
            .await
            .map_err(|err| {
                tracing::warn!(error = %err, "lifecycle facade: failed to set ready");
                LifecycleFacadeError::UpdateState(err.to_string())
            })?;

        tracing::info!("lifecycle facade: retry completed");
        Ok(LifecycleStateView::Ready)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    struct InMemoryLifecycleStatus {
        state: Mutex<LifecycleStateView>,
        fail_on_set: bool,
    }

    #[async_trait]
    impl LifecycleStatusGateway for InMemoryLifecycleStatus {
        async fn set_state(&self, state: LifecycleStateView) -> Result<()> {
            if self.fail_on_set {
                anyhow::bail!("status unavailable");
            }
            *self.state.lock().expect("state lock") = state;
            Ok(())
        }

        async fn get_state(&self) -> LifecycleStateView {
            self.state.lock().expect("state lock").clone()
        }
    }

    fn facade_with(state: LifecycleStateView, fail_on_set: bool) -> LifecycleFacade {
        LifecycleFacade::new(LifecycleFacadeDeps {
            status: Arc::new(InMemoryLifecycleStatus {
                state: Mutex::new(state),
                fail_on_set,
            }),
        })
    }

    #[tokio::test]
    async fn status_returns_current_state() {
        let facade = facade_with(LifecycleStateView::Pending, false);

        assert_eq!(facade.status().await, LifecycleStateView::Pending);
    }

    #[tokio::test]
    async fn retry_moves_non_ready_state_to_ready() {
        let facade = facade_with(LifecycleStateView::NetworkFailed, false);

        let result = facade.retry_to_ready().await.expect("retry");

        assert_eq!(result, LifecycleStateView::Ready);
        assert_eq!(facade.status().await, LifecycleStateView::Ready);
    }

    #[tokio::test]
    async fn retry_keeps_ready_state_ready() {
        let facade = facade_with(LifecycleStateView::Ready, true);

        let result = facade.retry_to_ready().await.expect("already ready");

        assert_eq!(result, LifecycleStateView::Ready);
    }

    #[tokio::test]
    async fn retry_returns_error_when_status_update_fails() {
        let facade = facade_with(LifecycleStateView::Idle, true);

        let error = facade.retry_to_ready().await.expect_err("update fails");

        assert!(matches!(error, LifecycleFacadeError::UpdateState(_)));
    }
}
