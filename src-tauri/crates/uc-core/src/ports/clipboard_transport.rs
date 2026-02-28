//! Clipboard transport port.
//!
//! Defines clipboard payload send/receive capabilities over network transports.

use crate::network::ClipboardMessage;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait ClipboardTransportPort: Send + Sync {
    /// Send an encrypted clipboard payload to one peer.
    async fn send_clipboard(&self, peer_id: &str, encrypted_data: Vec<u8>) -> Result<()>;

    /// Broadcast an encrypted clipboard payload to all eligible peers.
    async fn broadcast_clipboard(&self, encrypted_data: Vec<u8>) -> Result<()>;

    /// Subscribe to incoming clipboard payloads.
    ///
    /// Contract: adapters may expose this as a single-consumer stream.
    async fn subscribe_clipboard(&self) -> Result<tokio::sync::mpsc::Receiver<ClipboardMessage>>;

    /// Ensure business protocol path is available before payload send.
    ///
    /// Default behavior is a no-op for adapters that do not support proactive setup.
    async fn ensure_business_path(&self, _peer_id: &str) -> Result<()> {
        Ok(())
    }
}
