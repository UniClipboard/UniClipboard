//! Feature-specific daemon member client.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::member::{
    DecideDeviceTrustRequestDto, DeviceTrustDecisionDto, DeviceTrustSnapshotDto,
    MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto, MemberSyncResultDto,
    WorkspaceConvergenceDto,
};

use crate::http::encode_path_segment;
use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const DEVICE_TRUST_PATH: &str = "/member/device-trust";
const DEVICE_TRUST_DECISION_PATH: &str = "/member/device-trust/decision";
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

    pub async fn decide_device_trust(
        &self,
        request: &DecideDeviceTrustRequestDto,
    ) -> Result<DeviceTrustDecisionDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            DEVICE_TRUST_DECISION_PATH,
            |http_request| http_request.json(request),
        )
        .await?)
    }

    pub async fn member_sync_preferences(
        &self,
        device_id: &str,
    ) -> Result<MemberSyncPreferencesDto> {
        let path = member_sync_preferences_path(device_id)?;
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            &path,
            |request| request,
        )
        .await?)
    }

    pub async fn update_member_sync_preferences(
        &self,
        device_id: &str,
        patch: &MemberSyncPreferencesPatchDto,
    ) -> Result<MemberSyncResultDto> {
        let path = member_sync_preferences_path(device_id)?;
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::PATCH,
            &path,
            |request| request.json(patch),
        )
        .await?)
    }
}

fn member_sync_preferences_path(device_id: &str) -> Result<String> {
    let device_id = encode_path_segment(device_id)?;
    Ok(format!("/member/{device_id}/sync-preferences"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::member::{
        DecideDeviceTrustRequestDto, DeviceGroupRelationshipDto, DeviceMembershipDto,
        DeviceSyncRelationshipDto, DeviceTrustChoiceDto, DeviceTrustDecisionDto,
        MemberSyncPreferencesPatchDto,
    };
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn member_sync_path_encodes_dynamic_device_id() {
        assert_eq!(
            member_sync_preferences_path("device/a?mode=unsafe").expect("encoded member path"),
            "/member/device%2Fa%3Fmode%3Dunsafe/sync-preferences"
        );
    }

    #[test]
    fn member_sync_path_rejects_dot_segments() {
        assert!(member_sync_preferences_path(".").is_err());
        assert!(member_sync_preferences_path("..").is_err());
    }

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

    #[tokio::test]
    async fn decide_device_trust_posts_bound_change_and_decodes_result() {
        let (server, client) = test_client().await;
        Mock::given(method("POST"))
            .and(path("/member/device-trust/decision"))
            .and(header("authorization", "Session test-session"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "changeId": "change-1",
                "choice": "apply_change",
                "confirmLocalRemoval": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "kind": "applied",
                    "changeId": "change-1",
                    "snapshot": empty_snapshot_json()
                },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .decide_device_trust(&DecideDeviceTrustRequestDto {
                change_id: "change-1".to_string(),
                choice: DeviceTrustChoiceDto::ApplyChange,
                confirm_local_removal: false,
            })
            .await
            .expect("device trust decision");

        assert!(matches!(result, DeviceTrustDecisionDto::Applied { .. }));
    }

    #[tokio::test]
    async fn member_sync_preferences_get_uses_device_route() {
        let (server, client) = test_client().await;
        Mock::given(method("GET"))
            .and(path("/member/device-a/sync-preferences"))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "sendEnabled": true,
                    "receiveEnabled": false,
                    "sendContentTypes": content_types_json(true),
                    "receiveContentTypes": content_types_json(false)
                },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let preferences = client
            .member_sync_preferences("device-a")
            .await
            .expect("member sync preferences");

        assert!(preferences.send_enabled);
        assert!(!preferences.receive_enabled);
    }

    #[tokio::test]
    async fn update_member_sync_preferences_patches_only_supplied_fields() {
        let (server, client) = test_client().await;
        Mock::given(method("PATCH"))
            .and(path("/member/device-a/sync-preferences"))
            .and(header("authorization", "Session test-session"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "sendEnabled": false,
                "receiveEnabled": null,
                "sendContentTypes": null,
                "receiveContentTypes": null
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": { "success": true },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .update_member_sync_preferences(
                "device-a",
                &MemberSyncPreferencesPatchDto {
                    send_enabled: Some(false),
                    receive_enabled: None,
                    send_content_types: None,
                    receive_content_types: None,
                },
            )
            .await
            .expect("update member sync preferences");

        assert!(result.success);
    }

    async fn test_client() -> (MockServer, DaemonMemberClient) {
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
        (server, client)
    }

    fn content_types_json(enabled: bool) -> serde_json::Value {
        serde_json::json!({
            "text": enabled,
            "image": enabled,
            "link": enabled,
            "file": enabled,
            "codeSnippet": enabled,
            "richText": enabled
        })
    }

    fn empty_snapshot_json() -> serde_json::Value {
        serde_json::json!({
            "revision": 1,
            "localDeviceId": "device-a",
            "localMembership": "active",
            "currentChange": null,
            "currentJoin": null,
            "pendingInboundMember": null,
            "devices": [],
            "recovery": "not_available_in_this_version",
            "allowedActions": [],
            "blockedReason": null,
            "updatedAtMs": 1
        })
    }
}
