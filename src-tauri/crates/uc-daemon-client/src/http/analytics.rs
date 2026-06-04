use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::{Method, RequestBuilder};

use crate::http::authorized_daemon_request_with_type;
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
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: Arc::new(reqwest::Client::new()),
            connection_state,
            client_type: "gui".to_string(),
        }
    }

    /// POST a single UI-interaction event to the daemon. Returns `Err` on a
    /// missing connection, transport failure, or non-2xx status — callers
    /// treat analytics as best-effort and should only `debug`-log the error.
    pub async fn capture(&self, event: CaptureUiEventRequest) -> Result<()> {
        let response = self
            .authorized_request(Method::POST, "/analytics/capture")
            .await?
            .json(&event)
            .send()
            .await
            .with_context(|| "failed to call daemon /analytics/capture")?;

        if response.status().is_success() {
            return Ok(());
        }
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "<failed to read body>".to_string());
        Err(anyhow!(
            "daemon /analytics/capture failed with status {}: {}",
            status,
            body,
        ))
    }

    async fn authorized_request(&self, method: Method, path: &str) -> Result<RequestBuilder> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            method,
            path,
            connection.pid,
            &self.client_type,
        )
        .await
    }
}
