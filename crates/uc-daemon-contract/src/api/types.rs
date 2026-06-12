//! HTTP and WebSocket DTOs for the daemon transport layer.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use uc_core::file_transfer::FileTransferDirection;
use utoipa::ToSchema;

/// Daemon residency mode reported in the health/status handshake (ADR-008 P5-L L1).
///
/// Wire values (camelCase, to match the `HealthResponse`/`StatusResponse` field
/// naming these enums travel inside): `"standalone" | "serverHeadless" |
/// "oneshot"`. The wire enum is defined HERE in the contract — it deliberately
/// does NOT depend on `uc-daemon`'s internal `DaemonRunMode`; the producer maps
/// `DaemonRunMode -> DaemonResidency` at the daemon/webserver boundary.
///
/// Consumers (CLI L2 version-check, future R8-F2 takeover) read this to learn
/// whether the daemon they are talking to is a persistent member node
/// (`Standalone`/`ServerHeadless`) or a transient `Oneshot` that a persistent
/// client may later take over. As of L1 the CLI/GUI do NOT act on this field.
///
/// Backward-tolerant: the field carries `#[serde(default)]`, so an OLDER
/// daemon body that omits `residency` decodes to [`Self::Standalone`], and a
/// NEWER body's `residency` is simply ignored by an older client. New variants
/// must be added at the END so existing clients keep deserializing known values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema, Default)]
#[serde(rename_all = "camelCase")]
pub enum DaemonResidency {
    /// Persistent member node — the safe, most-common production mode and the
    /// only one reachable today. Chosen as the `#[default]` so a missing field
    /// (older daemon) decodes to a non-null, takeover-safe value.
    #[default]
    Standalone,
    /// Persistent headless server node (no system clipboard); still a member
    /// node from a takeover standpoint, never an `Oneshot`.
    ServerHeadless,
    /// Transient one-shot daemon (ADR-008 P5-L). A persistent client detecting
    /// this may later take it over (R8-F2). Inert/unreachable as of L1.
    Oneshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct HealthResponse {
    pub status: String,
    pub package_version: String,
    pub api_revision: String,
    /// Daemon residency mode (ADR-008 P5-L L1). Backward-tolerant via
    /// `#[serde(default)]` — absent on older daemons, decodes to
    /// [`DaemonResidency::Standalone`].
    #[serde(default)]
    pub residency: DaemonResidency,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct StatusResponse {
    pub package_version: String,
    pub api_revision: String,
    pub uptime_seconds: u64,
    pub workers: Vec<WorkerStatusDto>,
    /// Daemon residency mode (ADR-008 P5-L L1). Backward-tolerant via
    /// `#[serde(default)]` — absent on older daemons, decodes to
    /// [`DaemonResidency::Standalone`].
    #[serde(default)]
    pub residency: DaemonResidency,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkerStatusDto {
    pub name: String,
    pub health: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PeerSnapshotDto {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub addresses: Vec<String>,
    pub is_paired: bool,
    pub connected: bool,
    pub pairing_state: String,
    /// Phase 96 INDIC-01:连接通道 4 态 wire 字符串。
    /// 取值严格限定 `"direct" | "relay" | "offline" | "unknown"`,
    /// 由 application 层 `connection_channel_to_wire` 单点产出,
    /// 前端按字符串模式匹配渲染徽章。**禁止**新增枚举值或缩写;
    /// "Out of LAN" 灰态由前端基于 `channel + LAN-only setting`
    /// 合成,不在 wire 协议里。
    pub channel: String,
    /// 当前活跃连接地址。直连时为对端 IP:port,中转时为 relay 地址。
    pub connection_address: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMemberDto {
    pub peer_id: String,
    pub device_name: String,
    pub pairing_state: String,
    pub last_seen_at_ms: Option<i64>,
    pub connected: bool,
    /// Phase 96 INDIC-01:连接通道 4 态 wire 字符串。同 `PeerSnapshotDto.channel`,
    /// 取值严格限定 `"direct" | "relay" | "offline" | "unknown"`。前端
    /// `SpaceMember` 直接消费,`ConnectionChannelBadge` 渲染。
    pub channel: String,
    /// 当前活跃连接地址。直连时为对端 IP:port,中转时为 relay 地址。
    pub connection_address: Option<String>,
}

/// Result of a `POST /presence/refresh` round.
///
/// 主动 probe 一轮 `ensure_reachable_all` 后的统计回执。UI 不靠这里直接判定
/// 在线状态：probe 过程中各设备的 Online/Offline 变化会通过既有
/// `peers.changed` WebSocket 链路推送，前端再走 `GET /paired-devices`
/// 重拉。该响应只用于调用方显示进度或排障。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct PresenceRefreshResponse {
    pub total: u32,
    pub online: u32,
    pub offline: u32,
    pub errors: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileTransferProgressPayload {
    pub transfer_id: String,
    pub entry_id: Option<String>,
    pub peer_id: String,
    pub direction: FileTransferDirection,
    pub bytes_transferred: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingSessionChangedPayload {
    pub session_id: String,
    pub state: String,
    pub stage: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub updated_at_ms: i64,
    pub ts: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingVerificationPayload {
    pub session_id: String,
    pub kind: String,
    pub peer_id: Option<String>,
    pub device_name: Option<String>,
    pub code: Option<String>,
    pub error: Option<String>,
    pub local_fingerprint: Option<String>,
    pub peer_fingerprint: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PairingFailurePayload {
    pub session_id: String,
    pub peer_id: Option<String>,
    pub error: String,
    pub reason: String,
}

/// Full-snapshot payload for `peers.changed` events.
/// Carries the complete current peer list so the frontend can replace its state atomically.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeersChangedFullPayload {
    pub peers: Vec<PeerSnapshotDto>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerNameUpdatedPayload {
    pub peer_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeerConnectionChangedPayload {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SpaceMembersChangedPayload {
    pub peer_id: String,
    pub device_name: Option<String>,
    pub connected: bool,
}

/// Response payload for GET /lifecycle/status.
/// Mirrors the frontend LifecycleStatusDto shape so the HTTP endpoint
/// can replace the Tauri get_lifecycle_status command without frontend type changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct LifecycleStatusResponse {
    /// Current lifecycle state.
    pub state: String,
}

/// POST /lifecycle/restart request body (ADR-008 P5-L L8d-1). `targetMode` is the
/// residency the successor daemon should launch in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartRequest {
    pub target_mode: DaemonResidency,
}

/// POST /lifecycle/restart 202 ACCEPTED body (ADR-008 P5-L L8d-1). Echoes the
/// locked-in `generation` + `targetMode` so the requester can correlate the
/// accepted restart with the eventual handover record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct RestartAccepted {
    pub generation: u64,
    pub target_mode: DaemonResidency,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonWsSubscribeRequest {
    pub action: String,
    pub topics: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
pub struct DaemonWsEvent {
    pub topic: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub session_id: Option<String>,
    pub ts: i64,
    #[schema(value_type = Object)]
    pub payload: Value,
}

pub use crate::api::dto::device::LocalDeviceInfoDto;
pub use crate::api::dto::pairing::PairingSessionSummaryDto;

// Re-export setup DTOs for backward compatibility with internal consumers.
pub use crate::api::dto::setup::{
    GetSetupStateResponse, SetupActionResponse, SetupResetResponse, SetupSelectPeerRequest,
    SetupStateResponse, SetupStateResponseDto, SetupSubmitPassphraseRequest,
};

#[cfg(test)]
mod residency_backward_tolerance_tests {
    use super::*;

    /// (a) OLD body -> NEW struct: a `HealthResponse` JSON emitted by a daemon
    /// that predates ADR-008 P5-L L1 carries NO `residency` field. The new
    /// struct must still deserialize, defaulting to
    /// [`DaemonResidency::Standalone`] via `#[serde(default)]`.
    #[test]
    fn health_body_without_residency_deserializes_to_standalone_default() {
        let old_body = serde_json::json!({
            "status": "ok",
            "packageVersion": "0.14.0",
            "apiRevision": "some-older-revision",
        });
        let parsed: HealthResponse = serde_json::from_value(old_body).unwrap();
        assert_eq!(parsed.residency, DaemonResidency::Standalone);
        assert_eq!(parsed.status, "ok");
    }

    /// Same backward-tolerance for `StatusResponse`.
    #[test]
    fn status_body_without_residency_deserializes_to_standalone_default() {
        let old_body = serde_json::json!({
            "packageVersion": "0.14.0",
            "apiRevision": "some-older-revision",
            "uptimeSeconds": 42,
            "workers": [],
        });
        let parsed: StatusResponse = serde_json::from_value(old_body).unwrap();
        assert_eq!(parsed.residency, DaemonResidency::Standalone);
        assert_eq!(parsed.uptime_seconds, 42);
    }

    /// (b) NEW body -> OLD decoder: the new struct serializes a `residency`
    /// field that an OLDER decoder (a struct shape that predates the field)
    /// simply ignores. serde drops unknown fields by default, so decoding a
    /// fresh `HealthResponse` body into the legacy shape must still succeed.
    #[test]
    fn new_health_body_residency_is_ignored_by_older_decoder() {
        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyHealthResponse {
            status: String,
            package_version: String,
            api_revision: String,
        }

        let new_body = serde_json::to_value(HealthResponse {
            status: "ok".to_string(),
            package_version: "0.14.0".to_string(),
            api_revision: "residency-revision".to_string(),
            residency: DaemonResidency::Oneshot,
        })
        .unwrap();
        // The new wire body MUST carry the camelCase residency value so newer
        // clients can read it.
        assert_eq!(new_body["residency"], "oneshot");

        // The older decoder ignores the unknown `residency` field and still
        // decodes every field it knows about.
        let legacy: LegacyHealthResponse = serde_json::from_value(new_body).unwrap();
        assert_eq!(legacy.status, "ok");
        assert_eq!(legacy.package_version, "0.14.0");
        assert_eq!(legacy.api_revision, "residency-revision");
    }

    /// Residency wire values are camelCase and round-trip stably for every
    /// variant — pins the externally-observable strings consumers match on.
    #[test]
    fn residency_variants_round_trip_camel_case() {
        for (variant, wire) in [
            (DaemonResidency::Standalone, "standalone"),
            (DaemonResidency::ServerHeadless, "serverHeadless"),
            (DaemonResidency::Oneshot, "oneshot"),
        ] {
            let json = serde_json::to_value(variant).unwrap();
            assert_eq!(json, serde_json::Value::String(wire.to_string()));
            let back: DaemonResidency = serde_json::from_value(json).unwrap();
            assert_eq!(back, variant);
        }
    }

    /// The `#[default]` is `Standalone` — the takeover-safe, most-common
    /// production value a missing field decodes to.
    #[test]
    fn residency_default_is_standalone() {
        assert_eq!(DaemonResidency::default(), DaemonResidency::Standalone);
    }
}
