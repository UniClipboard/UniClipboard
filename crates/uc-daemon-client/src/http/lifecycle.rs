use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::types::{DaemonResidency, RestartAccepted, RestartRequest};
use uc_daemon_contract::constants::http_route;

use crate::http::enveloped::{empty_request, enveloped_request};
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
    pub fn new(connection_state: DaemonConnectionState) -> Result<Self> {
        Ok(Self {
            http: Arc::new(crate::build_local_http_client()?),
            connection_state,
            client_type: "gui".to_string(),
        })
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

    /// POST /lifecycle/graceful-stop — ask THIS daemon process to shut down in
    /// an orderly way, regardless of residency.
    ///
    /// Unlike [`Self::restart`], this is not an Oneshot-promotion request and
    /// never conflicts with one: the daemon marks its own run's crash-detection
    /// marker clean synchronously, before starting the shutdown sequence, so a
    /// caller that then force-kills the process on a timeout never produces a
    /// false "previous daemon run exited abnormally" log on the next boot.
    pub async fn graceful_stop(&self) -> Result<()> {
        Ok(empty_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            http_route::LIFECYCLE_GRACEFUL_STOP,
            |r| r,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn connected_client(server: &MockServer) -> DaemonLifecycleClient {
        Mock::given(method("POST"))
            .and(path("/auth/connect"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "sessionToken": "test-session",
                    "expiresInSecs": 300,
                    "refreshAtSecs": 240
                },
                "ts": 1
            })))
            .mount(server)
            .await;

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: server.uri(),
            ws_url: "ws://127.0.0.1/unused".to_string(),
            token: "test-bearer".to_string(),
            pid: 42,
        });
        DaemonLifecycleClient::new(connection_state).unwrap()
    }

    #[tokio::test]
    async fn graceful_stop_posts_to_the_dedicated_route_with_a_session_token() {
        let server = MockServer::start().await;
        let client = connected_client(&server).await;

        Mock::given(method("POST"))
            .and(path(http_route::LIFECYCLE_GRACEFUL_STOP))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(202))
            .expect(1)
            .mount(&server)
            .await;

        client
            .graceful_stop()
            .await
            .expect("graceful-stop request must succeed against a 202 response");
    }

    #[tokio::test]
    async fn graceful_stop_surfaces_a_non_2xx_response_as_an_error() {
        // A daemon that is already gone (or too confused to answer) must make
        // the caller fall back to a hard terminate rather than silently
        // believing the graceful path worked.
        let server = MockServer::start().await;
        let client = connected_client(&server).await;

        Mock::given(method("POST"))
            .and(path(http_route::LIFECYCLE_GRACEFUL_STOP))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;

        let error = client
            .graceful_stop()
            .await
            .expect_err("a 500 response must surface as an error, not a silent success");
        assert!(error.to_string().contains("500"));
    }
}
