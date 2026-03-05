//! Clipboard transport port.
//!
//! Defines clipboard payload send/receive capabilities over network transports.
//! Uses `Arc<[u8]>` for outbound payloads to enable zero-copy multi-peer fanout.

use crate::network::ClipboardMessage;
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait ClipboardTransportPort: Send + Sync {
    /// Send an encrypted clipboard payload to one peer.
    ///
    /// Accepts `Arc<[u8]>` to allow zero-copy fanout when sending the same
    /// payload to multiple peers — each peer receives a cheap Arc clone.
    async fn send_clipboard(&self, peer_id: &str, encrypted_data: Arc<[u8]>) -> Result<()>;

    /// Broadcast an encrypted clipboard payload to all eligible peers.
    ///
    /// Accepts `Arc<[u8]>` for zero-copy multi-peer fanout.
    async fn broadcast_clipboard(&self, encrypted_data: Arc<[u8]>) -> Result<()>;

    /// Subscribe to incoming clipboard payloads.
    ///
    /// Returns `(ClipboardMessage, Option<Vec<u8>>)` where:
    /// - `Some(bytes)` = pre-decoded plaintext (already decrypted at transport level)
    /// - `None` = fallback (encrypted_content contains the payload, use case must decrypt)
    ///
    /// Contract: adapters may expose this as a single-consumer stream.
    async fn subscribe_clipboard(
        &self,
    ) -> Result<tokio::sync::mpsc::Receiver<(ClipboardMessage, Option<Vec<u8>>)>>;

    /// Ensure business protocol path is available before payload send.
    ///
    /// Default behavior is a no-op for adapters that do not support proactive setup.
    async fn ensure_business_path(&self, _peer_id: &str) -> Result<()> {
        Ok(())
    }
}
