use anyhow::Result;

#[async_trait::async_trait]
pub trait UiPort: Send + Sync {
    async fn open_settings(&self) -> Result<()>;
}
