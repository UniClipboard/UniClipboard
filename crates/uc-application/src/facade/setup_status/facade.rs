//! `SetupStatusFacade` implementation.
//!
//! Slice4 P3 T3.4 collapsed the legacy `crate::setup::usecases::*Usecase`
//! wrappers into the facade — both were thin pass-throughs to
//! [`SetupStatusPort`] and the orchestrator they belonged to is gone.

use std::sync::Arc;

use uc_core::ports::SetupStatusPort;

/// Setup-status facade: read + write the persisted completion flag.
pub struct SetupStatusFacade {
    setup_status: Arc<dyn SetupStatusPort>,
}

impl SetupStatusFacade {
    pub fn new(setup_status: Arc<dyn SetupStatusPort>) -> Self {
        Self { setup_status }
    }

    /// Returns `Ok(true)` when `SetupStatus.has_completed` is `true`.
    /// Propagates the port's `anyhow::Error` on read failure.
    pub async fn is_complete(&self) -> anyhow::Result<bool> {
        Ok(self.setup_status.get_status().await?.has_completed)
    }

    /// Persists `SetupStatus.has_completed = true`, preserving any other
    /// fields (e.g. `space_id`) already on the record.
    pub async fn mark_complete(&self) -> anyhow::Result<()> {
        let mut status = self.setup_status.get_status().await?;
        status.has_completed = true;
        self.setup_status.set_status(&status).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use async_trait::async_trait;
    use tokio::sync::RwLock;
    use uc_core::setup::SetupStatus;

    /// In-memory `SetupStatusPort` for facade tests; replaces the
    /// previous `crate::setup::testing::FakeSetupStatus` helper that
    /// shipped with the deleted setup module.
    struct InMemorySetupStatus {
        status: RwLock<SetupStatus>,
    }

    impl InMemorySetupStatus {
        fn new(initial: SetupStatus) -> Arc<Self> {
            Arc::new(Self {
                status: RwLock::new(initial),
            })
        }

        async fn snapshot(&self) -> SetupStatus {
            self.status.read().await.clone()
        }
    }

    #[async_trait]
    impl SetupStatusPort for InMemorySetupStatus {
        async fn get_status(&self) -> anyhow::Result<SetupStatus> {
            Ok(self.status.read().await.clone())
        }

        async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()> {
            *self.status.write().await = status.clone();
            Ok(())
        }
    }

    fn completed_status() -> SetupStatus {
        SetupStatus {
            has_completed: true,
            ..SetupStatus::default()
        }
    }

    #[tokio::test]
    async fn is_complete_reports_true_when_status_completed() {
        let port = InMemorySetupStatus::new(completed_status());
        let facade = SetupStatusFacade::new(port);
        assert!(facade.is_complete().await.unwrap());
    }

    #[tokio::test]
    async fn is_complete_reports_false_when_status_default() {
        let port = InMemorySetupStatus::new(SetupStatus::default());
        let facade = SetupStatusFacade::new(port);
        assert!(!facade.is_complete().await.unwrap());
    }

    #[tokio::test]
    async fn mark_complete_flips_the_persisted_flag() {
        let port = InMemorySetupStatus::new(SetupStatus::default());
        let facade = SetupStatusFacade::new(port.clone());

        facade.mark_complete().await.unwrap();

        assert!(port.snapshot().await.has_completed);
        assert!(facade.is_complete().await.unwrap());
    }
}
