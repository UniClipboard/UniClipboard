//! Integration tests for the daemon search HTTP API routes.
//!
//! Tests verify the full capture → index → query → delete flow plus 423 lock
//! behavior over the real daemon HTTP transport surface.
//!
//! These tests use a real daemon runtime with a real SQLite database and the
//! SearchCoordinator wired in, proving the phase goal without any UI.

use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{Method, Request, StatusCode};
use serde_json::Value;
use tokio::sync::RwLock;
use tower::ServiceExt;
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::CoreUseCases;
use uc_core::ids::{EntryId, EventId};
use uc_core::search::{ContentType, SearchKey};
use uc_core::security::model::MasterKey;
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::search::coordinator::SearchCoordinator;
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;
use uc_infra::db::schema::search_posting;
use uc_infra::search::text_extractor::SearchPipelineInput;

// ---------------------------------------------------------------------------
// Shared fixture
// ---------------------------------------------------------------------------

struct SearchApiFixture {
    app: axum::Router,
    /// JWT session token for authenticated requests.
    session_token: String,
    runtime: Arc<CoreRuntime>,
}

fn build_runtime() -> Arc<CoreRuntime> {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|p| p.into_inner());

    // Unique profile per test invocation to avoid SQLite contention between
    // tests run in parallel within the same binary.
    let profile = format!(
        "test_search_api_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system time")
            .as_nanos()
    );
    std::env::set_var("UC_PROFILE", &profile);
    Arc::new(uc_bootstrap::build_cli_runtime(None).expect("build_cli_runtime failed"))
}

async fn build_fixture() -> SearchApiFixture {
    let runtime = build_runtime();

    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();

    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);

    // Build DaemonApiState first, then create SearchCoordinator using the same
    // event_tx so rebuild progress events reach the WS fanout.
    let api_state_base = DaemonApiState::new(query_service, token, Some(runtime.clone()), security);
    let coordinator = Arc::new(SearchCoordinator::new(
        runtime.clone(),
        api_state_base.event_tx.clone(),
    ));
    let api_state = api_state_base.with_search_coordinator(coordinator);

    let app = build_router(api_state);

    SearchApiFixture {
        app,
        session_token,
        runtime,
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

async fn auth_request(
    app: &axum::Router,
    session_token: &str,
    method: Method,
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

    let request = builder.body(body.unwrap_or_else(Body::empty)).unwrap();
    app.clone().oneshot(request).await.unwrap()
}

/// Unlock the encryption session with a test key, making all search routes accessible.
async fn unlock_encryption(runtime: &Arc<CoreRuntime>) {
    let master_key = MasterKey::generate().expect("MasterKey::generate failed");
    runtime
        .wiring_deps()
        .security
        .encryption_session
        .set_master_key(master_key)
        .await
        .expect("set_master_key failed");
}

/// Lock (clear) the encryption session, causing search routes to return 423.
async fn lock_encryption(runtime: &Arc<CoreRuntime>) {
    runtime
        .wiring_deps()
        .security
        .encryption_session
        .clear()
        .await
        .expect("clear encryption session failed");
}

/// Index a test clipboard entry using a fixed key.
async fn index_test_entry(runtime: &Arc<CoreRuntime>, entry_id: &EntryId, text: &str) {
    let search_key = SearchKey([0x42u8; 32]);
    let deps = runtime.wiring_deps();

    let input = SearchPipelineInput {
        entry_id: entry_id.clone(),
        event_id: EventId::from("evt-search-api-test"),
        active_time_ms: chrono::Utc::now().timestamp_millis(),
        captured_at_ms: chrono::Utc::now().timestamp_millis(),
        content_type: ContentType::Text,
        mime_type: "text/plain".into(),
        file_extensions: vec![],
        plain_text: Some(text.to_string()),
        html_text: None,
        uri_list: vec![],
        file_paths: vec![],
        file_names: vec![],
        text_preview: Some(text[..text.len().min(80)].to_string()),
    };

    let pipeline = deps.search.search_pipeline.as_ref();
    let (doc, postings) = pipeline
        .build(&input, &search_key)
        .expect("pipeline.build should succeed");

    assert!(
        !postings.is_empty(),
        "should have postings for text content"
    );

    let usecases = CoreUseCases::new(runtime.as_ref());
    usecases
        .index_clipboard_entry()
        .execute(doc, postings)
        .await
        .expect("index_clipboard_entry should succeed");
}

/// Count how many `search_posting` rows exist for the given entry_id using a
/// direct SQLite connection to the runtime's database.
///
/// This provides the "direct DB inspection" required by the acceptance criteria:
/// "deleting that entry leaves zero search_posting rows for its entry_id".
fn count_search_postings_for_entry(db_path: &std::path::Path, entry_id_str: &str) -> i64 {
    use diesel::prelude::*;

    // Open a fresh connection to the same SQLite file used by the runtime.
    let db_url = db_path.to_str().expect("db_path must be valid UTF-8");
    let mut conn = diesel::sqlite::SqliteConnection::establish(db_url)
        .expect("failed to connect to SQLite for posting count");

    search_posting::table
        .filter(search_posting::entry_id.eq(entry_id_str))
        .count()
        .get_result(&mut conn)
        .expect("count query on search_posting failed")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// End-to-end HTTP integration test proving:
///
/// 1. A search entry can be indexed (via use case, not HTTP).
/// 2. `GET /search/query` returns 200 when encryption is unlocked.
/// 3. Deleting the entry via `SearchIndexPort::remove_entry` leaves zero
///    `search_posting` rows for that `entry_id` (direct DB inspection).
/// 4. After locking encryption, `/search/query`, `/search/status`, and
///    `/search/rebuild` each return HTTP 423 with `code == "session_locked"`.
#[tokio::test]
async fn search_api_end_to_end_capture_query_and_locking() {
    let fixture = build_fixture().await;
    let app = &fixture.app;
    let session = &fixture.session_token;
    let runtime = &fixture.runtime;
    let db_path = runtime.storage_paths().db_path.clone();

    // --- PHASE 1: unlock and index a test entry ---
    unlock_encryption(runtime).await;

    let entry_id = EntryId::from(format!(
        "search-api-e2e-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_micros()
    ));
    index_test_entry(runtime, &entry_id, "zephyrwordtest clipboard sync").await;

    // --- PHASE 2: search/query returns 200 (lock guard passes) ---
    let response = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=zephyrwordtest",
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "search/query should return 200 when encryption is unlocked"
    );
    let body_bytes = to_bytes(response.into_body(), 65536).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert!(json.get("data").is_some(), "response must have 'data' key");
    assert!(
        json.get("total").is_some(),
        "response must have 'total' key"
    );

    // --- PHASE 3: delete removes all search_posting rows ---
    // Verify postings exist before removal.
    let count_before = count_search_postings_for_entry(&db_path, entry_id.as_str());
    assert!(
        count_before > 0,
        "entry should have search_posting rows after indexing, got {}",
        count_before
    );

    // Remove via the search_index port (same path as DeleteClipboardEntry uses).
    runtime
        .wiring_deps()
        .search
        .search_index
        .remove_entry(&entry_id)
        .await
        .expect("remove_entry should succeed");

    let count_after = count_search_postings_for_entry(&db_path, entry_id.as_str());
    assert_eq!(
        count_after, 0,
        "zero search_posting rows should remain after remove_entry, got {}",
        count_after
    );

    // --- PHASE 4: lock encryption → all three search routes return 423 ---
    lock_encryption(runtime).await;

    // /search/query → 423
    let locked_query =
        auth_request(app, session, Method::GET, "/search/query?query=test", None).await;
    assert_eq!(
        locked_query.status(),
        StatusCode::LOCKED,
        "search/query must return 423 when encryption is locked"
    );
    let body_bytes = to_bytes(locked_query.into_body(), 4096).await.unwrap();
    let locked_json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        locked_json["code"], "session_locked",
        "error code must be session_locked"
    );

    // /search/status → 423
    let locked_status = auth_request(app, session, Method::GET, "/search/status", None).await;
    assert_eq!(
        locked_status.status(),
        StatusCode::LOCKED,
        "search/status must return 423 when encryption is locked"
    );

    // /search/rebuild → 423
    let locked_rebuild = auth_request(app, session, Method::POST, "/search/rebuild", None).await;
    assert_eq!(
        locked_rebuild.status(),
        StatusCode::LOCKED,
        "search/rebuild must return 423 when encryption is locked"
    );
}

