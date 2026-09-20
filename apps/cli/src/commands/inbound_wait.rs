use tokio::sync::mpsc;
use uc_daemon_client::{ControlLeaseGuard, DaemonService};
use uc_daemon_contract::api::dto::clipboard_command::InboundEntryEvent;

use crate::commands::app_session::wait_and_reconnect_daemon;
use crate::exit_codes;
use crate::ui;

const RECONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One subscribed daemon session used by all one-shot inbound waits.
///
/// The subscription is established before the caller starts waiting, so an
/// entry already present at command startup cannot satisfy the wait. The
/// control lease keeps a transient daemon alive for the whole operation.
pub struct InboundWaitSession {
    service: Box<dyn DaemonService>,
    _lease: ControlLeaseGuard,
    entries: mpsc::Receiver<InboundEntryEvent>,
    reconnected: bool,
}

impl InboundWaitSession {
    pub async fn connect(service: Box<dyn DaemonService>) -> Result<Self, i32> {
        let lease = service.hold_control_lease().await.map_err(|err| {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            exit_codes::EXIT_ERROR
        })?;
        let entries = service.subscribe_inbound_entries().await.map_err(|err| {
            ui::error(&format!("Failed to subscribe inbound entries: {err}"));
            exit_codes::EXIT_ERROR
        })?;
        Ok(Self {
            service,
            _lease: lease,
            entries,
            reconnected: false,
        })
    }

    pub fn service(&self) -> &dyn DaemonService {
        &*self.service
    }

    /// Wait for one remote entry. `Ok(None)` means the user pressed Ctrl-C.
    pub async fn next(&mut self) -> Result<Option<InboundEntryEvent>, i32> {
        loop {
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => return Ok(None),
                entry = self.entries.recv() => match entry {
                    Some(entry) => return Ok(Some(entry)),
                    None => self.reconnect().await?,
                }
            }
        }
    }

    async fn reconnect(&mut self) -> Result<(), i32> {
        if self.reconnected {
            ui::error("Inbound channel closed again; exiting.");
            return Err(exit_codes::EXIT_ERROR);
        }
        ui::warn("Daemon connection lost — reconnecting...");
        let service = wait_and_reconnect_daemon(RECONNECT_TIMEOUT).await?;
        let lease = service.hold_control_lease().await.map_err(|err| {
            ui::error(&format!(
                "Failed to re-acquire lease after reconnect: {err}"
            ));
            exit_codes::EXIT_ERROR
        })?;
        let entries = service.subscribe_inbound_entries().await.map_err(|err| {
            ui::error(&format!("Failed to re-subscribe after reconnect: {err}"));
            exit_codes::EXIT_ERROR
        })?;
        self.service = service;
        self._lease = lease;
        self.entries = entries;
        self.reconnected = true;
        ui::warn("Reconnected — events during daemon restart may have been missed");
        Ok(())
    }
}
