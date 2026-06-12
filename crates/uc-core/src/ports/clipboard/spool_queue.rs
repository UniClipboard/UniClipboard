use crate::ids::RepresentationId;

#[derive(Debug, Clone)]
pub struct SpoolRequest {
    pub rep_id: RepresentationId,
    pub bytes: Vec<u8>,
}

#[async_trait::async_trait]
pub trait SpoolQueuePort: Send + Sync {
    async fn enqueue(&self, request: SpoolRequest) -> anyhow::Result<()>;
}
