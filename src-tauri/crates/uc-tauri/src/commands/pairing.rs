//! Pairing-related Tauri commands
//! 配对相关的 Tauri 命令

use crate::bootstrap::AppRuntime;
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tracing::{error, info_span, warn, Instrument};
use uc_app::usecases::LocalDeviceInfo;
use uc_core::network::PairingState;
use uc_core::PeerId;
use uc_daemon_client::{
    http::{DaemonPairingClient, DaemonPairingRequestError, DaemonQueryClient},
    DaemonConnectionState,
};
use uc_platform::ports::observability::TraceMetadata;

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

#[derive(Debug, Clone)]
struct PairingCommandErrorContext {
    code: String,
    message: String,
    user_message: String,
}

fn map_daemon_paired_device_to_peer(device: uc_daemon::api::types::PairedDeviceDto) -> PairedPeer {
    PairedPeer {
        peer_id: device.peer_id,
        device_name: device.device_name,
        shared_secret: vec![],
        paired_at: String::new(),
        last_seen: device
            .last_seen_at_ms
            .and_then(chrono::DateTime::from_timestamp_millis)
            .map(|timestamp| timestamp.to_rfc3339()),
        last_known_addresses: vec![],
        connected: device.connected,
    }
}

/// List paired devices
/// 列出已配对设备
#[tauri::command]
pub async fn list_paired_devices(
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedPeer>, CommandError> {
    let span = info_span!(
        "command.pairing.list",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let devices = DaemonQueryClient::new(daemon_connection.inner().clone())
            .get_paired_devices()
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to query daemon paired devices");
                CommandError::InternalError(error.to_string())
            })?;
        let peers: Vec<PairedPeer> = devices
            .into_iter()
            .map(map_daemon_paired_device_to_peer)
            .collect();
        Ok(peers)
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_local_peer_id(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, CommandError> {
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
) -> Result<LocalDeviceInfo, CommandError> {
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
                CommandError::InternalError(e.to_string())
            })
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_p2p_peers(
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<P2PPeerInfo>, CommandError> {
    let span = info_span!(
        "command.pairing.get_p2p_peers",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let snapshot = DaemonQueryClient::new(daemon_connection.inner().clone())
            .get_peers()
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to query daemon peers snapshot");
                CommandError::InternalError(error.to_string())
            })?;

        tracing::info!(
            peer_count = snapshot.len(),
            "assembled p2p peer snapshot from daemon query"
        );

        Ok(snapshot
            .into_iter()
            .map(|p| P2PPeerInfo {
                peer_id: p.peer_id,
                device_name: p.device_name,
                addresses: p.addresses,
                is_paired: p.is_paired,
                connected: p.connected,
            })
            .collect())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn get_paired_peers(
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedPeer>, CommandError> {
    get_paired_peers_with_status(daemon_connection, _trace).await
}

#[tauri::command]
pub async fn get_paired_peers_with_status(
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<Vec<PairedPeer>, CommandError> {
    let span = info_span!(
        "command.pairing.get_paired_peers_with_status",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let devices = DaemonQueryClient::new(daemon_connection.inner().clone())
            .get_paired_devices()
            .await
            .map_err(|error| {
                tracing::error!(error = %error, "Failed to query daemon paired devices");
                CommandError::InternalError(error.to_string())
            })?;

        tracing::info!(
            paired_peer_count = devices.len(),
            "assembled paired peers with status from daemon query"
        );

        Ok(devices
            .into_iter()
            .map(map_daemon_paired_device_to_peer)
            .collect())
    }
    .instrument(span)
    .await
}

/// Update pairing state for a peer
/// 更新对等端配对状态
#[tauri::command]
pub async fn set_pairing_state(
    runtime: State<'_, Arc<AppRuntime>>,
    peer_id: String,
    state: PairingState,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
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
                CommandError::InternalError(e.to_string())
            })?;
        Ok(())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn initiate_p2p_pairing(
    request: P2PPairingRequest,
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<P2PPairingResponse, CommandError> {
    let span = info_span!(
        "command.pairing.initiate",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %request.peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        match DaemonPairingClient::new(daemon_connection.inner().clone())
            .initiate_pairing(request.peer_id)
            .await
        {
            Ok(response) => Ok(P2PPairingResponse {
                session_id: response.session_id,
                success: true,
                error: None,
            }),
            Err(error) => {
                let mapped = map_pairing_command_error(&error);
                log_pairing_command_error("initiate_p2p_pairing", &mapped);
                Ok(P2PPairingResponse {
                    session_id: String::new(),
                    success: false,
                    error: Some(mapped.user_message),
                })
            }
        }
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn accept_p2p_pairing(
    session_id: String,
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.pairing.accept",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        DaemonPairingClient::new(daemon_connection.inner().clone())
            .accept_pairing(&session_id)
            .await
            .map_err(|error| {
                let mapped = map_pairing_command_error(&error);
                log_pairing_command_error("accept_p2p_pairing", &mapped);
                CommandError::InternalError(mapped.user_message)
            })?;
        Ok(())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn reject_p2p_pairing(
    session_id: String,
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.pairing.reject",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        DaemonPairingClient::new(daemon_connection.inner().clone())
            .reject_pairing(&session_id)
            .await
            .map_err(|error| {
                let mapped = map_pairing_command_error(&error);
                log_pairing_command_error("reject_p2p_pairing", &mapped);
                CommandError::InternalError(mapped.user_message)
            })?;
        Ok(())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn verify_p2p_pairing_pin(
    request: P2PPinVerifyRequest,
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.pairing.verify_pin",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        session_id = %request.session_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        DaemonPairingClient::new(daemon_connection.inner().clone())
            .verify_pairing(&request.session_id, request.pin_matches)
            .await
            .map_err(|error| {
                let mapped = map_pairing_command_error(&error);
                log_pairing_command_error("verify_p2p_pairing_pin", &mapped);
                CommandError::InternalError(mapped.user_message)
            })?;
        Ok(())
    }
    .instrument(span)
    .await
}

#[tauri::command]
pub async fn unpair_p2p_device(
    peer_id: String,
    daemon_connection: State<'_, DaemonConnectionState>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.pairing.unpair",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        DaemonPairingClient::new(daemon_connection.inner().clone())
            .unpair_device(peer_id.clone())
            .await
            .map_err(|error| {
                tracing::error!(error = %error, peer_id = %peer_id, "Failed to unpair P2P device");
                CommandError::InternalError(error.to_string())
            })
    }
    .instrument(span)
    .await
}

/// Get resolved sync settings for a specific device.
/// Returns per-device overrides if set, otherwise global defaults.
#[tauri::command]
pub async fn get_device_sync_settings(
    runtime: State<'_, Arc<AppRuntime>>,
    peer_id: String,
    _trace: Option<TraceMetadata>,
) -> Result<uc_core::settings::model::SyncSettings, CommandError> {
    let span = info_span!(
        "command.pairing.get_device_sync_settings",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().get_device_sync_settings();
        uc.execute(&PeerId::from(peer_id.as_str()))
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to get device sync settings");
                CommandError::InternalError(e.to_string())
            })
    }
    .instrument(span)
    .await
}

/// Update or clear per-device sync settings.
/// Passing `null` for settings resets to global defaults.
#[tauri::command]
pub async fn update_device_sync_settings(
    runtime: State<'_, Arc<AppRuntime>>,
    peer_id: String,
    settings: Option<uc_core::settings::model::SyncSettings>,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.pairing.update_device_sync_settings",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        peer_id = %peer_id,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().update_device_sync_settings();
        uc.execute(&PeerId::from(peer_id.as_str()), settings)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to update device sync settings");
                CommandError::InternalError(e.to_string())
            })
    }
    .instrument(span)
    .await
}

fn map_pairing_command_error(error: &anyhow::Error) -> PairingCommandErrorContext {
    if let Some(pairing_error) = error.downcast_ref::<DaemonPairingRequestError>() {
        let code = pairing_error
            .code
            .clone()
            .unwrap_or_else(|| "internal".to_string());
        let user_message = match code.as_str() {
            "active_session_exists" => "active pairing session exists".to_string(),
            "no_local_participant" => "no local pairing participant ready".to_string(),
            "host_not_discoverable" => "host not discoverable".to_string(),
            "session_not_found" => "pairing session not found".to_string(),
            _ => pairing_error.message.clone(),
        };
        return PairingCommandErrorContext {
            code,
            message: pairing_error.message.clone(),
            user_message,
        };
    }

    PairingCommandErrorContext {
        code: "internal".to_string(),
        message: error.to_string(),
        user_message: error.to_string(),
    }
}

fn log_pairing_command_error(command: &'static str, mapped: &PairingCommandErrorContext) {
    match mapped.code.as_str() {
        "active_session_exists"
        | "no_local_participant"
        | "host_not_discoverable"
        | "session_not_found" => warn!(
            command,
            code = %mapped.code,
            message = %mapped.message,
            "daemon pairing command returned handled error"
        ),
        _ => error!(
            command,
            code = %mapped.code,
            message = %mapped.message,
            "daemon pairing command failed"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_daemon::api::types::PairedDeviceDto;

    #[test]
    fn map_daemon_paired_device_to_peer_uses_daemon_shape() {
        let result = map_daemon_paired_device_to_peer(PairedDeviceDto {
            peer_id: "peer-1".to_string(),
            device_name: "Peer One".to_string(),
            pairing_state: "Trusted".to_string(),
            last_seen_at_ms: Some(1_704_067_200_000_i64),
            connected: true,
        });

        assert_eq!(result.peer_id, "peer-1");
        assert_eq!(result.device_name, "Peer One");
        assert!(result.shared_secret.is_empty());
        assert_eq!(result.paired_at, "");
        assert_eq!(
            result.last_seen.as_deref(),
            Some("2024-01-01T00:00:00+00:00")
        );
        assert!(result.last_known_addresses.is_empty());
        assert!(result.connected);
    }

    #[test]
    fn map_daemon_paired_device_to_peer_skips_invalid_timestamp() {
        let result = map_daemon_paired_device_to_peer(PairedDeviceDto {
            peer_id: "peer-1".to_string(),
            device_name: "Peer One".to_string(),
            pairing_state: "Trusted".to_string(),
            last_seen_at_ms: Some(i64::MAX),
            connected: false,
        });

        assert_eq!(result.last_seen, None);
    }
}
