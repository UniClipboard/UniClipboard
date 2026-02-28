//! Network event subscription port.

use crate::network::NetworkEvent;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait NetworkEventPort: Send + Sync {
    /// Subscribe to network events.
    ///
    /// Contract: adapters may expose this as a single-consumer stream.
    async fn subscribe_events(&self) -> Result<tokio::sync::mpsc::Receiver<NetworkEvent>>;
}
