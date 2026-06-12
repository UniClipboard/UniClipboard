use crate::setup::SetupStatus;
use async_trait::async_trait;

#[async_trait]
pub trait SetupStatusPort: Send + Sync {
    async fn get_status(&self) -> anyhow::Result<SetupStatus>;
    async fn set_status(&self, status: &SetupStatus) -> anyhow::Result<()>;
}
