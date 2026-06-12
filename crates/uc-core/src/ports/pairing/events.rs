//! Pairing event subscription port (Slice 1).
//!
//! Sponsor-side inbound notifications for the iroh-native pairing flow.
//! Kept separate from the legacy [`NetworkEventPort`] so Slice 5 can delete
//! the libp2p event stream without touching new Slice 1 event wiring.
//!
//! Event payloads use domain-meaningful fields only — peer identity is
//! surfaced via `PairingRequest.identity_pubkey` on the wire message (F-036
//! concept split), not via a transport-flavoured `peer_id` string.
//!
//! [`NetworkEventPort`]: crate::ports::network_events::NetworkEventPort

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc::Receiver;

use super::session::PairingSessionId;
use crate::pairing::PairingSessionMessage;

/// Sponsor-side inbound events from the pairing transport.
#[derive(Debug, Clone)]
pub enum PairingSessionEvent {
    /// An inbound pairing session has opened and the joiner's first
    /// [`PairingSessionMessage`] is available. Orchestrator matches
    /// `invitation_code` on the inner
    /// [`JoinerRequest`](crate::pairing::JoinerRequest) against the
    /// in-memory pending invitation (Q-B1-3).
    Incoming {
        session: PairingSessionId,
        message: PairingSessionMessage,
    },

    /// Follow-up message on an already-accepted session.
    MessageReceived {
        session: PairingSessionId,
        message: PairingSessionMessage,
    },

    /// Session closed — peer-initiated, timeout, or transport error. The
    /// orchestrator should release any per-session state (pending handshake,
    /// pending invitation marker if this was the only live session).
    Closed {
        session: PairingSessionId,
        reason: Option<String>,
    },
}

/// Subscription-style port: one receiver per subscriber.
///
/// Contract: adapters may expose this as a single-consumer stream. A
/// second call to [`subscribe`](Self::subscribe) replaces the previous
/// receiver (the old one's sender side is dropped).
#[async_trait]
pub trait PairingEventPort: Send + Sync {
    async fn subscribe(&self) -> Result<Receiver<PairingSessionEvent>>;
}
