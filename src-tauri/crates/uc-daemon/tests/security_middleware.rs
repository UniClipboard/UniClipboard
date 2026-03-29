//! Integration tests for daemon security middleware.
//!
//! These tests exercise the HTTP-level behavior of:
//! - POST /auth/connect endpoint (bearer token exchange for JWT session token)
//! - auth_extractor_middleware (JWT validation, PID whitelist check)
//! - rate_limit_middleware (per-client rate limiting after authentication)
//! - L1 vs L2 router separation (public vs protected routes)
//!
//! These tests build on the integration test infrastructure in `tests/`
//! and use `tower::ServiceExt::oneshot` for stateless HTTP request dispatch.

use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use serde_json::Value;
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

/// Build a test router with a fresh SecurityState.
/// Returns (router, bearer_token, security_state).
async fn build_test_router_with_security() -> (axum::Router, String, Arc<SecurityState>) {
    let runtime = build_runtime();
    let state = Arc::new(tokio::sync::RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime, state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    let security = Arc::new(SecurityState::new());
    // Pre-register the test process PID so /auth/connect PID check passes
    security.register_pid(std::process::id()).await;
    let api_state = DaemonApiState::new(query_service, token, None, security.clone());
    let router = build_router(api_state);
    let token_value = std::fs::read_to_string(token_path).unwrap();
    (router, token_value, security)
}

/// Helper: call POST /auth/connect with the bearer token for the current PID.
async fn get_session_token(app: &axum::Router, bearer_token: &str) -> String {
    let pid = std::process::id();
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("Authorization", format!("Bearer {}", bearer_token.trim()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
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
        "/auth/connect should succeed with valid bearer token"
    );
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    json["sessionToken"].as_str().unwrap().to_string()
}

// ---- POST /auth/connect tests ----

#[tokio::test]
async fn auth_connect_returns_200_with_valid_bearer_token() {
    let (app, bearer, _security) = build_test_router_with_security().await;
    let pid = std::process::id();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("Authorization", format!("Bearer {}", bearer.trim()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "pid": pid,
                        "clientType": "test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json["sessionToken"].is_string(),
        "response should contain sessionToken string"
    );
    assert!(
        json["expiresInSecs"].is_number(),
        "response should contain expiresInSecs"
    );
    assert!(
        json["refreshAtSecs"].is_number(),
        "response should contain refreshAtSecs"
    );
}

#[tokio::test]
async fn auth_connect_returns_401_with_wrong_bearer_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;
    let pid = std::process::id();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("Authorization", "Bearer wrong-token-value")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "pid": pid,
                        "clientType": "test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn auth_connect_returns_401_with_missing_bearer_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;
    let pid = std::process::id();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "pid": pid,
                        "clientType": "test"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ---- auth_extractor_middleware tests ----

#[tokio::test]
async fn protected_route_returns_401_without_any_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

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
async fn protected_route_returns_401_with_bearer_token_instead_of_session_token() {
    let (app, bearer, _security) = build_test_router_with_security().await;

    // Use the raw bearer token directly on a protected route (should fail — bearer is not a JWT)
    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", format!("Bearer {}", bearer.trim()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Bearer token is not a valid JWT session token — should be rejected
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_200_with_valid_session_token() {
    let (app, bearer, _security) = build_test_router_with_security().await;
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
async fn protected_route_returns_401_with_tampered_session_token() {
    let (app, bearer, _security) = build_test_router_with_security().await;
    let session_token = get_session_token(&app, &bearer).await;

    // Tamper with the last few characters of the JWT signature
    let mut tampered = session_token.clone();
    tampered.push_str("INVALID");

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", format!("Session {}", tampered))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_returns_403_with_unregistered_pid() {
    let (app, _bearer, security) = build_test_router_with_security().await;
    // Generate a session token for a PID that is NOT registered in the whitelist
    let unregistered_pid = 999_999_999u32;
    let session_token = security.make_session_token_for_pid(unregistered_pid);

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

    // PID is not in the whitelist — should be 403 Forbidden
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

// ---- L1 vs L2 router separation tests ----

#[tokio::test]
async fn health_is_reachable_without_any_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "/health should be accessible without authentication"
    );
}

#[tokio::test]
async fn status_is_not_reachable_without_session_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "/status should require session token"
    );
}

#[tokio::test]
async fn paired_devices_is_not_reachable_without_session_token() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/paired-devices")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "/paired-devices should require session token"
    );
}

// ---- session token field validation ----

#[tokio::test]
async fn auth_connect_session_token_contains_expected_fields() {
    let (app, bearer, _security) = build_test_router_with_security().await;
    let pid = std::process::id();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("Authorization", format!("Bearer {}", bearer.trim()))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "pid": pid,
                        "clientType": "gui"
                    }))
                    .unwrap(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    let token_str = json["sessionToken"].as_str().expect("sessionToken should be a string");
    // JWT is three base64 segments separated by dots
    let parts: Vec<&str> = token_str.split('.').collect();
    assert_eq!(parts.len(), 3, "session token should be a 3-part JWT");

    let expires_in = json["expiresInSecs"].as_i64().expect("expiresInSecs should be integer");
    assert!(expires_in > 0, "expiresInSecs should be positive");

    let refresh_at = json["refreshAtSecs"].as_i64().expect("refreshAtSecs should be integer");
    assert!(refresh_at > 0, "refreshAtSecs should be positive");
    assert!(
        refresh_at < expires_in,
        "refreshAtSecs should be less than expiresInSecs"
    );
}
