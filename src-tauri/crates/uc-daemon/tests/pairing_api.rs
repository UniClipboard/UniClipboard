use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use chrono::Utc;
use serde_json::{json, Value};
use tokio::sync::{broadcast, RwLock};
use tower::ServiceExt;
use uc_app::runtime::CoreRuntime;
use uc_app::usecases::CoreUseCases;
use uc_bootstrap::assembly::SetupAssemblyPorts;
use uc_bootstrap::{build_non_gui_runtime_with_setup, builders::build_daemon_app};
use uc_core::network::{PairedDevice, PairingState};
use uc_daemon::api::auth::load_or_create_auth_token;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::server::{build_router, DaemonApiState};
use uc_daemon::api::types::DaemonWsEvent;
use uc_daemon::pairing::host::DaemonPairingHost;
use uc_daemon::security::SecurityState;
use uc_daemon::state::RuntimeState;

struct PairingApiFixture {
    app: axum::Router,
    /// JWT session token (pre-obtained) for authenticated requests.
    token: String,
    runtime: Arc<CoreRuntime>,
    pairing_host: Arc<DaemonPairingHost>,
}

fn build_api_fixture() -> PairingApiFixture {
    static RUNTIME_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = RUNTIME_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let ctx = build_daemon_app().unwrap();
    let setup_ports = SetupAssemblyPorts::from_network(
        ctx.pairing_orchestrator.clone(),
        ctx.space_access_orchestrator.clone(),
        ctx.deps.network_ports.peers.clone(),
        None,
        Arc::new(uc_app::usecases::LoggingLifecycleEventEmitter),
    );
    let runtime = Arc::new(
        build_non_gui_runtime_with_setup(
            ctx.deps,
            ctx.storage_paths.clone(),
            setup_ports,
        )
        .unwrap(),
    );
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let query_service = Arc::new(DaemonQueryService::new(runtime.clone(), state.clone()));
    let tempdir = tempfile::tempdir().unwrap();
    let token_path = tempdir.path().join("daemon.token");
    let token = load_or_create_auth_token(&token_path).unwrap();
    let token_value = std::fs::read_to_string(&token_path).unwrap();
    let (event_tx, _event_rx) = broadcast::channel::<DaemonWsEvent>(128);
    let pairing_host = Arc::new(DaemonPairingHost::new(
        runtime.clone(),
        ctx.pairing_orchestrator,
        ctx.pairing_action_rx,
        state,
        ctx.space_access_orchestrator,
        ctx.key_slot_store,
        event_tx,
    ));
    // Pre-register the test process PID so session tokens created here pass the whitelist check.
    let pid = std::process::id();
    let security = Arc::new(SecurityState::new_with_pid(pid));
    let session_token = security.make_session_token_for_pid(pid);
    let api_state = DaemonApiState::new(query_service, token, Some(runtime.clone()), security)
        .with_pairing_host(pairing_host.clone());
    PairingApiFixture {
        app: build_router(api_state),
        token: session_token,
        runtime,
        pairing_host,
    }
}

fn build_api_router() -> (axum::Router, String) {
    let fixture = build_api_fixture();
    (fixture.app, fixture.token)
}

async fn build_api_router_async() -> (axum::Router, String) {
    tokio::task::spawn_blocking(build_api_router)
        .await
        .expect("pairing api fixture join failed")
}

async fn build_api_fixture_async() -> PairingApiFixture {
    tokio::task::spawn_blocking(build_api_fixture)
        .await
        .expect("pairing api fixture join failed")
}

fn authed_request(
    method: &str,
    uri: &str,
    token: &str,
    body: Body,
    content_type: Option<&str>,
) -> Request<Body> {
    // token is a JWT session token (pre-obtained via SecurityState::make_session_token_for_pid)
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Session {}", token.trim()));
    if let Some(content_type) = content_type {
        builder = builder.header("Content-Type", content_type);
    }
    builder.body(body).unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    serde_json::from_slice(&body).unwrap()
}

async fn set_discoverability(
    app: &axum::Router,
    token: &str,
    discoverable: bool,
    lease_ttl_ms: Option<u64>,
) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "PUT",
            "/pairing/discoverability/current",
            token,
            Body::from(
                json!({
                    "clientKind": "cli",
                    "discoverable": discoverable,
                    "leaseTtlMs": lease_ttl_ms
                })
                .to_string(),
            ),
            Some("application/json"),
        ))
        .await
        .unwrap()
}

