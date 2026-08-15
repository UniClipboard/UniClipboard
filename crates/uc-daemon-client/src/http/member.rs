//! Feature-specific daemon member client.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::member::{DeviceTrustSnapshotDto, WorkspaceConvergenceDto};

use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const DEVICE_TRUST_PATH: &str = "/member/device-trust";
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

    pub async fn device_trust(&self) -> Result<DeviceTrustSnapshotDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            DEVICE_TRUST_PATH,
            |request| request,
        )
        .await?)
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
    use uc_daemon_contract::api::dto::member::{
        DeviceGroupRelationshipDto, DeviceMembershipDto, DeviceSyncRelationshipDto,
    };
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn device_trust_uses_current_route_and_decodes_envelope() {
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
            .and(path(DEVICE_TRUST_PATH))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "revision": 3,
                    "localDeviceId": "device-a",
                    "localMembership": "active",
                    "currentChange": null,
                    "devices": [{
                        "deviceId": "device-a",
                        "displayName": "A",
                        "isLocal": true,
                        "reachability": "online",
                        "membership": "active",
                        "groupRelationship": "consistent",
                        "compatibility": "compatible",
                        "syncRelationship": "usable",
                        "availableActions": [],
                        "blockedReason": null
                    }],
                    "recovery": "not_available_in_this_version",
                    "allowedActions": [],
                    "blockedReason": null,
                    "updatedAtMs": 42
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

        let status = client.device_trust().await.expect("device trust request");

        assert_eq!(status.local_device_id, "device-a");
        assert_eq!(status.local_membership, DeviceMembershipDto::Active);
        assert_eq!(
            status.devices[0].group_relationship,
            DeviceGroupRelationshipDto::Consistent
        );
        assert_eq!(
            status.devices[0].sync_relationship,
            DeviceSyncRelationshipDto::Usable
        );
    }
}
