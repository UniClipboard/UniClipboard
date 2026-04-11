//! Host event emitter port — abstract event delivery for background tasks.
//!
//! This module defines the [`HostEventEmitterPort`] trait and the [`HostEvent`]
//! type system that background tasks use to deliver events to the host environment.
//!
//! The port is intentionally free of Tauri, serde, and any infrastructure
//! dependency. Adapters (e.g., `DaemonApiEventEmitter`) own the serialization
//! contract and event name mapping.
//!
//! # Design
//!
//! - [`HostEvent`] is a pure semantic model — no serde annotations.
//! - [`HostEventEmitterPort`] is synchronous (fire-and-forget semantics).
//! - Emit failures are best-effort: callers log the error and continue.

use crate::ports::transfer_progress::TransferDirection;
use crate::setup::SetupState;

// ---------------------------------------------------------------------------
// ClipboardOriginKind
// ---------------------------------------------------------------------------

/// Indicates whether clipboard content originated locally or from a remote peer.
#[derive(Debug, Clone)]
pub enum ClipboardOriginKind {
    /// Captured from the local clipboard watcher.
    Local,
    /// Received from a remote peer via sync.
    Remote,
}

// ---------------------------------------------------------------------------
// ClipboardHostEvent
// ---------------------------------------------------------------------------

/// Semantic events emitted by the clipboard subsystem.
#[derive(Debug, Clone)]
pub enum ClipboardHostEvent {
    /// New clipboard content was captured or received.
    ///
    /// `preview` is always present — a brief text summary of the content.
    NewContent {
        entry_id: String,
        preview: String,
        origin: ClipboardOriginKind,
    },
}

// ---------------------------------------------------------------------------
// TransferHostEvent
// ---------------------------------------------------------------------------

/// Semantic events emitted by the file transfer subsystem.
#[derive(Debug, Clone)]
pub enum TransferHostEvent {
    /// The status of a transfer entry changed.
    StatusChanged {
        transfer_id: String,
        entry_id: String,
        status: String,
        reason: Option<String>,
    },
    /// Continuous transfer progress update.
    Progress {
        transfer_id: String,
        entry_id: Option<String>,
        peer_id: String,
        direction: TransferDirection,
        chunks_completed: u32,
        total_chunks: u32,
        bytes_transferred: u64,
        total_bytes: Option<u64>,
    },
}

// ---------------------------------------------------------------------------
// SetupHostEvent
// ---------------------------------------------------------------------------

/// Semantic events emitted by the setup subsystem.
#[derive(Debug, Clone)]
pub enum SetupHostEvent {
    /// The setup wizard state changed.
    ///
    /// IMPORTANT: `state` carries the full `SetupState` enum (not a String) to
    /// preserve data-carrying variants (JoinSpaceConfirmPeer, ProcessingCreateSpace, etc.).
    StateChanged {
        state: SetupState,
        session_id: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// SpaceAccessHostEvent
// ---------------------------------------------------------------------------

/// Semantic events emitted by the space access subsystem.
#[derive(Debug, Clone)]
pub enum SpaceAccessHostEvent {
    /// A space access attempt completed (WebDAV / local path).
    ///
    /// IMPORTANT: `peer_id` is `String` (non-optional), matching the existing
    /// wire contract and `SpaceAccessCompletedEvent.peer_id: String`.
    Completed {
        session_id: String,
        peer_id: String,
        success: bool,
        reason: Option<String>,
        ts: i64,
    },
    /// A P2P space access attempt completed.
    P2PCompleted {
        session_id: String,
        peer_id: String,
        success: bool,
        reason: Option<String>,
        ts: i64,
    },
}

// ---------------------------------------------------------------------------
// HostEvent
// ---------------------------------------------------------------------------

/// Top-level host event enum — groups all in-scope semantic events by domain.
///
/// This is a pure Rust type with no serde annotations. Adapters are solely
/// responsible for serialization to frontend wire formats.
#[derive(Debug, Clone)]
pub enum HostEvent {
    Clipboard(ClipboardHostEvent),
    Transfer(TransferHostEvent),
    Setup(SetupHostEvent),
    SpaceAccess(SpaceAccessHostEvent),
}

// ---------------------------------------------------------------------------
// EmitError
// ---------------------------------------------------------------------------

/// Error returned when [`HostEventEmitterPort::emit`] fails.
///
/// Emit failures are best-effort — callers should log the error and continue.
#[derive(Debug, thiserror::Error)]
pub enum EmitError {
    #[error("emit failed: {0}")]
    Failed(String),
}

// ---------------------------------------------------------------------------
// HostEventEmitterPort
// ---------------------------------------------------------------------------

/// Abstract port for delivering host events to the runtime environment.
///
/// Implementations:
/// - `DaemonApiEventEmitter` — broadcasts via daemon WebSocket.
/// - `LoggingEventEmitter` — writes structured `tracing` output, always returns `Ok`.
///
/// The trait is synchronous — event delivery is fire-and-forget.
pub trait HostEventEmitterPort: Send + Sync {
    /// Deliver a host event to the runtime environment.
    ///
    /// On failure, the error is returned for the caller to log. The caller
    /// **must not** propagate the error as a business-logic failure.
    fn emit(&self, event: HostEvent) -> Result<(), EmitError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::SetupState;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingEmitter {
        events: Mutex<Vec<HostEvent>>,
    }

    impl HostEventEmitterPort for RecordingEmitter {
        fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
            self.events.lock().unwrap().push(event);
            Ok(())
        }
    }

    #[test]
    fn host_event_port_accepts_all_in_scope_events_without_infra_types() {
        let emitter = RecordingEmitter::default();

        let events = vec![
            // --- Clipboard ---
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "entry-1".to_string(),
                preview: "hello".to_string(),
                origin: ClipboardOriginKind::Local,
            }),
            // --- Transfer ---
            HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "transfer-3".to_string(),
                entry_id: "entry-3".to_string(),
                status: "pending".to_string(),
                reason: None,
            }),
            HostEvent::Transfer(TransferHostEvent::Progress {
                transfer_id: "transfer-4".to_string(),
                entry_id: Some("entry-4".to_string()),
                peer_id: "peer-4".to_string(),
                direction: TransferDirection::Sending,
                chunks_completed: 3,
                total_chunks: 7,
                bytes_transferred: 1_024,
                total_bytes: Some(2_048),
            }),
            // --- Setup ---
            HostEvent::Setup(SetupHostEvent::StateChanged {
                state: SetupState::Welcome,
                session_id: None,
            }),
            // --- SpaceAccess ---
            HostEvent::SpaceAccess(SpaceAccessHostEvent::Completed {
                session_id: "sa-session-1".to_string(),
                peer_id: "peer-7".to_string(),
                success: true,
                reason: None,
                ts: 1_700_000_000,
            }),
            HostEvent::SpaceAccess(SpaceAccessHostEvent::P2PCompleted {
                session_id: "sa-session-2".to_string(),
                peer_id: "peer-8".to_string(),
                success: false,
                reason: Some("timeout".to_string()),
                ts: 1_700_000_001,
            }),
        ];

        for event in events {
            emitter.emit(event).expect("emit through port");
        }

        assert_eq!(
            emitter.events.lock().unwrap().len(),
            6,
            "all HostEvent variants should be deliverable through the core port"
        );
    }
}
