//! Pairing-related Tauri commands
//! 配对相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::record_trace_fields;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::{Emitter, State};
use tracing::{info_span, Instrument};
use uc_app::usecases::{LocalDeviceInfo, PairingOrchestrator};
use uc_core::network::{ConnectedPeer, DiscoveredPeer, PairedDevice, PairingState};
use uc_core::ports::observability::TraceMetadata;
use uc_core::PeerId;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2PPeerInfo {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairedPeer {
    pub peer_id: String,
    pub device_name: String,
    pub shared_secret: Vec<u8>,
    pub paired_at: String,
    pub last_seen: Option<String>,
    pub last_known_addresses: Vec<String>,
    pub connected: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2PPairingRequest {
    pub peer_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2PPairingResponse {
    pub session_id: String,
    pub success: bool,
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct P2PPinVerifyRequest {
    pub session_id: String,
    pub pin_matches: bool,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct P2PCommandErrorEvent {
    command: String,
    message: String,
}

/// List paired devices
/// 列出已配对设备
#[tauri::command]
pub async fn list_paired_devices(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedDevice>, String> {
    let span = info_span!(
        "command.pairing.list",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().list_paired_devices();
        let devices = uc.execute().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to list paired devices");
            let message = e.to_string();
            emit_command_error(&runtime, "list_paired_devices", &message);
            message
        })?;
        Ok(devices)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_local_peer_id(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, String> {
    let span = info_span!(
        "command.pairing.get_local_peer_id",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async { Ok(runtime.usecases().get_local_peer_id().execute()) }
        .instrument(span)
        .await
}

#[tauri::command]
pub async fn get_local_device_info(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<LocalDeviceInfo, String> {
    let span = info_span!(
        "command.pairing.get_local_device_info",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        runtime
            .usecases()
            .get_local_device_info()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to get local device info");
                let message = e.to_string();
                emit_command_error(&runtime, "get_local_device_info", &message);
                message
            })
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_p2p_peers(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<P2PPeerInfo>, String> {
    let span = info_span!(
        "command.pairing.get_p2p_peers",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        device_id = %runtime.deps.device_identity.current_device_id(),
    );
    record_trace_fields(&span, &_trace);
    async {
        let discovered = runtime
            .usecases()
            .list_discovered_peers()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list discovered peers");
                let message = format!("list_discovered_peers: {}", e);
                emit_command_error(&runtime, "get_p2p_peers", &message);
                e.to_string()
            })?;
        let connected = runtime
            .usecases()
            .list_connected_peers()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list connected peers");
                let message = format!("list_connected_peers: {}", e);
                emit_command_error(&runtime, "get_p2p_peers", &message);
                e.to_string()
            })?;
        let connected_map = connected_peer_ids(&connected);
        tracing::info!(
            discovered_peer_count = discovered.len(),
            connected_peer_count = connected_map.len(),
            "assembled p2p peer snapshot"
        );

        Ok(discovered
            .into_iter()
            .map(|peer| P2PPeerInfo {
                peer_id: peer.peer_id.clone(),
                device_name: peer.device_name,
                addresses: peer.addresses,
                is_paired: peer.is_paired,
                connected: connected_map.contains_key(&peer.peer_id),
            })
            .collect())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_paired_peers(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedPeer>, String> {
    get_paired_peers_with_status(runtime, _trace).await
}

#[tauri::command]
pub async fn get_paired_peers_with_status(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedPeer>, String> {
    let span = info_span!(
        "command.pairing.get_paired_peers_with_status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        device_id = %runtime.deps.device_identity.current_device_id(),
    );
    record_trace_fields(&span, &_trace);
    async {
        let paired_devices = runtime
            .usecases()
            .list_paired_devices()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list paired devices");
                let message = format!("list_paired_devices: {}", e);
                emit_command_error(&runtime, "get_paired_peers_with_status", &message);
                e.to_string()
            })?;
        let discovered = runtime
            .usecases()
            .list_discovered_peers()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list discovered peers");
                let message = format!("list_discovered_peers: {}", e);
                emit_command_error(&runtime, "get_paired_peers_with_status", &message);
                e.to_string()
            })?;
        let connected = runtime
            .usecases()
            .list_connected_peers()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to list connected peers");
                let message = format!("list_connected_peers: {}", e);
                emit_command_error(&runtime, "get_paired_peers_with_status", &message);
                e.to_string()
            })?;
        let discovered_map = discovered_peer_map(&discovered);
        let connected_map = connected_peer_ids(&connected);
        tracing::info!(
            paired_device_count = paired_devices.len(),
            discovered_peer_count = discovered_map.len(),
            connected_peer_count = connected_map.len(),
            "assembled paired peers with status"
        );

        Ok(paired_devices
            .into_iter()
            .map(|device| {
                let peer_id = device.peer_id.as_str().to_string();
                let discovered_peer = discovered_map.get(&peer_id);
                let connected = connected_map.contains_key(&peer_id);
                map_paired_device_to_peer(device, discovered_peer, connected)
            })
            .collect())
    }
    .instrument(span)
    .await
}

fn map_paired_device_to_peer(
    device: PairedDevice,
    discovered_peer: Option<&DiscoveredPeer>,
    connected: bool,
) -> PairedPeer {
    let peer_id = device.peer_id.as_str().to_string();

    // Use persisted device_name as primary, fallback to discovered name, then to "Unknown Device"
    let device_name = if !device.device_name.is_empty() {
        device.device_name.clone()
    } else {
        discovered_peer
            .and_then(|peer| peer.device_name.clone())
            .unwrap_or_else(|| "Unknown Device".to_string())
    };

    let addresses = discovered_peer
        .map(|peer| peer.addresses.clone())
        .unwrap_or_default();

    PairedPeer {
        peer_id,
        device_name,
        shared_secret: vec![],
        paired_at: device.paired_at.to_rfc3339(),
        last_seen: device.last_seen_at.map(|time| time.to_rfc3339()),
        last_known_addresses: addresses,
        connected,
    }
}

/// Update pairing state for a peer
/// 更新对等端配对状态
#[tauri::command]
pub async fn set_pairing_state(
    runtime: State<'_, Arc<AppRuntime>>,
    peer_id: String,
    state: PairingState,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.pairing.set_state",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().set_pairing_state();
        uc.execute(PeerId::from(peer_id.as_str()), state)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to set pairing state");
                let message = e.to_string();
                emit_command_error(&runtime, "set_pairing_state", &message);
                message
            })?;
        Ok(())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn initiate_p2p_pairing(
    request: P2PPairingRequest,
    orchestrator: State<'_, Arc<PairingOrchestrator>>,
    _trace: Option<TraceMetadata>,
) -> Result<P2PPairingResponse, String> {
    let span = info_span!(
        "command.pairing.initiate",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %request.peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        let session_id = orchestrator
            .initiate_pairing(request.peer_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to initiate P2P pairing");
                e.to_string()
            })?;
        Ok(P2PPairingResponse {
            session_id,
            success: true,
            error: None,
        })
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn accept_p2p_pairing(
    session_id: String,
    orchestrator: State<'_, Arc<PairingOrchestrator>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.pairing.accept",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        orchestrator
            .user_accept_pairing(&session_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, session_id = %session_id, "Failed to accept P2P pairing");
                e.to_string()
            })
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn reject_p2p_pairing(
    session_id: String,
    orchestrator: State<'_, Arc<PairingOrchestrator>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.pairing.reject",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        orchestrator
            .user_reject_pairing(&session_id)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, session_id = %session_id, "Failed to reject P2P pairing");
                e.to_string()
            })
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn verify_p2p_pairing_pin(
    request: P2PPinVerifyRequest,
    orchestrator: State<'_, Arc<PairingOrchestrator>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.pairing.verify_pin",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %request.session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        if request.pin_matches {
            orchestrator
                .user_accept_pairing(&request.session_id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        session_id = %request.session_id,
                        "Failed to accept P2P pairing (pin verify)"
                    );
                    e.to_string()
                })
        } else {
            orchestrator
                .user_reject_pairing(&request.session_id)
                .await
                .map_err(|e| {
                    tracing::error!(
                        error = %e,
                        session_id = %request.session_id,
                        "Failed to reject P2P pairing (pin verify)"
                    );
                    e.to_string()
                })
        }
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn unpair_p2p_device(
    peer_id: String,
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.pairing.unpair",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().unpair_device();
        uc.execute(peer_id.clone()).await.map_err(|e| {
            tracing::error!(error = %e, peer_id = %peer_id, "Failed to unpair P2P device");
            let message = e.to_string();
            emit_command_error(&runtime, "unpair_p2p_device", &message);
            message
        })
    }
    .instrument(span)
    .await
}

