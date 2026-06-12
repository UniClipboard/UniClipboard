use crate::ids::SessionId;

#[async_trait::async_trait]
pub trait TimerPort: Send {
    async fn start(&mut self, session_id: &SessionId, ttl_secs: u64) -> anyhow::Result<()>;
    async fn stop(&mut self, session_id: &SessionId) -> anyhow::Result<()>;
}
