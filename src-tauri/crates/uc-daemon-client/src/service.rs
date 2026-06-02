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
}
