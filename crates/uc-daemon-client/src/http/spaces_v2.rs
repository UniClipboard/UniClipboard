use std::sync::Arc;

use anyhow::Result;
use reqwest::Method;
use uc_daemon_contract::api::dto::v2::spaces::{
    CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
    SpaceProfileSummaryDto,
};

use crate::http::encode_path_segment;
use crate::http::enveloped::enveloped_request;
use crate::DaemonConnectionState;

const SPACES_PATH: &str = "/v2/spaces";
const SPACES_JOIN_PATH: &str = "/v2/spaces/join";
const SPACES_ACTIVE_SEND_PATH: &str = "/v2/spaces/active-send";

/// Typed loopback HTTP client for daemon multi-space profile routes.
#[derive(Clone)]
pub struct DaemonSpacesV2Client {
    http: Arc<reqwest::Client>,
    connection_state: DaemonConnectionState,
    client_type: String,
}

impl DaemonSpacesV2Client {
    pub fn new(connection_state: DaemonConnectionState) -> Self {
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

    /// `GET /v2/spaces` — list every persisted space profile.
    pub async fn list_spaces(&self) -> Result<Vec<SpaceProfileSummaryDto>> {
        self.enveloped(Method::GET, SPACES_PATH, |request| request)
            .await
    }

    /// `POST /v2/spaces` — create a new local space profile.
    pub async fn create_space(
        &self,
        request: &CreateSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto> {
        self.enveloped(Method::POST, SPACES_PATH, |builder| builder.json(request))
            .await
    }

    /// `POST /v2/spaces/join` — join an existing space as a new profile.
    pub async fn join_space(
        &self,
        request: &JoinSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto> {
        self.enveloped(Method::POST, SPACES_JOIN_PATH, |builder| {
            builder.json(request)
        })
        .await
    }

    /// `PUT /v2/spaces/active-send` — choose the profile for local sends.
    pub async fn set_active_send(
        &self,
        request: &SetActiveSendSpaceRequestDto,
    ) -> Result<SpaceProfileSummaryDto> {
        self.enveloped(Method::PUT, SPACES_ACTIVE_SEND_PATH, |builder| {
            builder.json(request)
        })
        .await
    }

    /// `DELETE /v2/spaces/{profileId}` — remove and return the profile summary.
    ///
    /// The success contract is always HTTP 200 with
    /// `ApiEnvelope<SpaceProfileSummaryDto>`; a 204 response is a decode error.
    pub async fn remove_space(&self, profile_id: &str) -> Result<SpaceProfileSummaryDto> {
        let profile_id = encode_path_segment(profile_id)?;
        let path = format!("{SPACES_PATH}/{profile_id}");
        self.enveloped(Method::DELETE, &path, |request| request)
            .await
    }

    async fn enveloped<T>(
        &self,
        method: Method,
        path: &str,
        customize: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    ) -> Result<T>
    where
        T: serde::de::DeserializeOwned,
    {
        Ok(enveloped_request(
            &self.http,
            &self.connection_state,
            &self.client_type,
            method,
            path,
            customize,
        )
        .await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;

    use crate::DaemonRequestError;
    use uc_daemon_contract::api::auth::DaemonConnectionInfo;
    use uc_daemon_contract::api::dto::v2::spaces::{
        CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
        SpaceRuntimeStateDto,
    };
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn list_spaces_gets_v2_route_and_decodes_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("GET"))
            .and(path("/v2/spaces"))
            .and(header("authorization", "Session test-session"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [summary_json("profile-a", true)],
                "ts": 2
            })))
            .expect(1)
            .mount(&server)
            .await;

        let spaces = client.list_spaces().await.expect("list spaces");

        assert_eq!(spaces.len(), 1);
        assert_eq!(spaces[0].profile_id, "profile-a");
        assert!(spaces[0].is_active_send);
        assert_eq!(spaces[0].runtime_state, SpaceRuntimeStateDto::Running);
    }

    #[tokio::test]
    async fn create_space_posts_typed_body_and_decodes_summary_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("POST"))
            .and(path("/v2/spaces"))
            .and(header("authorization", "Session test-session"))
            .and(body_json(serde_json::json!({
                "passphrase": "correct horse battery staple",
                "passphraseConfirm": "correct horse battery staple",
                "deviceName": "Office PC"
            })))
            .respond_with(summary_envelope("profile-created", false))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .create_space(&CreateSpaceProfileRequestDto {
                passphrase: "correct horse battery staple".to_string(),
                passphrase_confirm: "correct horse battery staple".to_string(),
                device_name: Some("Office PC".to_string()),
            })
            .await
            .expect("create space");

