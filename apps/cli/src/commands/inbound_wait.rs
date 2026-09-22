use tokio::sync::mpsc;
use uc_daemon_client::{ControlLeaseGuard, DaemonService, InboundActivityEvent};
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
    stream: InboundWaitStream,
    reconnected: bool,
}

enum InboundWaitStream {
    Entries(mpsc::Receiver<InboundEntryEvent>),
    Activity(mpsc::Receiver<InboundActivityEvent>),
}

pub enum InboundActivityUpdate {
    Event(InboundActivityEvent),
    Reconnected,
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
            stream: InboundWaitStream::Entries(entries),
            reconnected: false,
        })
    }

    pub async fn connect_activity(service: Box<dyn DaemonService>) -> Result<Self, i32> {
        let lease = service.hold_control_lease().await.map_err(|err| {
            ui::error(&format!("Failed to hold daemon session lease: {err}"));
            exit_codes::EXIT_ERROR
        })?;
        let activity = service.subscribe_inbound_activity().await.map_err(|err| {
            ui::error(&format!("Failed to subscribe inbound activity: {err}"));
            exit_codes::EXIT_ERROR
        })?;
        Ok(Self {
            service,
            _lease: lease,
            stream: InboundWaitStream::Activity(activity),
            reconnected: false,
        })
    }

    pub fn service(&self) -> &dyn DaemonService {
        &*self.service
    }

    /// Wait for one remote entry. `Ok(None)` means the user pressed Ctrl-C.
    pub async fn next(&mut self) -> Result<Option<InboundEntryEvent>, i32> {
        loop {
            let InboundWaitStream::Entries(entries) = &mut self.stream else {
                ui::error("Inbound wait session is not configured for completed entries.");
                return Err(exit_codes::EXIT_ERROR);
            };
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => return Ok(None),
                entry = entries.recv() => match entry {
                    Some(entry) => return Ok(Some(entry)),
                    None => {
                        tokio::select! {
                            biased;
                            _ = tokio::signal::ctrl_c() => return Ok(None),
                            result = self.reconnect() => result?,
                        }
                    }
                }
            }
        }
    }

    /// Wait for the next ordered inbound activity event. `Ok(None)` means the
    /// user pressed Ctrl-C.
    pub async fn next_activity(&mut self) -> Result<Option<InboundActivityUpdate>, i32> {
        loop {
            let InboundWaitStream::Activity(activity) = &mut self.stream else {
                ui::error("Inbound wait session is not configured for activity events.");
                return Err(exit_codes::EXIT_ERROR);
            };
            tokio::select! {
                biased;
                _ = tokio::signal::ctrl_c() => return Ok(None),
                event = activity.recv() => match event {
                    Some(event) => return Ok(Some(InboundActivityUpdate::Event(event))),
                    None => {
                        tokio::select! {
                            biased;
                            _ = tokio::signal::ctrl_c() => return Ok(None),
                            result = self.reconnect() => {
                                result?;
                                return Ok(Some(InboundActivityUpdate::Reconnected));
                            },
                        }
                    }
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
        let stream = match self.stream {
            InboundWaitStream::Entries(_) => {
                let entries = service.subscribe_inbound_entries().await.map_err(|err| {
                    ui::error(&format!("Failed to re-subscribe after reconnect: {err}"));
                    exit_codes::EXIT_ERROR
                })?;
                InboundWaitStream::Entries(entries)
            }
            InboundWaitStream::Activity(_) => {
                let activity = service.subscribe_inbound_activity().await.map_err(|err| {
                    ui::error(&format!("Failed to re-subscribe inbound activity: {err}"));
                    exit_codes::EXIT_ERROR
                })?;
                InboundWaitStream::Activity(activity)
            }
        };
        self.service = service;
        self._lease = lease;
        self.stream = stream;
        self.reconnected = true;
        ui::warn("Reconnected — events during daemon restart may have been missed");
        Ok(())
    }
}