fn emit_command_error(runtime: &AppRuntime, command: &str, message: &str) {
    if let Some(app) = runtime.app_handle().as_ref() {
        let payload = P2PCommandErrorEvent {
            command: command.to_string(),
            message: message.to_string(),
        };
        if let Err(err) = app.emit("p2p-command-error", payload) {
            tracing::warn!(error = %err, command = %command, "Failed to emit p2p command error");
        }
    } else {
        tracing::debug!(
            command = %command,
            "AppHandle not available, skipping p2p command error emission"
        );
    }
}

fn discovered_peer_map(peers: &[DiscoveredPeer]) -> HashMap<String, DiscoveredPeer> {
    peers
        .iter()
        .map(|peer| (peer.peer_id.clone(), peer.clone()))
        .collect()
}

fn connected_peer_ids(peers: &[ConnectedPeer]) -> HashMap<String, ConnectedPeer> {
    peers
        .iter()
        .map(|peer| (peer.peer_id.clone(), peer.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uc_core::network::{DiscoveredPeer, PairedDevice, PairingState};
    use uc_core::PeerId;

    #[test]
    fn test_map_paired_device_to_peer_uses_persisted_name() {
        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "Persisted Name".to_string(),
        };

        let discovered = DiscoveredPeer {
            peer_id: "peer-1".to_string(),
            device_name: Some("Discovered Name".to_string()),
            device_id: None,
            addresses: vec!["127.0.0.1:1234".to_string()],
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: true,
        };

        let result = map_paired_device_to_peer(device, Some(&discovered), true);

        assert_eq!(result.device_name, "Persisted Name");
        assert_eq!(result.last_known_addresses, vec!["127.0.0.1:1234"]);
        assert!(result.connected);
    }

    #[test]
    fn test_map_paired_device_to_peer_falls_back_to_discovered_name() {
        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "".to_string(),
        };

        let discovered = DiscoveredPeer {
            peer_id: "peer-1".to_string(),
            device_name: Some("Discovered Name".to_string()),
            device_id: None,
            addresses: vec![],
            discovered_at: Utc::now(),
            last_seen: Utc::now(),
            is_paired: true,
        };

        let result = map_paired_device_to_peer(device, Some(&discovered), false);

        assert_eq!(result.device_name, "Discovered Name");
        assert!(!result.connected);
    }

    #[test]
    fn test_map_paired_device_to_peer_falls_back_to_unknown_device() {
        let device = PairedDevice {
            peer_id: PeerId::from("peer-1"),
            pairing_state: PairingState::Trusted,
            identity_fingerprint: "fp".to_string(),
            paired_at: Utc::now(),
            last_seen_at: None,
            device_name: "".to_string(),
        };

        let result = map_paired_device_to_peer(device, None, false);

        assert_eq!(result.device_name, "Unknown Device");
    }
}
