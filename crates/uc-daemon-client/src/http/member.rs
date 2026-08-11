//! Feature-specific daemon member client.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::member::WorkspaceConvergenceDto;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const WORKSPACE_CONVERGENCE_PATH: &str = "/member/workspace-convergence";

#[derive(Clone)]
pub struct DaemonMemberClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonMemberClient {
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

    pub async fn workspace_convergence(&self) -> Result<WorkspaceConvergenceDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            WORKSPACE_CONVERGENCE_PATH,
            |request| request,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::member::WorkspaceConvergencePhaseDto;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn workspace_convergence_uses_current_route_and_decodes_envelope() {
        let server = MockServer::start().await;
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
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(WORKSPACE_CONVERGENCE_PATH))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "phase": "converging",
                    "revision": 3,
                    "changeCount": 2,
                    "removalIntentCount": 1,
                    "effectiveMemberCount": 3,
                    "confirmedMemberCount": 2,
                    "waitingMemberDeviceIds": ["device-b"],
                    "waitingMemberCount": 1,
                    "convergenceDigest": "digest-1",
                    "updatedAtMs": 42,
                    "removed": false,
                    "failureCategory": null
                },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let connection_state = DaemonConnectionState::default();
        connection_state.set(DaemonConnectionInfo {
            base_url: server.uri(),
            ws_url: "ws://127.0.0.1/unused".to_string(),
            token: "test-bearer".to_string(),
            pid: 42,
        });
        let client = DaemonMemberClient::new(connection_state);

        let status = client
            .workspace_convergence()
            .await
            .expect("workspace convergence request");

        assert_eq!(status.phase, WorkspaceConvergencePhaseDto::Converging);
        assert_eq!(status.confirmed_member_count, 2);
        assert_eq!(status.waiting_member_device_ids, vec!["device-b"]);
        assert_eq!(status.waiting_member_count, 1);
    }
}