        assert_eq!(result.profile_id, "profile-created");
    }

    #[tokio::test]
    async fn join_space_posts_typed_body_and_decodes_summary_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("POST"))
            .and(path("/v2/spaces/join"))
            .and(header("authorization", "Session test-session"))
            .and(body_json(serde_json::json!({
                "code": "ABCD-1234",
                "passphrase": "correct horse battery staple",
                "deviceName": null
            })))
            .respond_with(summary_envelope("profile-joined", false))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .join_space(&JoinSpaceProfileRequestDto {
                code: "ABCD-1234".to_string(),
                passphrase: "correct horse battery staple".to_string(),
                device_name: None,
            })
            .await
            .expect("join space");

        assert_eq!(result.profile_id, "profile-joined");
    }

    #[tokio::test]
    async fn set_active_send_puts_typed_body_and_decodes_summary_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("PUT"))
            .and(path("/v2/spaces/active-send"))
            .and(header("authorization", "Session test-session"))
            .and(body_json(serde_json::json!({ "profileId": "profile-b" })))
            .respond_with(summary_envelope("profile-b", true))
            .expect(1)
            .mount(&server)
            .await;

        let result = client
            .set_active_send(&SetActiveSendSpaceRequestDto {
                profile_id: "profile-b".to_string(),
            })
            .await
            .expect("set active send");

        assert_eq!(result.profile_id, "profile-b");
        assert!(result.is_active_send);
    }

    #[tokio::test]
    async fn remove_space_deletes_encoded_profile_route_and_requires_summary_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("DELETE"))
            .and(path("/v2/spaces/profile%2Fa%3Fmode%3Dunsafe"))
            .and(header("authorization", "Session test-session"))
            .respond_with(summary_envelope("profile/a?mode=unsafe", false))
            .expect(1)
            .mount(&server)
            .await;

        let removed = client
            .remove_space("profile/a?mode=unsafe")
            .await
            .expect("remove space");

        assert_eq!(removed.profile_id, "profile/a?mode=unsafe");
    }

    #[tokio::test]
    async fn remove_space_rejects_dot_segments_before_sending() {
        let (_server, client) = test_client().await;

        let error = client
            .remove_space("..")
            .await
            .expect_err("reject dot path");

        assert!(error.to_string().contains("cannot be `.` or `..`"));
    }

    #[tokio::test]
    async fn remove_space_rejects_204_because_delete_contract_requires_summary_envelope() {
        let (server, client) = test_client().await;
        Mock::given(method("DELETE"))
            .and(path("/v2/spaces/profile-a"))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .remove_space("profile-a")
            .await
            .expect_err("204 must not satisfy the 200 summary contract");
        let request_error = error
            .downcast_ref::<DaemonRequestError>()
            .expect("shared daemon decode error");

        assert!(
            matches!(request_error, DaemonRequestError::Decode { path, .. } if path == "/v2/spaces/profile-a")
        );
    }

    #[tokio::test]
    async fn daemon_error_uses_shared_status_code_and_message() {
        let (server, client) = test_client().await;
        Mock::given(method("POST"))
            .and(path("/v2/spaces/join"))
            .respond_with(ResponseTemplate::new(409).set_body_json(serde_json::json!({
                "code": "space_already_joined",
                "message": "this device already belongs to the space"
            })))
            .expect(1)
            .mount(&server)
            .await;

        let error = client
            .join_space(&JoinSpaceProfileRequestDto {
                code: "ABCD-1234".to_string(),
                passphrase: "correct horse battery staple".to_string(),
                device_name: None,
            })
            .await
            .expect_err("join conflict");
        let request_error = error
            .downcast_ref::<DaemonRequestError>()
            .expect("shared daemon request error");

        assert_eq!(request_error.status(), Some(reqwest::StatusCode::CONFLICT));
        assert_eq!(request_error.code(), Some("space_already_joined"));
        assert_eq!(
            request_error.message(),
            Some("this device already belongs to the space")
        );
    }

    async fn test_client() -> (MockServer, DaemonSpacesV2Client) {
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
        let client = DaemonSpacesV2Client::with_http_conn_state_and_type(
            Arc::new(reqwest::Client::new()),
            connection_state,
            "test".to_string(),
        );
        (server, client)
    }

    fn summary_envelope(profile_id: &str, active: bool) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": summary_json(profile_id, active),
            "ts": 2
        }))
    }

    fn summary_json(profile_id: &str, active: bool) -> serde_json::Value {
        serde_json::json!({
            "profileId": profile_id,
            "spaceId": "space-a",
            "displayName": "Work",
            "deviceName": "Office PC",
            "runtimeState": { "state": "running" },
            "incomingSyncState": { "state": "enabled" },
            "lastFault": null,
            "isActiveSend": active
        })
    }
}
