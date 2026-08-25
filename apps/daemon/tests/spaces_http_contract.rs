#![cfg(target_os = "windows")]

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use serde_json::{json, Value};
use uc_daemon::daemon::spaces_http::{
    SpacesBackendError, SpacesHttpBackend, SpacesHttpMethod, SpacesHttpRequest, SpacesHttpService,
};
use uc_daemon_contract::api::dto::v2::spaces::{
    CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
    SpaceIncomingSyncStateDto, SpaceProfileSummaryDto, SpaceRuntimeStateDto,
};

const PROFILE_A: &str = "11111111-1111-4111-8111-111111111111";
const PROFILE_B: &str = "22222222-2222-4222-8222-222222222222";

#[derive(Debug, Clone, PartialEq, Eq)]
enum BackendCall {
    List,
    Create(CreateSpaceProfileRequestDto),
    Join(JoinSpaceProfileRequestDto),
    SetActiveSend(SetActiveSendSpaceRequestDto),
    Remove(String),
}

#[derive(Default)]
struct RecordingBackend {
    calls: Mutex<Vec<BackendCall>>,
    failure: Mutex<Option<SpacesBackendError>>,
}

impl RecordingBackend {
    fn failing(error: SpacesBackendError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            failure: Mutex::new(Some(error)),
        }
    }

    fn calls(&self) -> Vec<BackendCall> {
        self.calls.lock().expect("calls lock").clone()
    }

    fn record(&self, call: BackendCall) -> Result<(), SpacesBackendError> {
        self.calls.lock().expect("calls lock").push(call);
        match self.failure.lock().expect("failure lock").clone() {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

#[async_trait]
impl SpacesHttpBackend for RecordingBackend {
    async fn list_spaces(&self) -> Result<Vec<SpaceProfileSummaryDto>, SpacesBackendError> {
        self.record(BackendCall::List)?;
        Ok(vec![summary(PROFILE_A, true)])
    }

    async fn create_space(
        &self,
        request: CreateSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        self.record(BackendCall::Create(request))?;
        Ok(summary(PROFILE_B, false))
    }

    async fn join_space(
        &self,
        request: JoinSpaceProfileRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        self.record(BackendCall::Join(request))?;
        Ok(summary(PROFILE_B, false))
    }

    async fn set_active_send(
        &self,
        request: SetActiveSendSpaceRequestDto,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        self.record(BackendCall::SetActiveSend(request.clone()))?;
        Ok(summary(&request.profile_id, true))
    }

    async fn remove_space(
        &self,
        profile_id: String,
    ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
        self.record(BackendCall::Remove(profile_id.clone()))?;
        Ok(summary(&profile_id, false))
    }
}

#[tokio::test]
async fn get_spaces_lists_profiles_in_the_success_envelope() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let response = service
        .handle(SpacesHttpRequest::new(SpacesHttpMethod::Get, "/v2/spaces"))
        .await;

    assert_eq!(response.status, 200);
    assert_success(&response.body);
    assert_eq!(response.body["data"][0]["profileId"], PROFILE_A);
    assert_eq!(backend.calls(), vec![BackendCall::List]);
}

#[tokio::test]
async fn post_spaces_decodes_the_create_dto_and_returns_the_created_summary() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let create_body = json!({
        "passphrase": "correct horse battery staple",
        "passphraseConfirm": "correct horse battery staple",
        "deviceName": "Office PC"
    });
    let response = service
        .handle(SpacesHttpRequest::json(
            SpacesHttpMethod::Post,
            "/v2/spaces",
            &create_body,
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_success(&response.body);
    assert_eq!(response.body["data"]["profileId"], PROFILE_B);
    assert_eq!(
        backend.calls(),
        vec![BackendCall::Create(CreateSpaceProfileRequestDto {
            passphrase: "correct horse battery staple".into(),
            passphrase_confirm: "correct horse battery staple".into(),
            device_name: Some("Office PC".into()),
        })]
    );
}

#[tokio::test]
async fn post_spaces_join_decodes_the_join_dto_and_returns_the_joined_summary() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let join_body = json!({
        "code": "ABCD-1234",
        "passphrase": "correct horse battery staple",
        "deviceName": null
    });
    let response = service
        .handle(SpacesHttpRequest::json(
            SpacesHttpMethod::Post,
            "/v2/spaces/join",
            &join_body,
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_success(&response.body);
    assert_eq!(response.body["data"]["profileId"], PROFILE_B);
    assert_eq!(
        backend.calls(),
        vec![BackendCall::Join(JoinSpaceProfileRequestDto {
            code: "ABCD-1234".into(),
            passphrase: "correct horse battery staple".into(),
            device_name: None,
        })]
    );
}

#[tokio::test]
async fn put_active_send_decodes_the_target_and_returns_the_updated_summary() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let active_body = json!({ "profileId": PROFILE_B });
    let response = service
        .handle(SpacesHttpRequest::json(
            SpacesHttpMethod::Put,
            "/v2/spaces/active-send",
            &active_body,
        ))
        .await;

    assert_eq!(response.status, 200);
    assert_success(&response.body);
    assert_eq!(response.body["data"]["isActiveSend"], true);
    assert_eq!(
        backend.calls(),
        vec![BackendCall::SetActiveSend(SetActiveSendSpaceRequestDto {
            profile_id: PROFILE_B.into(),
        })]
    );
}

#[tokio::test]
async fn delete_profile_returns_exactly_200_with_the_removed_summary() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let response = service
        .handle(SpacesHttpRequest::new(
            SpacesHttpMethod::Delete,
            format!("/v2/spaces/{PROFILE_B}"),
        ))
        .await;

    assert_eq!(response.status, 200, "DELETE success must be exactly 200");
    assert_success(&response.body);
    assert_eq!(response.body["data"]["profileId"], PROFILE_B);
    assert_eq!(backend.calls(), vec![BackendCall::Remove(PROFILE_B.into())]);
}

#[tokio::test]
async fn malformed_json_and_unknown_fields_return_canonical_bad_request_without_backend_calls() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let malformed = service
        .handle(SpacesHttpRequest::with_body(
            SpacesHttpMethod::Post,
            "/v2/spaces",
            br#"{"passphrase":"x""#.to_vec(),
        ))
        .await;
    assert_error(&malformed.body, "bad_request");
    assert_eq!(malformed.status, 400);

    let unknown_field = service
        .handle(SpacesHttpRequest::json(
            SpacesHttpMethod::Put,
            "/v2/spaces/active-send",
            &json!({ "profileId": PROFILE_A, "profile_id": PROFILE_B }),
        ))
        .await;
    assert_error(&unknown_field.body, "bad_request");
    assert_eq!(unknown_field.status, 400);
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn wrong_methods_and_unknown_paths_have_canonical_405_and_404_responses() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    let wrong_method = service
        .handle(SpacesHttpRequest::new(
            SpacesHttpMethod::Post,
            "/v2/spaces/active-send",
        ))
        .await;
    assert_eq!(wrong_method.status, 405);
    assert_error(&wrong_method.body, "method_not_allowed");

    let unknown = service
        .handle(SpacesHttpRequest::new(
            SpacesHttpMethod::Get,
            "/v2/spaces/unknown/extra",
        ))
        .await;
    assert_eq!(unknown.status, 404);
    assert_error(&unknown.body, "not_found");
    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn delete_rejects_unsafe_or_ambiguous_profile_path_segments_before_backend_access() {
    let backend = Arc::new(RecordingBackend::default());
    let service = SpacesHttpService::new(backend.clone());

    for path in [
        "/v2/spaces/..",
        "/v2/spaces/%2e%2e",
        "/v2/spaces/profile%2Fchild",
        "/v2/spaces/profile\\child",
        "/v2/spaces/profile/child",
    ] {
        let response = service
            .handle(SpacesHttpRequest::new(SpacesHttpMethod::Delete, path))
            .await;
        assert!(
            response.status == 400 || response.status == 404,
            "unsafe path {path} returned {}",
            response.status
        );
        assert!(response.body.get("code").is_some());
    }

    assert!(backend.calls().is_empty());
}

#[tokio::test]
async fn backend_error_categories_map_to_stable_status_and_canonical_error_envelopes() {
    let cases = [
        (
            SpacesBackendError::bad_request("invalid_profile_id", "profile ID is invalid"),
            400,
            "invalid_profile_id",
        ),
        (
            SpacesBackendError::not_found("space_not_found", "space profile was not found"),
            404,
            "space_not_found",
        ),
        (
            SpacesBackendError::conflict(
                "space_already_joined",
                "this device already belongs to the space",
            ),
            409,
            "space_already_joined",
        ),
        (
            SpacesBackendError::runtime_unavailable(
                "space_runtime_unavailable",
                "space runtime is unavailable",
            ),
            503,
            "space_runtime_unavailable",
        ),
    ];

    for (error, expected_status, expected_code) in cases {
        let backend = Arc::new(RecordingBackend::failing(error));
        let response = SpacesHttpService::new(backend)
            .handle(SpacesHttpRequest::new(SpacesHttpMethod::Get, "/v2/spaces"))
            .await;
        assert_eq!(response.status, expected_status);
        assert_error(&response.body, expected_code);
    }
}

#[tokio::test]
async fn internal_backend_failures_do_not_leak_sensitive_details() {
    let backend = Arc::new(RecordingBackend::failing(SpacesBackendError::internal(
        "catalog write failed at C:\\Users\\alice\\secret",
    )));
    let response = SpacesHttpService::new(backend)
        .handle(SpacesHttpRequest::new(SpacesHttpMethod::Get, "/v2/spaces"))
        .await;

    assert_eq!(response.status, 500);
    assert_eq!(response.body["code"], "internal_error");
    assert_eq!(response.body["message"], "space operation failed");
    assert!(!response.body.to_string().contains("alice"));
}

fn summary(profile_id: &str, active: bool) -> SpaceProfileSummaryDto {
    SpaceProfileSummaryDto {
        profile_id: profile_id.to_string(),
        space_id: Some("space-a".to_string()),
        display_name: Some("Work".to_string()),
        device_name: Some("Office PC".to_string()),
        runtime_state: SpaceRuntimeStateDto::Running,
        incoming_sync_state: SpaceIncomingSyncStateDto::Enabled,
        last_fault: None,
        is_active_send: active,
    }
}

fn assert_success(body: &Value) {
    assert!(body.get("data").is_some());
    assert!(body["ts"].as_i64().is_some());
    assert!(body.get("code").is_none());
}

fn assert_error(body: &Value, expected_code: &str) {
    assert_eq!(body["code"], expected_code);
    assert!(body["message"].as_str().is_some());
    assert!(body.get("data").is_none());
    assert!(body.get("ts").is_none());
}
