use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;
use uc_daemon_contract::api::dto::v2::setup::{
    CancelJoinSpaceRequest, InitializeSpaceRequest, InitializeSpaceResponse,
    IssueInvitationResponse, JoinSpaceResponse, RedeemRequest, SetupStateResponse,
    SwitchSpaceRequest,
};
use uc_daemon_contract::constants::http_route_v2;

#[derive(Clone)]
pub struct DaemonSetupV2Client {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSetupV2Client {
    pub fn with_conn_state(connection_state: DaemonConnectionState) -> Self {
        Self::with_http_conn_state_and_type(
            Arc::new(reqwest::Client::new()),
            connection_state,
            "gui".to_string(),
        )
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

    pub async fn issue_invitation(&self) -> Result<IssueInvitationResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/issue-invitation",
            |r| r,
        )
        .await?)
    }

    pub async fn get_state(&self) -> Result<SetupStateResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            http_route_v2::SETUP_STATE,
            |r| r,
        )
        .await?)
    }

    pub async fn initialize_space(
        &self,
        req: &InitializeSpaceRequest,
    ) -> Result<InitializeSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/initialize",
            |r| r.json(req),
        )
        .await?)
    }

    pub async fn redeem_invitation(&self, req: &RedeemRequest) -> Result<JoinSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/redeem",
            |r| r.json(req),
        )
        .await?)
    }

    pub async fn switch_space(&self, req: &SwitchSpaceRequest) -> Result<JoinSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/switch-space",
            |r| r.json(req),
        )
        .await?)
    }

    pub async fn cancel_join(&self, join_id: &str) -> Result<JoinSpaceResponse> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            "/v2/setup/cancel-join",
            |request| {
                request.json(&CancelJoinSpaceRequest {
                    join_id: join_id.to_string(),
                })
            },
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

    #[tokio::test]
    async fn get_state_uses_v2_route_and_decodes_envelope() {
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
            .and(path("/v2/setup/state"))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "hasCompleted": true,
                    "currentInvitation": null,
                    "deviceName": "Test Mac"
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
        let client = DaemonSetupV2Client::with_http_conn_state_and_type(
            Arc::new(reqwest::Client::new()),
            connection_state,
            "test".to_string(),
        );

        let state = client.get_state().await.expect("setup state request");

        assert!(state.has_completed);
        assert_eq!(state.device_name.as_deref(), Some("Test Mac"));
    }

    #[tokio::test]
    async fn cancel_join_posts_join_id_and_decodes_current_status() {
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
            .and(path("/v2/setup/cancel-join"))
            .and(header("authorization", "Session test-session"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "joinId": "join-1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "status": "rejected",
                    "joinId": "join-1",
                    "reason": "cancelled"
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
        let client = DaemonSetupV2Client::with_http_conn_state_and_type(
            Arc::new(reqwest::Client::new()),
            connection_state,
            "test".to_string(),
        );

        let status = client.cancel_join("join-1").await.expect("cancel join");

        assert!(matches!(
            status,
            JoinSpaceResponse::Rejected {
                reason:
                    uc_daemon_contract::api::dto::v2::setup::JoinSpaceRejectionReason::Cancelled,
                ..
            }
        ));
    }
}
