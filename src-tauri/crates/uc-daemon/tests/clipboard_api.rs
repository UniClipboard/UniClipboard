//! Integration tests for the daemon clipboard HTTP API routes.
//!
//! Tests verify HTTP method contracts, status codes, and error response shapes
//! for clipboard CRUD endpoints.

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
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    let security = Arc::new(SecurityState::new());
    security.register_pid(std::process::id()).await;
    let api_state = DaemonApiState::new(query_service, token, Some(runtime.clone()), security);
    let router = build_router(api_state);
    let token_value = std::fs::read_to_string(token_path).unwrap();
    (router, token_value)
}

/// Helper: get a valid session token for authenticated requests.
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

/// Helper: make an authenticated request.
async fn auth_request(
    app: &axum::Router,
    session_token: &str,
    method: axum::http::Method,
    uri: &str,
    body: Option<Body>,
) -> axum::response::Response {
    use axum::http::header::AUTHORIZATION;

    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header(AUTHORIZATION, format!("Session {}", session_token));

    if let Some(_b) = &body {
        builder = builder.header("content-type", "application/json");
    }

    let request = builder.body(body.unwrap_or_else(|| Body::empty())).unwrap();

    app.clone().oneshot(request).await.unwrap()
}

// ── GET /clipboard/entries ────────────────────────────────────────

#[tokio::test]
async fn list_entries_returns_200_with_pagination() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::GET,
        "/clipboard/entries?limit=10&offset=0",
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    assert!(
        json.get("data").is_some(),
        "response should have 'data' key"
    );
    assert!(
        json.get("ts").is_some(),
        "response should have 'ts' timestamp"
    );
}

#[tokio::test]
async fn list_entries_requires_auth() {
    let (app, _token) = build_test_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/clipboard/entries")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ── DELETE /clipboard/entries/:id ───────────────────────────────

#[tokio::test]
async fn delete_entry_returns_404_for_nonexistent_id() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::DELETE,
        "/clipboard/entries/nonexistent-entry-id-00000000-0000-0000-0000-000000000000",
        None,
    )
    .await;

    // Should return 404 for non-existent entry
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── POST /clipboard/entries/clear ───────────────────────────────

#[tokio::test]
async fn clear_history_returns_200_with_result() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::POST,
        "/clipboard/entries/clear",
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Response must have data.deleted_count and data.failed_entries
    let data = json.get("data").expect("response should have 'data'");
    assert!(
        data.get("deleted_count").is_some() || data.get("deletedCount").is_some(),
        "data should have 'deleted_count' or 'deletedCount'"
    );
    assert!(
        data.get("failed_entries").is_some() || data.get("failedEntries").is_some(),
        "data should have 'failed_entries' or 'failedEntries'"
    );
}

#[tokio::test]
async fn clear_history_requires_auth() {
    let (app, _token) = build_test_router().await;

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/clipboard/entries/clear")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

// ── GET /clipboard/entries/:id ─────────────────────────────────

#[tokio::test]
async fn get_entry_returns_404_for_nonexistent_id() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::GET,
        "/clipboard/entries/nonexistent-entry-id-00000000-0000-0000-0000-000000000000",
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── GET /clipboard/entries/:id/resource ────────────────────────

#[tokio::test]
async fn get_entry_resource_returns_404_for_nonexistent_id() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::GET,
        "/clipboard/entries/nonexistent-entry-id-00000000-0000-0000-0000-000000000000/resource",
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

// ── POST /clipboard/entries/:id/favorite ───────────────────────

#[tokio::test]
async fn toggle_favorite_returns_404_for_nonexistent_id() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let body = Body::from(serde_json::to_string(&json!({ "isFavorited": true })).unwrap());
    let response = auth_request(
        &app,
        &session,
        axum::http::Method::POST,
        "/clipboard/entries/nonexistent-entry-id-00000000-0000-0000-0000-000000000000/favorite",
        Some(body),
    )
    .await;

    // Returns 404 when entry does not exist
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn toggle_favorite_returns_400_when_body_missing() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::POST,
        "/clipboard/entries/some-id/favorite",
        None, // No body
    )
    .await;

    // Should return 400 Bad Request when is_favorited body is missing
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

// ── GET /clipboard/stats ───────────────────────────────────────

#[tokio::test]
async fn get_stats_returns_200() {
    let (app, token) = build_test_router().await;
    let session = get_session_token(&app, &token).await;

    let response = auth_request(
        &app,
        &session,
        axum::http::Method::GET,
        "/clipboard/stats",
        None,
    )
    .await;

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();
    let data = json.get("data").expect("response should have 'data'");
    // Stats response must have total_items and total_size
    assert!(
        data.get("total_items").is_some() || data.get("totalItems").is_some(),
        "stats should have total_items or totalItems"
    );
    assert!(
        data.get("total_size").is_some() || data.get("totalSize").is_some(),
        "stats should have total_size or totalSize"
    );
}
