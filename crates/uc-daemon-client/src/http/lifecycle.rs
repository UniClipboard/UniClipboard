use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::types::{DaemonResidency, RestartAccepted, RestartRequest};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::enveloped_request;
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
    /// restart_disabled / invalid_target) on `DaemonRequestError::Status` for the
    /// caller to branch on.
    pub async fn restart(&self, target_mode: DaemonResidency) -> Result<RestartAccepted> {
        let req_body = RestartRequest { target_mode };
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::LIFECYCLE_RESTART,
            |r| r.json(&req_body),
        )
        .await?)
    }
}