/// HTTP integration test proving that the search query route parses filters
/// correctly and rejects mixed AND/OR operators with `invalid_query`.
///
/// Tests the transport-layer integration: URL query string → parse_search_query()
/// → actual axum response.
#[tokio::test]
async fn search_query_route_parses_filters_and_rejects_mixed_operators() {
    let fixture = build_fixture().await;
    let app = &fixture.app;
    let session = &fixture.session_token;
    let runtime = &fixture.runtime;

    // Unlock encryption so the route proceeds past the lock guard.
    unlock_encryption(runtime).await;

    // --- Mixed AND + OR returns 400 with invalid_query ---
    let response = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=hello+AND+world+OR+foo",
        None,
    )
    .await;
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "mixed AND/OR query must return 400"
    );
    let body_bytes = to_bytes(response.into_body(), 4096).await.unwrap();
    let json: Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(
        json["code"], "invalid_query",
        "error code must be invalid_query for mixed AND/OR"
    );

    // --- contentTypes filter parses comma-separated values ---
    let response_ft = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&contentTypes=text,html",
        None,
    )
    .await;
    assert_eq!(
        response_ft.status(),
        StatusCode::OK,
        "valid contentTypes filter should return 200"
    );

    // --- extensions filter parses comma-separated values ---
    let response_ext = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&extensions=md,txt",
        None,
    )
    .await;
    assert_eq!(
        response_ext.status(),
        StatusCode::OK,
        "valid extensions filter should return 200"
    );

    // --- timePreset filter is accepted ---
    let response_preset = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&timePreset=last_7d",
        None,
    )
    .await;
    assert_eq!(
        response_preset.status(),
        StatusCode::OK,
        "valid timePreset should return 200"
    );

    // --- fromMs + toMs absolute range ---
    let response_abs = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&fromMs=1000&toMs=2000",
        None,
    )
    .await;
    assert_eq!(
        response_abs.status(),
        StatusCode::OK,
        "valid absolute time range should return 200"
    );

    // --- invalid fileType returns 400 ---
    let response_bad_ft = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&contentTypes=not_a_type",
        None,
    )
    .await;
    assert_eq!(
        response_bad_ft.status(),
        StatusCode::BAD_REQUEST,
        "invalid fileType should return 400"
    );

    // --- invalid timePreset returns 400 ---
    let response_bad_preset = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&timePreset=last_5_years",
        None,
    )
    .await;
    assert_eq!(
        response_bad_preset.status(),
        StatusCode::BAD_REQUEST,
        "invalid timePreset should return 400"
    );

    // --- mismatched fromMs/toMs (only fromMs present) returns 400 ---
    let response_mismatch = auth_request(
        app,
        session,
        Method::GET,
        "/search/query?query=test&fromMs=1000",
        None,
    )
    .await;
    assert_eq!(
        response_mismatch.status(),
        StatusCode::BAD_REQUEST,
        "mismatched fromMs/toMs should return 400"
    );
}
