//! Transport-agnostic daemon service interface (ADR-008 P2.5).
//!
//! CLI commands depend on `DaemonService`, not on HTTP/WS details.
//! The current implementation is [`HttpWsDaemonService`]; future
//! transports (Unix domain socket, named pipe) only need a new
//! impl — CLI code stays unchanged.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;
use uc_daemon_contract::api::dto::clipboard::{
    EntryDetailDto, EntryProjectionResponseDto, EntryResourceDto,
};
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferResponse, DispatchOutcomeResponse, InboundEntryEvent, InboundNoticeEvent,
    ResendResponse,
};
use uc_daemon_contract::api::dto::setup_events::SetupPairingCompletedEvent;

/// A free-file exported from the daemon (ADR-008 P5-1b).
///
/// The bytes are the materialized file's contents; `filename` is the basename
/// the daemon advertised via `Content-Disposition` (sanitized on the daemon
/// side), suitable for writing into a caller-chosen output directory.
#[derive(Debug, Clone)]
pub struct FileExport {
    pub filename: String,
    pub bytes: Vec<u8>,
}

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

    /// Subscribe to `clipboard.new_content` events (ADR-008 P5-1b).
    ///
    /// Each delivered event signals that an inbound clipboard entry has been
    /// fully applied (including free-file materialization) and carries the
    /// **receiver-side** `entry_id`. The implementation filters to
    /// `origin == "remote"` so local clipboard captures do not leak through.
    async fn subscribe_inbound_entries(&self) -> Result<mpsc::Receiver<InboundEntryEvent>>;

    /// Export the bytes of an entry's first materialized free-file
    /// (ADR-008 P5-1b) by calling `GET /clipboard/entries/{id}/file`.
    ///
    /// Returns `Ok(Some(_))` on HTTP 200, `Ok(None)` on HTTP 404 (the entry
    /// is text-only / has no materialized file — callers waiting for a file
    /// should keep waiting), and `Err` for any other status or transport
    /// failure.
    async fn export_entry_file(&self, entry_id: &str) -> Result<Option<FileExport>>;

    /// List clipboard history entry projections, newest first, by calling
    /// `GET /clipboard/entries?limit&offset`. This is the real-time list view
    /// (NOT the search index), so it reliably reflects "the latest synced
    /// entry" — the basis for `uniclip get` on a headless node.
    async fn list_entries(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<EntryProjectionResponseDto>>;

    /// Fetch an entry's full text detail via `GET /clipboard/entries/{id}`.
    /// `Ok(None)` on 404; `Err` for other statuses (e.g. 422 non-text).
    async fn entry_detail(&self, entry_id: &str) -> Result<Option<EntryDetailDto>>;

    /// Fetch an entry's resource metadata via
    /// `GET /clipboard/entries/{id}/resource` (used to materialize images,
    /// which are not free-files). `Ok(None)` on 404.
    async fn entry_resource(&self, entry_id: &str) -> Result<Option<EntryResourceDto>>;

    /// Fetch raw blob bytes via `GET /clipboard/blobs/{blob_id}`.
    /// `Ok(None)` on 404.
    async fn fetch_blob(&self, blob_id: &str) -> Result<Option<Vec<u8>>>;

    /// Subscribe to `setup.pairingCompleted` events (ADR-008 P5-2b).
    ///
    /// Returns a receiver that yields [`SetupPairingCompletedEvent`] payloads
    /// when the daemon's setup topic broadcasts a pairing completion (success
    /// or failure). The WS connection doubles as a control-WS lease.
    async fn subscribe_setup_pairing_completion(
        &self,
    ) -> Result<mpsc::Receiver<SetupPairingCompletedEvent>>;

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
