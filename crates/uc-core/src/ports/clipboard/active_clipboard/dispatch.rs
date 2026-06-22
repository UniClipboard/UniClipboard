//! Outbound active-clipboard register port.
//!
//! Complement to [`ActiveClipboardReceiverPort`](super::receiver::ActiveClipboardReceiverPort):
//! that port exposes the active-clipboard state observations arriving from
//! peers, this one sends one such observation to a single reachable peer.
//! Multi-target fan-out (selecting eligible peers, concurrency) is assembled
//! by the application layer, so the port stays a single-target primitive.

use async_trait::async_trait;

use crate::clipboard::ActiveClipboardState;
use crate::ids::DeviceId;

/// Error surface for sending an active-clipboard state observation.
#[derive(Debug, thiserror::Error)]
pub enum ActiveClipboardDispatchError {
    /// No reachable connection could be established for the target (missing
    /// address or dial failure). Treated by the caller as "peer offline".
    #[error("target device offline or unreachable")]
    Offline,
    /// Stream I/O failure — broken connection, short write, etc.
    #[error("stream io: {0}")]
    Io(String),
    /// Other internal failure.
    #[error("internal: {0}")]
    Internal(String),
}

/// Single-target send of one active-clipboard register observation.
///
/// The state is a last-writer-wins observation — "this content is now the
/// active clipboard, activated at `activated_at_ms` by `activated_by`". It is
/// fire-and-forget: the call returns once the bytes are written, without
/// waiting for the peer to acknowledge or apply them. Convergence is the
/// register's responsibility, not this send's.
///
/// `entry_id` on the carried [`ActiveClipboardState`] is the *sending*
/// device's per-device handle; it travels for traceability only and is never
/// resolved by the receiver, which looks the content up by `snapshot_hash`.
#[async_trait]
pub trait ActiveClipboardDispatchPort: Send + Sync {
    /// Send `state` to `target`. Returns once the observation has been
    /// written to the peer's stream; an unreachable peer surfaces as
    /// [`ActiveClipboardDispatchError::Offline`].
    async fn dispatch(
        &self,
        target: &DeviceId,
        state: &ActiveClipboardState,
    ) -> Result<(), ActiveClipboardDispatchError>;
}
