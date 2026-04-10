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
use axum::http::{
    header::{ACCESS_CONTROL_ALLOW_METHODS, ACCESS_CONTROL_ALLOW_ORIGIN},
    Request, StatusCode,
};
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
async fn protected_route_auth_failures_still_include_cors_headers() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Origin", "http://localhost:1420")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_ORIGIN)
            .and_then(|value| value.to_str().ok()),
        Some("http://localhost:1420")
    );
}

#[tokio::test]
async fn preflight_delete_includes_delete_in_allowed_methods() {
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/clipboard/entries/some-id")
                .header("Origin", "http://localhost:1420")
                .header("Access-Control-Request-Method", "DELETE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        response
            .headers()
            .get(ACCESS_CONTROL_ALLOW_METHODS)
            .and_then(|value| value.to_str().ok()),
        Some("GET, POST, PUT, DELETE, OPTIONS")
    );
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

// ========================================================================
// Phase 84: Bare Bearer Token Rejection Tests (AUTH-04, AUTH-06)
// ========================================================================

#[tokio::test]
async fn bare_bearer_rejected_with_invalid_auth_scheme_error() {
    // Phase 84: Daemon L2+ routes explicitly reject bare bearer tokens
    // with "invalid_auth_scheme" error (not "invalid_session_token").
    let (app, bearer, _security) = build_test_router_with_security().await;

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

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"].as_str(),
        Some("invalid_auth_scheme"),
        "error should be 'invalid_auth_scheme', got: {:?}",
        json["error"]
    );
    assert!(
        json["message"]
            .as_str()
            .is_some_and(|m| m.contains("/auth/connect")),
        "error message should mention /auth/connect hint"
    );
}

#[tokio::test]
async fn empty_session_token_rejected() {
    // Phase 84: "Session " with no token value gets "missing_session_token".
    let (app, _bearer, _security) = build_test_router_with_security().await;

    let response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", "Session ")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        json["error"].as_str(),
        Some("missing_session_token"),
        "error should be 'missing_session_token'"
    );
}

