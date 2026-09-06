//! Feature-specific daemon member client.

use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::member::{
    ChooseDeviceGroupRequestDto, DeviceGroupChoiceResultDto, DeviceGroupChoicesDto,
    MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto, MemberSyncResultDto,
};

use crate::http::encode_path_segment;
use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const DEVICE_GROUP_CHOICES_PATH: &str = "/member/device-group-choices";

#[derive(Clone)]
pub struct DaemonMemberClient {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonMemberClient {
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

    pub async fn query_device_group_choices(&self) -> Result<DeviceGroupChoicesDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::GET,
            DEVICE_GROUP_CHOICES_PATH,
            |request| request,
        )
        .await?)
    }

    pub async fn choose_device_group(
        &self,
        request: &ChooseDeviceGroupRequestDto,
    ) -> Result<DeviceGroupChoiceResultDto> {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            Method::POST,
            DEVICE_GROUP_CHOICES_PATH,
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
        ChooseDeviceGroupRequestDto, DeviceGroupChoiceOutcomeDto, DeviceGroupRelationshipDto,
        DeviceMembershipDto, DeviceSyncRelationshipDto, MemberSyncPreferencesPatchDto,
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
    async fn device_group_choices_use_current_route_and_decode_opaque_options() {
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
            .and(path(DEVICE_GROUP_CHOICES_PATH))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "revision": 3,
                    "deviceTrust": {
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
                    "issues": [{
                        "issueId": "p:issue-1",
                        "choices": [{
                            "choiceId": "keep",
                            "isCurrentGroup": true,
                            "requiresRePairing": false,
                            "memberDeviceIds": ["device-a"],
                            "membersComplete": true
                        }]
                    }]
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
        let client = DaemonMemberClient::new(connection_state).unwrap();

        let choices = client
            .query_device_group_choices()
            .await
            .expect("device group choices request");
        let status = choices.device_trust;

        assert_eq!(choices.issues[0].issue_id, "p:issue-1");
        assert_eq!(choices.issues[0].choices[0].choice_id, "keep");
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
    async fn choose_device_group_posts_opaque_ids_and_query_revision() {
        let (server, client) = test_client().await;
        Mock::given(method("POST"))
            .and(path("/member/device-group-choices"))
            .and(header("authorization", "Session test-session"))
            .and(wiremock::matchers::body_json(serde_json::json!({
                "issueId": "p:issue-1",
                "choiceId": "apply",
                "expectedRevision": 7,
                "confirmLocalRemoval": false
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": {
                    "outcome": "completed",
                    "currentRevision": null
                },
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .choose_device_group(&ChooseDeviceGroupRequestDto {
                issue_id: "p:issue-1".to_string(),
                choice_id: "apply".to_string(),
                expected_revision: 7,
                confirm_local_removal: false,
            })
            .await
            .expect("device group choice");

        assert_eq!(result.outcome, DeviceGroupChoiceOutcomeDto::Completed);
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
        let client = DaemonMemberClient::new(connection_state).unwrap();
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
}
