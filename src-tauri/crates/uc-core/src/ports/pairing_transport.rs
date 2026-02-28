//! Pairing transport port.
//!
//! Defines session-oriented transport capabilities used by pairing workflows.

use crate::network::PairingMessage;
use anyhow::Result;
use async_trait::async_trait;

#[async_trait]
pub trait PairingTransportPort: Send + Sync {
    /// Open a pairing session-specific stream toward a peer. Best-effort.
    async fn open_pairing_session(&self, peer_id: String, session_id: String) -> Result<()>;

    /// Send a message on an already opened pairing session stream.
    async fn send_pairing_on_session(&self, message: PairingMessage) -> Result<()>;

    /// Close a pairing session stream, optionally reporting a reason.
    async fn close_pairing_session(&self, session_id: String, reason: Option<String>)
        -> Result<()>;

    /// Unpair a device.
    async fn unpair_device(&self, peer_id: String) -> Result<()>;
}