#[tokio::test]
async fn bare_bearer_on_l2_route_rejected_differently_than_invalid_jwt() {
    // Phase 84: Bare bearer gets "invalid_auth_scheme" (wrong scheme).
    // Tampered JWT gets "invalid_session_token" (valid scheme, bad value).
    // These are distinguishable error codes.
    let (app, bearer, _security) = build_test_router_with_security().await;
    let session_token = get_session_token(&app, &bearer).await;

    // Bare bearer (wrong scheme)
    let bare_response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", format!("Bearer {}", bearer.trim()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Tampered JWT (right scheme, bad value)
    let mut tampered = session_token.clone();
    tampered.push_str("X");
    let tampered_response = app
        .oneshot(
            Request::builder()
                .uri("/status")
                .header("Authorization", format!("Session {}", tampered))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(bare_response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(tampered_response.status(), StatusCode::UNAUTHORIZED);

    let bare_body = to_bytes(bare_response.into_body(), 4096).await.unwrap();
    let tampered_body = to_bytes(tampered_response.into_body(), 4096).await.unwrap();
    let bare_json: Value = serde_json::from_slice(&bare_body).unwrap();
    let tampered_json: Value = serde_json::from_slice(&tampered_body).unwrap();

    assert_eq!(
        bare_json["error"].as_str(),
        Some("invalid_auth_scheme"),
        "bare bearer should get 'invalid_auth_scheme'"
    );
    assert_eq!(
        tampered_json["error"].as_str(),
        Some("invalid_session_token"),
        "tampered JWT should get 'invalid_session_token'"
    );
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

// ---- WebSocket security behavior (documented via manual approach) ----
//
// Full WebSocket upgrade tests (connecting via tokio_tungstenite) require a running
// TCP server with a real listener. HTTP-level tests below cover the security rejection
// paths by verifying JWT session token content and correctness.
//
// MANUAL TESTING for WS security:
// 1. Start daemon: cargo run --bin uniclipboard-daemon
// 2. Get session token:
//    curl -X POST http://127.0.0.1:<port>/auth/connect \
//      -H "Authorization: Bearer $(cat ~/.config/uniclipboard/daemon.token)" \
//      -H "Content-Type: application/json" \
//      -d '{"pid":12345,"clientType":"gui"}'
// 3. Open WebSocket (expect success):
//    websocat "ws://127.0.0.1:<port>/ws" -H "Authorization: Session <token>"
// 4. Rejection scenarios:
//    - WS with no Authorization header: connection closed (401)
//    - WS with invalid token: HTTP 401 in upgrade response
//    - WS with valid token but unregistered PID: HTTP 403 in upgrade response
//    - WS after exceeding rate limit: HTTP 429 in upgrade response

#[tokio::test]
async fn session_token_contains_correct_pid() {
    // Verify that after calling /auth/connect with a specific client type and PID,
    // the resulting JWT can be verified and its claims contain the expected values.
    let (app, bearer, security) = build_test_router_with_security().await;
    // Use the current test process PID (which is pre-registered in build_test_router_with_security)
    let pid = std::process::id();

    let session_token = get_session_token(&app, &bearer).await;

    // Verify the token using SessionTokenClaims::verify — this exercises the
    // full JWT round-trip (sign at /auth/connect, verify here).
    use uc_daemon::security::claims::SessionTokenClaims;
    let claims = SessionTokenClaims::verify(&session_token, &security.jwt_secret)
        .expect("session token from /auth/connect should be valid");

    assert_eq!(
        claims.pid, pid,
        "JWT should contain the PID used in /auth/connect"
    );
    assert_eq!(
        claims.client_type, "test",
        "JWT should contain the clientType from /auth/connect"
    );
    assert_eq!(claims.access_level, 2, "JWT should have L2 access level");
    assert!(
        !claims.encryption_ready,
        "encryption_ready should be false by default"
    );
    assert!(!claims.jti.is_empty(), "JWT should have a non-empty jti");
    assert_eq!(
        claims.iss, "uniclipboard-daemon",
        "JWT issuer should be uniclipboard-daemon"
    );
    assert_eq!(claims.sub, "frontend", "JWT subject should be frontend");
    assert!(claims.exp > claims.iat, "exp should be greater than iat");
}

// ========================================================================
// AUTH-03: PID-based rate limiting isolation (per-client counters)
// ========================================================================

#[tokio::test]
async fn rate_limit_is_per_client_not_global() {
    // Verify that rate limiting uses per-client (PID) counters.
    // Exhausting the limit for PID A must NOT affect PID B.
    let (_app, _bearer, security) = build_test_router_with_security().await;

    let pid_a = 33333u32;
    let pid_b = 44444u32;

    // Register both PIDs
    security.register_pid(pid_a).await;
    security.register_pid(pid_b).await;

    // Exhaust rate limit for PID A (100 requests)
    for _ in 0..100 {
        let allowed = security.rate_limiter.check(&pid_a.to_string()).await;
        if !allowed {
            break;
        }
    }

    // Verify PID A is rate limited
    assert!(
        !security.rate_limiter.check(&pid_a.to_string()).await,
        "PID A should be rate limited after 100 requests"
    );

    // Verify PID B is NOT rate limited (independent counter)
    assert!(
        security.rate_limiter.check(&pid_b.to_string()).await,
        "PID B should NOT be rate limited (per-client counter isolation)"
    );
}

// ========================================================================
// AUTH-06: Bearer token ONLY accepted at /auth/connect
// ========================================================================

#[tokio::test]
async fn bearer_token_only_accepted_at_auth_connect() {
    // Verify that bearer token is accepted ONLY at /auth/connect.
    // All other L2 routes must reject it.
    let (app, bearer, _security) = build_test_router_with_security().await;

    // /auth/connect accepts bearer token (this is correct behavior)
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/auth/connect")
                .header("Authorization", format!("Bearer {bearer}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::to_string(&serde_json::json!({
                        "pid": 55555,
                        "clientType": "cli"
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
        "/auth/connect should accept bearer token"
    );

    // L2 routes reject bearer token (already verified by
    // bare_bearer_rejected_with_invalid_auth_scheme_error test above)
}

#[tokio::test]
async fn session_token_for_gui_client_type_contains_correct_claims() {
    // Verify that /auth/connect with clientType "gui" returns a JWT with
    // client_type = "gui" (not "test" or any other value).
    let (app, bearer, security) = build_test_router_with_security().await;
    let pid = std::process::id();

    // Call /auth/connect with clientType "gui" explicitly
    use axum::body::to_bytes;
    use axum::body::Body;
    use axum::http::Request;
    use serde_json::Value;
    use tower::ServiceExt;

    let response = app
        .clone()
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

    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let token_str = json["sessionToken"].as_str().unwrap();

    use uc_daemon::security::claims::SessionTokenClaims;
    let claims = SessionTokenClaims::verify(token_str, &security.jwt_secret)
        .expect("session token should be valid");

    assert_eq!(
        claims.client_type, "gui",
        "JWT clientType should match request"
    );
    assert_eq!(claims.pid, pid, "JWT pid should match request");
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

    let token_str = json["sessionToken"]
        .as_str()
        .expect("sessionToken should be a string");
    // JWT is three base64 segments separated by dots
    let parts: Vec<&str> = token_str.split('.').collect();
    assert_eq!(parts.len(), 3, "session token should be a 3-part JWT");

    let expires_in = json["expiresInSecs"]
        .as_i64()
        .expect("expiresInSecs should be integer");
    assert!(expires_in > 0, "expiresInSecs should be positive");

    let refresh_at = json["refreshAtSecs"]
        .as_i64()
        .expect("refreshAtSecs should be integer");
    assert!(refresh_at > 0, "refreshAtSecs should be positive");
    assert!(
        refresh_at < expires_in,
        "refreshAtSecs should be less than expiresInSecs"
    );
}
