//! Feature-specific daemon member client.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::member::{
    MembershipConvergenceDto, SharedDeviceRefreshDto, SharedDeviceRefreshStartedDto,
};

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const MEMBERSHIP_CONVERGENCE_PATH: &str = "/member/convergence";
const SHARED_DEVICE_REFRESH_PATH: &str = "/member/shared-device-refresh";

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

    pub async fn convergence(&self) -> Result<MembershipConvergenceDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            MEMBERSHIP_CONVERGENCE_PATH,
            |request| request,
        )
        .await?)
    }

    pub async fn start_shared_device_refresh(&self) -> Result<SharedDeviceRefreshStartedDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            SHARED_DEVICE_REFRESH_PATH,
            |request| request,
        )
        .await?)
    }

    pub async fn shared_device_refresh(&self, request_id: &str) -> Result<SharedDeviceRefreshDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &format!("{SHARED_DEVICE_REFRESH_PATH}/{request_id}"),
            |request| request,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::member::{
        MembershipConvergenceStateDto, SharedDeviceRefreshDeviceStateDto,
        SharedDeviceRefreshPhaseDto,
    };
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn convergence_uses_member_route_and_decodes_envelope() {
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
            .and(path(MEMBERSHIP_CONVERGENCE_PATH))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "state": "converging" },
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

        let status = client.convergence().await.expect("convergence request");

        assert_eq!(status.state, MembershipConvergenceStateDto::Converging);
    }

    #[tokio::test]
    async fn shared_device_refresh_routes_through_member_endpoints() {
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
        Mock::given(method("POST"))
            .and(path(SHARED_DEVICE_REFRESH_PATH))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "requestId": "refresh-1" },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(format!("{SHARED_DEVICE_REFRESH_PATH}/refresh-1")))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "requestId": "refresh-1",
                    "phase": "connecting",
                    "devices": [
                        {
                            "deviceId": "peer-1",
                            "displayName": "Windows workstation",
                            "state": "connecting"
                        }
                    ],
                    "totalCount": 1,
                    "discoveredCount": 0,
                    "connectingCount": 1,
                    "connectedCount": 0,
                    "alreadyPresentCount": 0,
                    "waitingForPeerCount": 0,
                    "waitingForUpdateCount": 0,
                    "versionIncompatibleCount": 0,
                    "rejectedCount": 0,
                    "unavailableSourceCount": 0
                },
                "ts": 3
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

        let started = client
            .start_shared_device_refresh()
            .await
            .expect("start shared device refresh");
        assert_eq!(started.request_id, "refresh-1");

        let snapshot = client
            .shared_device_refresh(&started.request_id)
            .await
            .expect("query shared device refresh");
        assert_eq!(snapshot.phase, SharedDeviceRefreshPhaseDto::Connecting);
        assert_eq!(snapshot.devices.len(), 1);
        assert_eq!(
            snapshot.devices[0].state,
            SharedDeviceRefreshDeviceStateDto::Connecting
        );
    }
}
