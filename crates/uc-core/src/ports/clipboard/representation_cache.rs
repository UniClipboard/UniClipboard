use crate::ids::RepresentationId;

#[async_trait::async_trait]
pub trait RepresentationCachePort: Send + Sync {
    async fn put(&self, rep_id: &RepresentationId, bytes: Vec<u8>);
    async fn get(&self, rep_id: &RepresentationId) -> Option<Vec<u8>>;
    async fn mark_completed(&self, rep_id: &RepresentationId);
    async fn mark_spooling(&self, rep_id: &RepresentationId);
    async fn remove(&self, rep_id: &RepresentationId);
}