async fn set_participant_ready(
    app: &axum::Router,
    token: &str,
    ready: bool,
    lease_ttl_ms: Option<u64>,
) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "PUT",
            "/pairing/participants/current",
            token,
            Body::from(
                json!({
                    "clientKind": "cli",
                    "ready": ready,
                    "leaseTtlMs": lease_ttl_ms
                })
                .to_string(),
            ),
            Some("application/json"),
        ))
        .await
        .unwrap()
}

async fn initiate_pairing(
    app: &axum::Router,
    token: &str,
    peer_id: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/sessions",
            token,
            Body::from(json!({ "peerId": peer_id }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap()
}

async fn initiate_pairing_v2(
    app: &axum::Router,
    token: &str,
    peer_id: &str,
) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/initiate",
            token,
            Body::from(json!({ "peerId": peer_id }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap()
}

async fn gui_lease(
    app: &axum::Router,
    token: &str,
    enabled: bool,
    lease_ttl_ms: Option<u64>,
) -> axum::response::Response {
    app.clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/gui/lease",
            token,
            Body::from(
                json!({
                    "enabled": enabled,
                    "leaseTtlMs": lease_ttl_ms
                })
                .to_string(),
            ),
            Some("application/json"),
        ))
        .await
        .unwrap()
}

fn test_paired_device(peer_id: &str) -> PairedDevice {
    PairedDevice {
        peer_id: peer_id.into(),
        pairing_state: PairingState::Trusted,
        identity_fingerprint: "fingerprint-test".to_string(),
        paired_at: Utc::now(),
        last_seen_at: None,
        device_name: "Remote Device".to_string(),
        sync_settings: None,
    }
}

#[tokio::test]
async fn pairing_api_returns_409_active_pairing_session_exists() {
    let (app, token) = build_api_router_async().await;
    assert_eq!(
        set_discoverability(&app, &token, true, Some(60_000))
            .await
            .status(),
        StatusCode::ACCEPTED
    );
    assert_eq!(
        set_participant_ready(&app, &token, true, Some(60_000))
            .await
            .status(),
        StatusCode::ACCEPTED
    );

    let first = initiate_pairing(&app, &token, "peer-a").await;
    let second = initiate_pairing(&app, &token, "peer-b").await;

    assert_eq!(first.status(), StatusCode::ACCEPTED);
    assert_eq!(second.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(second).await["error"],
        Value::String("active_pairing_session_exists".to_string())
    );
}

#[tokio::test]
async fn pairing_api_returns_412_when_no_local_participant_ready() {
    let (app, token) = build_api_router_async().await;
    assert_eq!(
        set_discoverability(&app, &token, true, Some(60_000))
            .await
            .status(),
        StatusCode::ACCEPTED
    );

    let response = initiate_pairing(&app, &token, "peer-a").await;

    assert_eq!(response.status(), StatusCode::PRECONDITION_FAILED);
    assert_eq!(
        json_body(response).await["error"],
        Value::String("no_local_pairing_participant_ready".to_string())
    );
}

#[tokio::test]
async fn pairing_api_returns_409_host_not_discoverable() {
    let (app, token) = build_api_router_async().await;
    assert_eq!(
        set_participant_ready(&app, &token, true, Some(60_000))
            .await
            .status(),
        StatusCode::ACCEPTED
    );

    let response = initiate_pairing(&app, &token, "peer-a").await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"],
        Value::String("host_not_discoverable".to_string())
    );
}

