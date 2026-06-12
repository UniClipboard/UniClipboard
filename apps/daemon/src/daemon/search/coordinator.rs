//! Daemon wrapper for the application search coordinator.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};
use uc_application::facade::{SearchCoordinator, SearchCoordinatorEvent};
use uc_daemon_contract::constants::{ws_event, ws_topic};

use crate::daemon::service::{DaemonService, ServiceHealth};
use uc_webserver::api::types::DaemonWsEvent;

pub struct SearchCoordinatorService {
    coordinator: Arc<SearchCoordinator>,
    event_tx: broadcast::Sender<DaemonWsEvent>,
}

impl SearchCoordinatorService {
    pub fn new(
        coordinator: Arc<SearchCoordinator>,
        event_tx: broadcast::Sender<DaemonWsEvent>,
    ) -> Self {
        Self {
            coordinator,
            event_tx,
        }
    }
}

#[async_trait]
impl DaemonService for SearchCoordinatorService {
    fn name(&self) -> &str {
        "search-coordinator"
    }

    async fn start(&self, cancel: CancellationToken) -> anyhow::Result<()> {
        info!("search coordinator service starting");
        let mut rx = self.coordinator.subscribe_events();
        let forward_cancel = cancel.child_token();
        let event_tx = self.event_tx.clone();
        let forwarder = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = forward_cancel.cancelled() => return,
                    event = rx.recv() => {
                        match event {
                            Ok(event) => forward_search_event(&event_tx, event),
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                debug!(skipped, "search coordinator service dropped application events");
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                }
            }
        });

        let result = self.coordinator.start(cancel).await;
        forwarder.abort();
        result
    }

    async fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn health_check(&self) -> ServiceHealth {
        ServiceHealth::Healthy
    }
}

fn forward_search_event(
    event_tx: &broadcast::Sender<DaemonWsEvent>,
    event: SearchCoordinatorEvent,
) {
    let (event_type, payload) = match event {
        SearchCoordinatorEvent::Status(snapshot) => {
            let payload = match serde_json::to_value(snapshot) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(error = %err, "failed to serialize search status snapshot");
                    return;
                }
            };
            (ws_event::SEARCH_STATUS_SNAPSHOT, payload)
        }
        SearchCoordinatorEvent::RebuildProgress(progress) => {
            let payload = match serde_json::to_value(progress) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(error = %err, "failed to serialize search rebuild progress");
                    return;
                }
            };
            (ws_event::SEARCH_REBUILD_PROGRESS, payload)
        }
    };

    let event = DaemonWsEvent {
        topic: ws_topic::SEARCH.to_string(),
        event_type: event_type.to_string(),
        session_id: None,
        ts: chrono::Utc::now().timestamp_millis(),
        payload,
    };
    if let Err(err) = event_tx.send(event) {
        debug!(error = %err, "no WS subscribers for search coordinator event");
    }
}
