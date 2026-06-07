use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use reqwest::Method;
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_daemon_contract::api::types::{DaemonResidency, RestartAccepted, RestartRequest};
use uc_daemon_contract::constants::http_route;

use crate::http::authorized_daemon_request_with_type;
use crate::DaemonConnectionState;

/// Loopback HTTP client for the daemon's `/lifecycle/*` control endpoints.
///
/// ADR-008 P5-L L8d-1: surfaces `POST /lifecycle/restart` as a typed native
/// client method so a persistent client can request a controlled
/// restart/promotion of a transient (Oneshot) daemon. Production-neutral as of
/// L8d-1 — nothing calls this yet; the promotion orchestration lands in L8d-2.
#[derive(Clone)]
pub struct DaemonLifecycleClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonLifecycleClient {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
        Self {
            http: Arc::new(reqwest::Client::new()),
            connection_state,
            client_type: "gui".to_string(),
        }
    }

    pub(crate) fn with_http_conn_state_and_type(
        http: Arc<reqwest::Client>,
        connection_state: DaemonConnectionState,
        client_type: String,
    ) -> Self {
        Self {
            http,
            connection_state,
            client_type,
        }
    }

    /// POST /lifecycle/restart — request a controlled restart/promotion
    /// (ADR-008 P5-L). Returns the accepted {generation, targetMode}. The daemon
    /// raises quiescing + drains + self-terminates; the requester then spawns the
    /// target. Errors carry a stable `code` (restart_in_progress / not_promotable /
    /// restart_disabled / invalid_target) for the caller to branch on.
    pub async fn restart(&self, target_mode: DaemonResidency) -> Result<RestartAccepted> {
        let connection = self
            .connection_state
            .get()
            .ok_or_else(|| anyhow!("daemon connection info is not available"))?;
        let req_body = RestartRequest { target_mode };
        let request = authorized_daemon_request_with_type(
            &self.http,
            &self.connection_state,
            Method::POST,
            http_route::LIFECYCLE_RESTART,
            connection.pid,
            &self.client_type,
        )
        .await?;

        let response = request
            .json(&req_body)
            .send()
            .await
            .context("failed to call daemon lifecycle restart")?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("lifecycle restart failed ({}): {}", status, body);
        }

        // Wire shape (ADR-008 P5-L L8d-1): `{ data: RestartAccepted, ts }`.
        let envelope = response
            .json::<ApiEnvelope<RestartAccepted>>()
            .await
            .context("failed to decode lifecycle restart response")?;
        Ok(envelope.data)
    }
}