#[tokio::test]
async fn pairing_api_returns_404_for_unknown_followup_session() {
    let (app, token) = build_api_router_async().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/sessions/missing-session/accept",
            &token,
            Body::empty(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn pairing_api_returns_400_for_malformed_payload() {
    let (app, token) = build_api_router_async().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "PUT",
            "/pairing/discoverability/current",
            &token,
            Body::from("{"),
            Some("application/json"),
        ))
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn pairing_api_requires_explicit_discoverability_opt_in_for_cli() {
    let (app, token) = build_api_router_async().await;
    set_participant_ready(&app, &token, true, Some(60_000)).await;

    let response = initiate_pairing(&app, &token, "peer-a").await;
    let body = json_body(response).await;

    assert_eq!(body["error"], "host_not_discoverable");
}

#[tokio::test]
async fn pairing_api_expires_discoverability_lease() {
    let (app, token) = build_api_router_async().await;
    set_discoverability(&app, &token, true, Some(10)).await;
    set_participant_ready(&app, &token, true, Some(60_000)).await;

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let response = initiate_pairing(&app, &token, "peer-a").await;

    assert_eq!(response.status(), StatusCode::CONFLICT);
    assert_eq!(
        json_body(response).await["error"],
        Value::String("host_not_discoverable".to_string())
    );
}

#[tokio::test]
async fn pairing_api_v2_initiate_returns_session_id_body() {
    let (app, token) = build_api_router_async().await;

    assert_eq!(
        gui_lease(&app, &token, true, None).await.status(),
        StatusCode::NO_CONTENT
    );

    let response = initiate_pairing_v2(&app, &token, "peer-a").await;
    let body = json_body(response).await;

    assert_eq!(
        body.get("sessionId").and_then(Value::as_str).is_some(),
        true
    );
}

#[tokio::test]
async fn pairing_api_gui_lease_honors_custom_ttl() {
    let fixture = build_api_fixture_async().await;

    assert_eq!(
        gui_lease(&fixture.app, &fixture.token, true, Some(20))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert!(fixture.pairing_host.discoverable().await);
    assert!(fixture.pairing_host.participant_ready().await);

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    assert!(
        !fixture.pairing_host.discoverable().await,
        "custom gui lease ttl should expire discoverability"
    );
    assert!(
        !fixture.pairing_host.participant_ready().await,
        "custom gui lease ttl should expire participant readiness"
    );
}

#[tokio::test]
async fn pairing_api_v2_maps_no_local_participant_error_to_400_code() {
    let (app, token) = build_api_router_async().await;
    assert_eq!(
        set_discoverability(&app, &token, true, Some(60_000))
            .await
            .status(),
        StatusCode::ACCEPTED
    );

    let response = initiate_pairing_v2(&app, &token, "peer-a").await;
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "no_local_participant");
    assert_eq!(body["message"], "no local pairing participant ready");
}

#[tokio::test]
async fn pairing_api_v2_returns_404_for_unknown_accept_session() {
    let (app, token) = build_api_router_async().await;

    let response = app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/accept",
            &token,
            Body::from(json!({ "sessionId": "missing-session" }).to_string()),
            Some("application/json"),
        ))
        .await
        .unwrap();
    let status = response.status();
    let body = json_body(response).await;

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "session_not_found");
}

#[tokio::test]
async fn pairing_unpair_route_unpairs_device_via_daemon_usecase() {
    let fixture = build_api_fixture_async().await;
    CoreUseCases::new(fixture.runtime.as_ref())
        .start_network()
        .execute()
        .await
        .expect("start network for unpair route fixture");
    let local_peer_id = CoreUseCases::new(fixture.runtime.as_ref())
        .get_local_device_info()
        .execute()
        .await
        .expect("local device info should load")
        .peer_id;
    let peer_id = if local_peer_id == "12D3KooWRkw1xHve9tp9KjBKWDC7oMqwY8Y7FuPMBcUvr7G7q42T" {
        "12D3KooWPNHngjzbB6MkDnLPrm89g43hHZq94zX94dsZAwHrvS2S"
    } else {
        "12D3KooWRkw1xHve9tp9KjBKWDC7oMqwY8Y7FuPMBcUvr7G7q42T"
    };
    let repo = fixture
        .runtime
        .wiring_deps()
        .device
        .paired_device_repo
        .clone();
    repo.upsert(test_paired_device(peer_id))
        .await
        .expect("seed paired device");

    let usecases = CoreUseCases::new(fixture.runtime.as_ref());
    let before = usecases
        .list_paired_devices()
        .execute()
        .await
        .expect("list paired devices before unpair");
    assert!(
        before
            .iter()
            .any(|device| device.peer_id.as_str() == peer_id),
        "seeded paired device should exist before unpair route"
    );

    let response = fixture
        .app
        .clone()
        .oneshot(authed_request(
            "POST",
            "/pairing/unpair",
            &fixture.token,
            Body::from(json!({ "peerId": peer_id }).to_string()),
            Some("application/json"),
        ))
        .await
        .expect("unpair request should complete");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let after = usecases
        .list_paired_devices()
        .execute()
        .await
        .expect("list paired devices after unpair");
    assert!(
        after
            .iter()
            .all(|device| device.peer_id.as_str() != peer_id),
        "daemon unpair route should remove paired device from runtime state"
    );
}
