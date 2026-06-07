//! Transport-agnostic daemon service interface (ADR-008 P2.5).
//!
//! CLI commands depend on `DaemonService`, not on HTTP/WS details.
//! The current implementation is [`HttpWsDaemonService`]; future
//! transports (Unix domain socket, named pipe) only need a new
//! impl — CLI code stays unchanged.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferResponse, DispatchOutcomeResponse, InboundNoticeEvent, ResendResponse,
};

#[async_trait]
pub trait DaemonService: Send + Sync {
    async fn dispatch_text(
        &self,
        text: &str,
        peers: Option<Vec<String>>,
    ) -> Result<DispatchOutcomeResponse>;

    async fn resend_entry(
        &self,
        entry_id: &str,
        peers: Option<Vec<String>>,
    ) -> Result<ResendResponse>;

    async fn cancel_transfer(
        &self,
        transfer_id: &str,
        reason: &str,
    ) -> Result<CancelTransferResponse>;

    async fn subscribe_inbound_notices(&self) -> Result<mpsc::Receiver<InboundNoticeEvent>>;

    /// Open a bare control WebSocket that the daemon counts as an active
    /// lease (ADR-008 P5-1a). The connection does NOT subscribe to any topic —
    /// it exists solely to keep a transient Oneshot daemon alive while a
    /// short-lived command (e.g. `send`) does its work. Dropping the returned
    /// guard closes the WS, releasing the lease.
    async fn hold_control_lease(&self) -> Result<ControlLeaseGuard>;
}

/// RAII guard for a held control-WS lease (ADR-008 P5-1a). Dropping it signals
/// the background keep-alive task to close the WebSocket, which makes the
/// daemon release the lease. A `noop()` guard holds nothing (for impls that
/// model no real connection).
pub struct ControlLeaseGuard {
    shutdown: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl ControlLeaseGuard {
    pub(crate) fn new(
        shutdown: tokio::sync::oneshot::Sender<()>,
        task: tokio::task::JoinHandle<()>,
    ) -> Self {
        Self {
            shutdown: Some(shutdown),
            task: Some(task),
        }
    }

    pub fn noop() -> Self {
        Self {
            shutdown: None,
            task: None,
        }
    }
}

impl Drop for ControlLeaseGuard {
    fn drop(&mut self) {
        // Signal the keep-alive task to send a clean Close; if it already
        // exited, this is a no-op. We do NOT block on the task — process
        // exit / TCP teardown releases the lease even if Close never flushes,
        // and the daemon's CLIENT_TIMEOUT (40s) + supervisor grace cover lag.
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.task.take() {
            handle.abort();
        }
    }
}
