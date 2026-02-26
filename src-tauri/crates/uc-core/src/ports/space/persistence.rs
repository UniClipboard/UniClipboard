use crate::ids::SpaceId;

#[async_trait::async_trait]
pub trait PersistencePort: Send {
    async fn persist_joiner_access(
        &mut self,
        space_id: &SpaceId,
        peer_id: &str,
    ) -> anyhow::Result<()>;
    async fn persist_sponsor_access(
        &mut self,
        space_id: &SpaceId,
        peer_id: &str,
    ) -> anyhow::Result<()>;
}
