use std::sync::Arc;
use std::sync::OnceLock;

use tokio::sync::RwLock;
use uc_core::network::{PairedDevice, PairingState};
use uc_daemon::api::projection::IntoApiDto;
use uc_daemon::api::query::DaemonQueryService;
use uc_daemon::api::types::{DaemonWsEvent, PairedDeviceDto, PeerSnapshotDto};
use uc_daemon::service::ServiceHealth;
use uc_daemon::state::{DaemonServiceSnapshot, RuntimeState};

fn build_runtime() -> Arc<uc_app::runtime::CoreRuntime> {
    static RUNTIME: OnceLock<Arc<uc_app::runtime::CoreRuntime>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| Arc::new(uc_bootstrap::build_cli_runtime(None).unwrap()))
        .clone()
}

#[tokio::test]
async fn empty_status_response() {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let service = DaemonQueryService::new(runtime, state);

    let status = service.status().await.unwrap();

    assert_eq!(status.connected_peers, 0);
    assert!(status.workers.is_empty());
    assert_eq!(status.package_version, uc_daemon::DAEMON_VERSION);
    assert_eq!(status.api_revision, uc_daemon::DAEMON_API_REVISION);
}

#[tokio::test]
async fn health_response_uses_package_version_and_api_revision_contract() {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let service = DaemonQueryService::new(runtime, state);

    let health = service.health().await;
    let json = serde_json::to_value(&health).unwrap();

    assert_eq!(health.package_version, uc_daemon::DAEMON_VERSION);
    assert_eq!(health.api_revision, uc_daemon::DAEMON_API_REVISION);
    assert_eq!(json["packageVersion"], uc_daemon::DAEMON_VERSION);
    assert_eq!(json["apiRevision"], uc_daemon::DAEMON_API_REVISION);
    assert!(json.get("version").is_none());
}

#[tokio::test]
async fn status_response_uses_package_version_and_api_revision_contract() {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![])));
    let service = DaemonQueryService::new(runtime, state);

    let status = service.status().await.unwrap();
    let json = serde_json::to_value(&status).unwrap();

    assert_eq!(json["packageVersion"], uc_daemon::DAEMON_VERSION);
    assert_eq!(json["apiRevision"], uc_daemon::DAEMON_API_REVISION);
    assert!(json.get("version").is_none());
}

#[test]
fn peers_query_maps_is_connected_to_connected() {
    let dto: PeerSnapshotDto = uc_app::usecases::pairing::get_p2p_peers_snapshot::P2pPeerSnapshot {
        peer_id: "peer-1".to_string(),
        device_name: Some("Desk".to_string()),
        addresses: vec!["/ip4/127.0.0.1/tcp/7000".to_string()],
        is_paired: true,
        is_connected: true,
        pairing_state: "Trusted".to_string(),
        identity_fingerprint: "fp-secret".to_string(),
    }
    .into_api_dto();

    assert_eq!(dto.peer_id, "peer-1");
    assert!(dto.connected);
    assert!(dto.is_paired);
}

#[test]
fn paired_devices_query_does_not_expose_identity_fingerprint() {
    let dto: PairedDeviceDto = PairedDevice {
        peer_id: uc_core::PeerId::from("peer-1"),
        pairing_state: PairingState::Trusted,
        identity_fingerprint: "fp-secret".to_string(),
        paired_at: chrono::Utc::now(),
        last_seen_at: None,
        device_name: "Desk".to_string(),
        sync_settings: None,
    }
    .into_api_dto();

    let json = serde_json::to_value(&dto).unwrap();

    assert!(json.get("identityFingerprint").is_none());
    assert_eq!(json["peerId"], "peer-1");
}

#[tokio::test]
async fn pairing_session_query_returns_none_when_no_daemon_side_record_exists() {
    let runtime = build_runtime();
    let state = Arc::new(RwLock::new(RuntimeState::new(vec![
        DaemonServiceSnapshot {
            name: "rpc".to_string(),
            health: ServiceHealth::Healthy,
        },
    ])));
    let service = DaemonQueryService::new(runtime, state);

    let session = service.pairing_session("missing-session").await.unwrap();

    assert!(session.is_none());
}

#[test]
fn websocket_dto_serialization_yields_session_id_and_type_keys() {
    let event = DaemonWsEvent {
        topic: "peers".to_string(),
        event_type: "peers.snapshot".to_string(),
        session_id: Some("session-1".to_string()),
        ts: 1_742_371_200_000,
        payload: serde_json::json!({
            "workers": [{"name": "rpc", "health": ServiceHealth::Healthy}]
        }),
    };

    let json = serde_json::to_value(&event).unwrap();

    assert_eq!(json["type"], "peers.snapshot");
    assert_eq!(json["sessionId"], "session-1");
    assert!(json.get("event_type").is_none());
    assert!(json.get("session_id").is_none());
}
