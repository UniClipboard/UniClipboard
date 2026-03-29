use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tokio::sync::RwLock;
use tower::ServiceExt;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

fn build_runtime() -> Arc<uc_app::runtime::CoreRuntime> {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap();
    Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap())
}

async fn build_test_router() -> (axum::Router, String) {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime, state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    // Register test process PID so session token PID whitelist checks pass
    let security = Arc::new(SecurityState::new());
    security.register_pid(std::process::id()).await;
    let api_state = DaemonApiState::new(query_service, token, None, security);
    let router = build_router(api_state);
    let token_value = std::fs::read_to_string(token_path).unwrap();
    (router, token_value)
}

/// Get a JWT session token by calling POST /auth/connect with the bearer token.
/// The test process PID is registered in the whitelist by build_test_router.
async fn get_session_token(app: &axum::Router, bearer_token: &str) -> String {
    use axum::http::header::AUTHORIZATION;
    let pid = std::process::id();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header(AUTHORIZATION, format!("Bearer {}", bearer_token.trim()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&json!({
                        "pid": pid,
                        "clientType": "test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "auth/connect should succeed with valid bearer token"
    );
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    json["sessionToken"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn health_is_reachable_without_auth() {
    let (app, _) = build_test_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn status_returns_401_without_session_token() {
    let (app, _) = build_test_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn status_returns_200_with_valid_session_token() {
    let (app, bearer) = build_test_router().await;
    let session_token = get_session_token(&app, &bearer).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", format!("Session {}", session_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn paired_devices_returns_array_body() {
    let (app, bearer) = build_test_router().await;
    let session_token = get_session_token(&app, &bearer).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/paired-devices")
                .header("Authorization", format!("Session {}", session_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(json.is_array());
}

#[tokio::test]
async fn pairing_sessions_returns_404_when_absent() {
    let (app, bearer) = build_test_router().await;
    let session_token = get_session_token(&app, &bearer).await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/pairing/sessions/missing-session")
                .header("Authorization", format!("Session {}", session_token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
