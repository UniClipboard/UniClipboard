//! Network port - abstracts P2P network capabilities
//!
//! This port defines the interface for network operations including
//! clipboard messaging, peer discovery, device pairing, and event subscriptions.

use crate::network::{
    ClipboardMessage, ConnectedPeer, DiscoveredPeer, NetworkEvent, PairingMessage,
};
use anyhow::Result;
use async_trait::async_trait;

/// Network port - abstracts P2P network capabilities
///
/// This trait provides a clean abstraction over libp2p/network layer,
/// allowing use cases to interact with network functionality without
/// depending on concrete implementations.
#[async_trait]
pub trait NetworkPort: Send + Sync {
    // === Clipboard operations ===

    /// Send clipboard message to a specific peer
    async fn send_clipboard(&self, peer_id: &str, encrypted_data: Vec<u8>) -> Result<()>;

    /// Broadcast clipboard message to all connected peers
    async fn broadcast_clipboard(&self, encrypted_data: Vec<u8>) -> Result<()>;

    /// Subscribe to incoming clipboard messages
    ///
    /// Returns a receiver that will yield clipboard messages received from remote peers.
    async fn subscribe_clipboard(&self) -> Result<tokio::sync::mpsc::Receiver<ClipboardMessage>>;

    // === Peer operations ===

    /// Get all discovered peers (from mDNS)
    async fn get_discovered_peers(&self) -> Result<Vec<DiscoveredPeer>>;

    /// Get currently connected peers
    async fn get_connected_peers(&self) -> Result<Vec<ConnectedPeer>>;

    /// List peers that are eligible for business payload fan-out.
    ///
    /// Default behavior returns discovered peers marked as paired.
    async fn list_sendable_peers(&self) -> Result<Vec<DiscoveredPeer>> {
        let peers = self.get_discovered_peers().await?;
        Ok(peers
            .into_iter()
            .filter(|peer| peer.is_paired)
            .collect::<Vec<_>>())
    }

    /// Ensure business protocol path is available for a peer before sending payload.
    ///
    /// Default behavior is a no-op for adapters that do not support proactive path setup.
    async fn ensure_business_path(&self, _peer_id: &str) -> Result<()> {
        Ok(())
    }

    /// Get local peer ID
    fn local_peer_id(&self) -> String;

    /// Announce local device name to peers
    async fn announce_device_name(&self, device_name: String) -> Result<()>;

    // === Pairing operations ===

    /// Open a pairing session-specific stream toward a peer. Best-effort.
    async fn open_pairing_session(&self, peer_id: String, session_id: String) -> Result<()>;

    /// Send a message on an already opened pairing session stream.
    async fn send_pairing_on_session(
        &self,
        session_id: String,
        message: PairingMessage,
    ) -> Result<()>;

    /// Close a pairing session stream, optionally reporting a reason.
    async fn close_pairing_session(&self, session_id: String, reason: Option<String>)
        -> Result<()>;

    /// Unpair a device
    async fn unpair_device(&self, peer_id: String) -> Result<()>;

    // === Event operations ===

    /// Subscribe to network events
    ///
    /// Returns a receiver that will yield network events including:
    /// - Peer discovery/loss
    /// - Connection/disconnection
    /// - Pairing state changes
    /// - Clipboard send/receive confirmations
    async fn subscribe_events(&self) -> Result<tokio::sync::mpsc::Receiver<NetworkEvent>>;
}
