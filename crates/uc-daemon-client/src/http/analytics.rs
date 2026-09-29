use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::empty_request;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::analytics::CaptureUiEventRequest;

/// Client for `POST /analytics/capture` (ADR-008 D20).
///
/// The daemon is the single authoritative product-analytics sender. UI-process
/// events that originate in the GUI's own Rust background tasks (the updater /
/// scheduler, which the webview never sees) are forwarded here so the daemon
/// dispatches them through its own gated sink + `EventContext`, instead of the
/// GUI holding a second in-process PostHog sink that would double-count
/// device-level signals.
///
/// Fire-and-forget on the wire: a successful call only confirms the daemon
/// decoded the event and handed it to its sink, not that it reached PostHog.
#[derive(Clone)]
pub struct DaemonAnalyticsClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonAnalyticsClient {
    pub fn new(connection_state: DaemonConnectionState) -> Result<Self> {
        Ok(Self {
            http: Arc::new(crate::build_local_http_client()?),
            connection_state,
            client_type: "gui".to_string(),
        })
    }

    /// POST a single UI-interaction event to the daemon. Returns `Err` on a
    /// missing connection, transport failure, or non-2xx status — callers
    /// treat analytics as best-effort and should only `debug`-log the error.
    pub async fn capture(&self, event: CaptureUiEventRequest) -> Result<()> {
        Ok(empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/analytics/capture",
            |r| r.json(&event),
        )
        .await?)
    }

    /// Fire-and-forget [`Self::capture`] for synchronous call sites (the GUI
    /// updater and update scheduler). The POST is spawned on the ambient tokio
    /// runtime and its failure is only `debug`-logged; outside a runtime the
    /// event is dropped with a debug line instead of panicking.
    pub fn capture_in_background(&self, event: CaptureUiEventRequest) {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let client = self.clone();
                handle.spawn(async move {
                    if let Err(err) = client.capture(event).await {
                        tracing::debug!(
                            error = %err,
                            "daemon-forward analytics: capture failed (best-effort)"
                        );
                    }
                });
            }
            Err(_) => {
                tracing::debug!("daemon-forward analytics: no tokio runtime; dropping event");
            }
        }
    }
}
