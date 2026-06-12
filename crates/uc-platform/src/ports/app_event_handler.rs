use async_trait::async_trait;

#[async_trait]
pub trait AppEventHandlerPort: Send + Sync {
    type Event;

    async fn handle(&self, event: Self::Event);
}
