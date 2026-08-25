//! Axum adapter for the Windows multi-space HTTP service.

use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};

use super::spaces_http::{
    SpacesHttpMethod, SpacesHttpRequest, SpacesHttpResponse, SpacesHttpService,
};

#[derive(Clone)]
struct SpacesAxumState {
    service: SpacesHttpService,
}

/// Build routes whose missing outer state can be selected by the caller.
/// Production chooses `DaemonApiState`, allowing the webserver to apply the
/// same L2 auth/rate-limit middleware as every other protected endpoint.
pub fn router<S>(service: SpacesHttpService) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/v2/spaces", get(list_spaces).post(create_space))
        .route("/v2/spaces/join", post(join_space))
        .route("/v2/spaces/active-send", put(set_active_send))
        .route("/v2/spaces/:profile_id", delete(remove_space))
        .with_state::<S>(SpacesAxumState { service })
}

async fn list_spaces(State(state): State<SpacesAxumState>) -> impl IntoResponse {
    respond(
        state
            .service
            .handle(SpacesHttpRequest::new(SpacesHttpMethod::Get, "/v2/spaces"))
            .await,
    )
}

async fn create_space(State(state): State<SpacesAxumState>, body: Bytes) -> impl IntoResponse {
    respond(
        state
            .service
            .handle(SpacesHttpRequest::with_body(
                SpacesHttpMethod::Post,
                "/v2/spaces",
                body.to_vec(),
            ))
            .await,
    )
}

async fn join_space(State(state): State<SpacesAxumState>, body: Bytes) -> impl IntoResponse {
    respond(
        state
            .service
            .handle(SpacesHttpRequest::with_body(
                SpacesHttpMethod::Post,
                "/v2/spaces/join",
                body.to_vec(),
            ))
            .await,
    )
}

async fn set_active_send(State(state): State<SpacesAxumState>, body: Bytes) -> impl IntoResponse {
    respond(
        state
            .service
            .handle(SpacesHttpRequest::with_body(
                SpacesHttpMethod::Put,
                "/v2/spaces/active-send",
                body.to_vec(),
            ))
            .await,
    )
}

async fn remove_space(
    State(state): State<SpacesAxumState>,
    Path(profile_id): Path<String>,
) -> impl IntoResponse {
    respond(
        state
            .service
            .handle(SpacesHttpRequest::new(
                SpacesHttpMethod::Delete,
                format!("/v2/spaces/{profile_id}"),
            ))
            .await,
    )
}

fn respond(response: SpacesHttpResponse) -> impl IntoResponse {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    (status, Json(response.body))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use async_trait::async_trait;
    use axum::body::{to_bytes, Body};
    use axum::http::{Method, Request};
    use tower::ServiceExt;
    use uc_daemon_contract::api::dto::v2::spaces::{
        CreateSpaceProfileRequestDto, JoinSpaceProfileRequestDto, SetActiveSendSpaceRequestDto,
        SpaceIncomingSyncStateDto, SpaceProfileSummaryDto, SpaceRuntimeStateDto,
    };

    use super::*;
    use crate::daemon::spaces_http::{SpacesBackendError, SpacesHttpBackend};

    struct Backend;

    fn summary(profile_id: &str) -> SpaceProfileSummaryDto {
        SpaceProfileSummaryDto {
            profile_id: profile_id.into(),
            space_id: Some("space-1".into()),
            display_name: None,
            device_name: Some("windows".into()),
            runtime_state: SpaceRuntimeStateDto::Running,
            incoming_sync_state: SpaceIncomingSyncStateDto::Enabled,
            last_fault: None,
            is_active_send: true,
        }
    }

    #[async_trait]
    impl SpacesHttpBackend for Backend {
        async fn list_spaces(&self) -> Result<Vec<SpaceProfileSummaryDto>, SpacesBackendError> {
            Ok(vec![summary("profile-a")])
        }

        async fn create_space(
            &self,
            _request: CreateSpaceProfileRequestDto,
        ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
            Ok(summary("created"))
        }

        async fn join_space(
            &self,
            _request: JoinSpaceProfileRequestDto,
        ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
            Ok(summary("joined"))
        }

        async fn set_active_send(
            &self,
            request: SetActiveSendSpaceRequestDto,
        ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
            Ok(summary(&request.profile_id))
        }

        async fn remove_space(
            &self,
            profile_id: String,
        ) -> Result<SpaceProfileSummaryDto, SpacesBackendError> {
            Ok(summary(&profile_id))
        }
    }

    #[tokio::test]
    async fn adapter_forwards_method_body_and_status() {
        let app = router::<()>(SpacesHttpService::new(Arc::new(Backend)));
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri("/v2/spaces/active-send")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"profileId":"profile-b"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["profileId"], "profile-b");
    }

    #[tokio::test]
    async fn adapter_preserves_delete_exact_200_contract() {
        let app = router::<()>(SpacesHttpService::new(Arc::new(Backend)));
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/v2/spaces/profile-c")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["data"]["profileId"], "profile-c");
    }
}
