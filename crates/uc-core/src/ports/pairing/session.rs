//! Pairing session port (Slice 1).
//!
//! Replaces the legacy [`PairingTransportPort`] for the iroh-native pairing
//! flow. The legacy port exposes a libp2p-flavoured `peer_id: String`; this
//! port stays implementation-agnostic by returning an opaque
//! [`PairingSessionId`] that adapters mint.
//!
//! Joiner side drives pairing via [`dial_by_invitation`]. Sponsor side
//! receives inbound sessions through the companion [`PairingEventPort`] (see
//! `super::events`) and then uses [`send`] / [`recv_next`] / [`close`] on the
//! same [`PairingSessionId`] the event carried.
//!
//! [`PairingTransportPort`]: crate::ports::pairing_transport::PairingTransportPort
//! [`dial_by_invitation`]: PairingSessionPort::dial_by_invitation
//! [`send`]: PairingSessionPort::send
//! [`recv_next`]: PairingSessionPort::recv_next
//! [`close`]: PairingSessionPort::close
//! [`PairingEventPort`]: super::events::PairingEventPort

use async_trait::async_trait;
use thiserror::Error;

use crate::pairing::{InvitationCode, PairingSessionMessage};

/// Opaque identifier for an in-flight pairing session.
///
/// Adapters pick the concrete format (iroh EndpointId + stream id, UUID,
/// …); the core only uses it for correlation between dial/send/recv/close
/// and between sponsor-side events and subsequent operations.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PairingSessionId(String);

impl PairingSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for PairingSessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Errors raised by [`PairingSessionPort::dial_by_invitation`].
#[derive(Debug, Error)]
pub enum DialError {
    /// No discovery channel returned a record for this code — typo, never
    /// issued, or already reaped. For adapters that resolve through more
    /// than one channel, this is returned only after every channel has
    /// reported "no record".
    #[error("invitation not found")]
    InvitationNotFound,

    /// At least one discovery channel returned a record, but it is past
    /// its TTL. Kept distinct from `InvitationNotFound` so the UI can tell
    /// stale codes from typos.
    #[error("invitation has expired")]
    InvitationExpired,

    /// Sponsor advertised an address but the underlying transport couldn't
    /// establish a connection (NAT, relay down, sponsor went offline).
    #[error("sponsor is not reachable")]
    SponsorUnreachable,

    /// Every discovery channel the adapter would query is itself
    /// unreachable, so no resolution attempt could even start. Distinct
    /// from `InvitationNotFound`, which means the channel(s) answered but
    /// had nothing for this code.
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,

    /// Adapter-side failure; message is for logs only.
    #[error("internal dial error: {0}")]
    Internal(String),
}

/// Which discovery channel resolved an invitation before the dial.
///
/// Adapters that race several channels report whichever resolution won;
/// adapters with a single channel always report that channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoveryChannel {
    /// Resolved via the directory service.
    Cloud,
    /// Resolved via local-network discovery.
    Lan,
}

/// Outcome of a successful [`PairingSessionPort::dial_by_invitation`].
#[derive(Debug, Clone)]
pub struct DialOutcome {
    /// Handle for the opened pairing session.
    pub session_id: PairingSessionId,
    /// The discovery channel whose resolution won the dial.
    pub channel: DiscoveryChannel,
}

/// Errors raised by send/recv/close on a session.
#[derive(Debug, Error)]
pub enum SessionError {
    /// No session with this id exists (adapter has no record, or it was
    /// already closed and GC'd).
    #[error("pairing session not found: {0}")]
    NotFound(PairingSessionId),

    /// Session was closed (locally or by peer) before this call completed.
    #[error("pairing session already closed")]
    Closed,

    /// Adapter-side failure; message is for logs only.
    #[error("internal session error: {0}")]
    Internal(String),
}

/// Session-level pairing transport (Slice 1).
#[async_trait]
pub trait PairingSessionPort: Send + Sync {
    /// Joiner entry point. Resolves the invitation through one or more
    /// discovery channels (adapters may race them in parallel), dials the
    /// sponsor at the first successful resolution, opens a bi-directional
    /// stream, and returns the session handle together with the discovery
    /// channel that resolved the invitation. No bytes are sent by this
    /// call — the caller writes the first [`PairingSessionMessage`] via
    /// [`send`](Self::send).
    async fn dial_by_invitation(&self, code: &InvitationCode) -> Result<DialOutcome, DialError>;

    /// Send a pairing message on an existing session. Used by both sides
    /// throughout the handshake.
    async fn send(
        &self,
        session: &PairingSessionId,
        message: PairingSessionMessage,
    ) -> Result<(), SessionError>;

    /// Receive the next pairing message on a session. `Ok(None)` means the
    /// peer closed the stream cleanly; callers should treat it as end of
    /// conversation and release the session.
    async fn recv_next(
        &self,
        session: &PairingSessionId,
    ) -> Result<Option<PairingSessionMessage>, SessionError>;

    /// Close a session. Idempotent — calling on an already-closed session
    /// is a no-op. Takes `&self` (not `self`) so the caller keeps the id
    /// around for logging.
    async fn close(&self, session: &PairingSessionId, reason: Option<String>);

    /// 返回本地传输地址的不透明编码（Slice 2 Phase 1 · T5）。
    ///
    /// 供 handshake coordinator 在发送 `JoinerRequest` / `SponsorConfirm`
    /// 前填充 `transport_address_blob` 字段使用。adapter 自己决定编码格式
    /// （iroh adapter 用 postcard 编码 `EndpointAddr`），core/application
    /// 只把字节透传给对端。
    ///
    /// 返回 `None` 表示 adapter 暂时无法提供（例如 endpoint 尚未发布 direct
    /// addrs，或测试用假 adapter 不实现此能力）；调用方应发送空 `Vec`，对端
    /// 接到空 blob 后会跳过 `peer_addr_repo.upsert`，由 `ensure_reachable_all`
    /// 下次重试兜底。
    async fn local_transport_address_blob(&self) -> Option<Vec<u8>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_id_round_trips_through_display() {
        let id = PairingSessionId::new("abc-123");
        assert_eq!(id.as_str(), "abc-123");
        assert_eq!(format!("{id}"), "abc-123");
    }

    #[test]
    fn session_id_equality_is_structural() {
        let a = PairingSessionId::new("x");
        let b = PairingSessionId::new("x");
        let c = PairingSessionId::new("y");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn dial_error_messages_are_human_readable() {
        assert_eq!(
            DialError::InvitationNotFound.to_string(),
            "invitation not found"
        );
        assert_eq!(
            DialError::InvitationExpired.to_string(),
            "invitation has expired"
        );
        assert_eq!(
            DialError::SponsorUnreachable.to_string(),
            "sponsor is not reachable"
        );
        assert_eq!(
            DialError::ServiceUnavailable.to_string(),
            "pairing invitation service unavailable"
        );
        assert_eq!(
            DialError::Internal("boom".into()).to_string(),
            "internal dial error: boom"
        );
    }

    #[test]
    fn session_error_carries_id_in_not_found() {
        let id = PairingSessionId::new("sess-42");
        let err = SessionError::NotFound(id);
        assert_eq!(err.to_string(), "pairing session not found: sess-42");
    }

    #[test]
    fn session_error_closed_is_flat() {
        assert_eq!(
            SessionError::Closed.to_string(),
            "pairing session already closed"
        );
    }
}
