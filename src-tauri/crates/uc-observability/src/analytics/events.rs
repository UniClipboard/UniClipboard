//! 事件类型与可枚举 properties。
//!
//! 与 schema doc §5 / §6.3 / §7 对应。本模块承担：
//!
//! - `Event` 枚举：v1 优先实现 Activation + Reliability，其它分类在后续
//!   slice 增量扩展。
//! - 区间化类型（`PayloadSizeBucket` / `LatencyBucket`）：把精确值落到
//!   预定义区间，避免泄露内容大小尾差。
//! - properties 提取：[`Event::name`] 与 [`Event::properties`] 把事件转成
//!   `(name, json properties)` 对，sink 直接交给后端 SDK。
//!
//! ## 不可重命名约束
//!
//! 事件名与 property 取值一旦上线**永不重命名**——schema doc §5.3 与 §8。
//! 本文件的测试把当前 wire 形态钉死，CI 守住向后兼容。

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::context::Os;

/// 事件枚举。每条事件对应一个 [`Event::name`] 字符串与一组 properties。
///
/// 新增事件 = 新增 variant；**禁止**重命名既有 variant 或改变其
/// [`Event::name`] 输出。要演化语义时新建 `*_v2` 变体并在 schema doc
/// 标注前者 deprecated。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    // —— Activation ————————————————————————————————————————————
    /// 进程启动且 `is_first_run == true`。
    AppFirstOpen,

    /// 引导页第一帧渲染。
    SetupStarted { entry: SetupEntry },

    /// 用户提交设备名。设备名原文不上传，仅上报长度区间。
    DeviceNameSet {
        name_length_bucket: NameLengthBucket,
    },

    /// 用户点击配对。
    PairingStarted { method: PairingMethod },

    /// 双端握手完成。
    PairingSucceeded {
        method: PairingMethod,
        peer_os: Option<Os>,
        duration_ms: u32,
    },

    /// 配对中断或超时。
    PairingFailed {
        method: PairingMethod,
        failure_reason: PairingFailureReason,
    },

    /// 首次同步发起。
    FirstClipboardSyncAttempted { direction: Direction },

    /// 首次同步对端确认。
    FirstClipboardSyncSucceeded {
        direction: Direction,
        peer_os: Option<Os>,
        transport_type: TransportType,
        duration_ms: u32,
    },

    /// 首次文件同步成功（文件传输已支持时）。
    FirstFileSyncSucceeded {
        peer_os: Option<Os>,
        transport_type: TransportType,
        payload_size_bucket: PayloadSizeBucket,
    },

    /// 引导流程完成——`SetupStatus.has_completed = true` 落地之后触发。
    ///
    /// Activation 漏斗 anchor：补齐"启动了引导但没走完" vs "走完引导但没配对"
    /// 之间的分桶口径（schema doc §12.1）。
    SetupCompleted {
        /// 同一引导流程内是否完成了配对——区分"独立 Space 用户"和"加入既有 Space 用户"。
        has_paired_in_same_flow: bool,
        /// 从 `SetupStarted` 起的耗时（毫秒）。若未观察到 `SetupStarted`
        /// （例如进程中途崩溃恢复），为 `None`。
        duration_ms_since_setup_started: Option<u32>,
    },

    /// Space 解锁成功——每次 daemon 重启的可靠性 anchor。
    SpaceUnlocked,

    /// Space 解锁失败——passphrase 错误 / keyring 失败等。
    SpaceUnlockFailed { failure_reason: UnlockFailureReason },

    /// 本地剪贴板成功捕获了一条新 entry（outbound 同步链路的源头流量）。
    ///
    /// **必须过滤 `origin = Inbound`**：`apply_inbound` 写本地剪贴板时也会
    /// 触发底层 capture，若不过滤会与入站同步双计、污染 DAU 信号
    /// （schema doc §12.1 红线）。
    ClipboardEntryCaptured {
        origin: CaptureOrigin,
        payload_type: PayloadType,
        payload_size_bucket: PayloadSizeBucket,
    },

    // —— Reliability ————————————————————————————————————————————
    /// 同步发起。
    SyncAttempted(SyncEventProps),
    /// 同步成功。`sync_latency_ms` 必填，`failure_reason` 必空。
    SyncSucceeded(SyncEventProps),
    /// 同步失败。`failure_reason` 必填，`sync_latency_ms` 必空。
    SyncFailed(SyncEventProps),
    /// 同步暂缓。目标在发送前已知不可用，本次不可达不计入失败。
    SyncDeferred(SyncDeferredProps),
}

impl Event {
    /// 事件名——一旦上线**永不重命名**。
    pub fn name(&self) -> &'static str {
        match self {
            Event::AppFirstOpen => "app_first_open",
            Event::SetupStarted { .. } => "setup_started",
            Event::DeviceNameSet { .. } => "device_name_set",
            Event::PairingStarted { .. } => "pairing_started",
            Event::PairingSucceeded { .. } => "pairing_succeeded",
            Event::PairingFailed { .. } => "pairing_failed",
            Event::FirstClipboardSyncAttempted { .. } => "first_clipboard_sync_attempted",
            Event::FirstClipboardSyncSucceeded { .. } => "first_clipboard_sync_succeeded",
            Event::FirstFileSyncSucceeded { .. } => "first_file_sync_succeeded",
            Event::SetupCompleted { .. } => "setup_completed",
            Event::SpaceUnlocked => "space_unlocked",
            Event::SpaceUnlockFailed { .. } => "space_unlock_failed",
            Event::ClipboardEntryCaptured { .. } => "clipboard_entry_captured",
            Event::SyncAttempted(_) => "sync_attempted",
            Event::SyncSucceeded(_) => "sync_succeeded",
            Event::SyncFailed(_) => "sync_failed",
            Event::SyncDeferred(_) => "sync_deferred",
        }
    }

    /// 把事件特有 properties 序列化为 JSON 对象。
    ///
    /// **不**包含 `EventContext` 的字段——sink 在上报前会把 context 与
    /// properties 合并。这样保证：
    ///
    /// 1. 事件类型本身可独立于 sink 测试。
    /// 2. context 字段冲突时由 sink 负责仲裁（properties 永远不会与 context 冲突）。
    pub fn properties(&self) -> Map<String, Value> {
        match self {
            Event::AppFirstOpen => Map::new(),
            Event::SetupStarted { entry } => to_map(json!({ "entry": entry })),
            Event::DeviceNameSet { name_length_bucket } => {
                to_map(json!({ "name_length_bucket": name_length_bucket }))
            }
            Event::PairingStarted { method } => to_map(json!({ "method": method })),
            Event::PairingSucceeded {
                method,
                peer_os,
                duration_ms,
            } => to_map(json!({
                "method": method,
                "peer_os": peer_os,
                "duration_ms": duration_ms,
            })),
            Event::PairingFailed {
                method,
                failure_reason,
            } => to_map(json!({
                "method": method,
                "failure_reason": failure_reason,
            })),
            Event::FirstClipboardSyncAttempted { direction } => {
                to_map(json!({ "direction": direction }))
            }
            Event::FirstClipboardSyncSucceeded {
                direction,
                peer_os,
                transport_type,
                duration_ms,
            } => to_map(json!({
                "direction": direction,
                "peer_os": peer_os,
                "transport_type": transport_type,
                "duration_ms": duration_ms,
            })),
            Event::FirstFileSyncSucceeded {
                peer_os,
                transport_type,
                payload_size_bucket,
            } => to_map(json!({
                "peer_os": peer_os,
                "transport_type": transport_type,
                "payload_size_bucket": payload_size_bucket,
            })),
            Event::SetupCompleted {
                has_paired_in_same_flow,
                duration_ms_since_setup_started,
            } => {
                let mut m = Map::new();
                m.insert(
                    "has_paired_in_same_flow".into(),
                    json!(has_paired_in_same_flow),
                );
                // None 不出现在 wire（schema doc §10.1：PostHog 把 null 当显式
                // 清空指令；Option<u32> 缺失就完全省略字段）。
                if let Some(ms) = duration_ms_since_setup_started {
                    m.insert("duration_ms_since_setup_started".into(), json!(ms));
                }
                m
            }
            Event::SpaceUnlocked => Map::new(),
            Event::SpaceUnlockFailed { failure_reason } => {
                to_map(json!({ "failure_reason": failure_reason }))
            }
            Event::ClipboardEntryCaptured {
                origin,
                payload_type,
                payload_size_bucket,
            } => to_map(json!({
                "origin": origin,
                "payload_type": payload_type,
                "payload_size_bucket": payload_size_bucket,
            })),
            Event::SyncAttempted(p) | Event::SyncSucceeded(p) | Event::SyncFailed(p) => {
                serde_json::to_value(p)
                    .ok()
                    .and_then(|v| v.as_object().cloned())
                    .unwrap_or_default()
            }
            Event::SyncDeferred(p) => serde_json::to_value(p)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default(),
        }
    }
}

fn to_map(value: Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}

/// `sync_*` 三件套共享的 properties 形状。
///
/// 对称约束：
/// - `SyncSucceeded` 必须设置 `sync_latency_ms`，不设 `failure_reason`。
/// - `SyncFailed` 必须设置 `failure_reason` / `failure_stage`，不设
///   `sync_latency_ms`。
/// - `SyncAttempted` 两者都不设。
///
/// 这些约束在 v1 不靠类型系统强制——`Event::properties` 直接序列化整个
/// 结构体，越界字段会以 `null` 上报，不会让事件丢失。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncEventProps {
    pub direction: Direction,
    pub payload_type: PayloadType,
    pub payload_size_bucket: PayloadSizeBucket,
    pub transport_type: TransportType,
    /// 已知则填，未知则 `None`——不要因为缺失就丢事件。
    pub peer_os: Option<Os>,
    /// 仅 `SyncSucceeded` 携带，单位毫秒。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync_latency_ms: Option<u32>,
    /// 仅 `SyncFailed` 携带。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_reason: Option<FailureReason>,
    /// 仅 `SyncFailed` 携带。用于把本地策略拒绝、即时发送失败和后续终态失败
    /// 分桶，避免 dashboard 把所有失败混成一个口径。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_stage: Option<SyncFailureStage>,
}

/// `sync_deferred` 的 properties。
///
/// 该事件表示"这次不应计入同步尝试失败率"。典型场景：发送前 presence 已经
/// 知道目标设备离线，后续 dispatch 仍然不可达。代码仍可尝试发送以防 presence
/// 过期，但产品分析上这是预期不可用，不是失败。
///
/// 不带 `transport_type`：deferred 时本次根本没有发生真实发送，记录任何
/// transport 都是误导性数据（dashboard 按 transport 切片会得到虚假结论）。
/// 如果未来要标注"原计划的"transport，请单独命名字段以避免混淆。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SyncDeferredProps {
    pub direction: Direction,
    pub payload_type: PayloadType,
    pub payload_size_bucket: PayloadSizeBucket,
    pub peer_os: Option<Os>,
    pub defer_reason: SyncDeferReason,
}

/// 同步暂缓原因（`sync_deferred` 专用）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SyncDeferReason {
    PeerKnownOffline,
}

/// 同步方向。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Outbound,
    Inbound,
}

/// payload 大类。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PayloadType {
    Text,
    Image,
    File,
}

/// 传输路径。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum TransportType {
    Local,
    P2pDirect,
    Relay,
    FallbackCloud,
}

/// 失败原因（`sync_failed` 专用）。`Unknown` 占比是架构债务指标——
/// schema doc §7.3 要求高于 5% 时专门排查并新增枚举值。
///
/// `pairing_failed` 使用独立的 [`PairingFailureReason`]：pairing 与 sync 失败
/// 语义不重叠（pairing 关心 passphrase / sponsor 决断，sync 关心 transport /
/// payload），共享一份 enum 会让 funnel 漏点信号在跨 domain dashboard 中误聚合。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    PeerOffline,
    Timeout,
    PermissionDenied,
    NetworkError,
    FileTooLarge,
    ClipboardPermission,
    EncryptionMismatch,
    Unknown,
}

/// 同步失败发生的阶段（`sync_failed` 专用）。
///
/// `ImmediateSend` 是当前 outbound dispatch 路径最常见的失败阶段，代表一次
/// 即时投递尝试没有完成；它不是最终投递失败。`LocalPolicy` 代表本机策略在
/// 发送前已经确定该 payload 不可投递（例如过大）。`TerminalDelivery` 预留给
/// 后续 pending/retry 耗尽后的最终失败事件。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SyncFailureStage {
    ImmediateSend,
    LocalPolicy,
    TerminalDelivery,
}

/// 配对失败原因（`pairing_failed` 专用）。
///
/// 与 `RedeemPairingInvitationError` 一一映射，使 funnel 分析能直接定位漏点
/// 的具体业务原因。`Internal` 占比 > 5% 同样视为架构债务（持久化层不稳定）。
/// schema doc §7.4。
///
/// `Display` 输出与 `Serialize` 的 wire 形态严格一致（`snake_case`）——
/// `PairingOutcome::Failure` 字段类型用此 enum，下游 subscriber（CLI / GUI）
/// 直接 `format!` 即可拿到稳定标识符。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PairingFailureReason {
    InvitationNotFound,
    InvitationExpired,
    SponsorUnreachable,
    ServiceUnavailable,
    PassphraseMismatch,
    CorruptedKeyMaterial,
    DeviceNameRequired,
    SponsorRejectedInvitation,
    SponsorDeclined,
    SponsorTimedOut,
    SponsorInternal,
    Timeout,
    ConnectionLost,
    Internal,
}

impl PairingFailureReason {
    /// 稳定 `snake_case` 标识符——与 `Serialize` wire 形态等价。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::InvitationNotFound => "invitation_not_found",
            Self::InvitationExpired => "invitation_expired",
            Self::SponsorUnreachable => "sponsor_unreachable",
            Self::ServiceUnavailable => "service_unavailable",
            Self::PassphraseMismatch => "passphrase_mismatch",
            Self::CorruptedKeyMaterial => "corrupted_key_material",
            Self::DeviceNameRequired => "device_name_required",
            Self::SponsorRejectedInvitation => "sponsor_rejected_invitation",
            Self::SponsorDeclined => "sponsor_declined",
            Self::SponsorTimedOut => "sponsor_timed_out",
            Self::SponsorInternal => "sponsor_internal",
            Self::Timeout => "timeout",
            Self::ConnectionLost => "connection_lost",
            Self::Internal => "internal",
        }
    }
}

impl std::fmt::Display for PairingFailureReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 配对方式。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PairingMethod {
    Qr,
    Code,
    Discovery,
}

/// Space 解锁失败原因（`space_unlock_failed` 专用）。
///
/// 与 `sync` / `pairing` 失败枚举不共享——schema doc §7.3 末尾的
/// domain-specific failure enum 原则：每个 domain 的失败语义不重叠，
/// 共享 enum 会让 funnel 跨 domain 误聚合。`Internal` 占比 > 5%
/// 视为本机持久化层不稳定。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UnlockFailureReason {
    PassphraseMismatch,
    KeyringUnavailable,
    KeyslotCorrupted,
    SpaceNotFound,
    Internal,
}

/// 剪贴板捕获来源（`clipboard_entry_captured` 专用）。
///
/// **不**包含 `Inbound`——入站同步路径必须在调用点过滤掉，避免与
/// 入站事件双计（schema doc §12.1 红线）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum CaptureOrigin {
    /// 系统剪贴板 watcher 检测到的变化（用户在本机复制 / 截屏 / 拖文件）。
    SystemWatcher,
    /// 用户在历史面板里点击恢复某条历史条目，重新进入本地剪贴板。
    ManualRestore,
}

/// 引导入口。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum SetupEntry {
    FirstRun,
    Manual,
}

/// 设备名长度区间——设备名原文永不上传。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NameLengthBucket {
    /// `< 8` 字符。
    #[serde(rename = "lt_8")]
    Lt8,
    /// `[8, 16]` 字符。
    #[serde(rename = "8_to_16")]
    Range8To16,
    /// `> 16` 字符。
    #[serde(rename = "gt_16")]
    Gt16,
}

impl NameLengthBucket {
    /// 按字符数（非字节数）落区间。
    pub fn from_char_count(count: usize) -> Self {
        match count {
            0..=7 => Self::Lt8,
            8..=16 => Self::Range8To16,
            _ => Self::Gt16,
        }
    }
}

/// payload 大小区间——精确值不上报。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum PayloadSizeBucket {
    /// `< 1 KiB`。
    #[serde(rename = "lt_1kb")]
    Lt1Kb,
    /// `[1 KiB, 100 KiB)`。
    #[serde(rename = "1kb_to_100kb")]
    Kb1To100,
    /// `[100 KiB, 10 MiB)`。
    #[serde(rename = "100kb_to_10mb")]
    Kb100ToMb10,
    /// `>= 10 MiB`。
    #[serde(rename = "gt_10mb")]
    Gt10Mb,
}

impl PayloadSizeBucket {
    pub fn from_bytes(bytes: u64) -> Self {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * KIB;
        match bytes {
            n if n < KIB => Self::Lt1Kb,
            n if n < 100 * KIB => Self::Kb1To100,
            n if n < 10 * MIB => Self::Kb100ToMb10,
            _ => Self::Gt10Mb,
        }
    }
}

/// latency 区间——精确 `sync_latency_ms` 仍可单独上报，本枚举用于其它
/// 不需要 p95 分析的耗时字段。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum LatencyBucket {
    #[serde(rename = "lt_100ms")]
    Lt100Ms,
    #[serde(rename = "100ms_to_500ms")]
    Ms100To500,
    #[serde(rename = "500ms_to_2s")]
    Ms500To2s,
    #[serde(rename = "2s_to_10s")]
    S2To10,
    #[serde(rename = "gt_10s")]
    Gt10s,
}

impl LatencyBucket {
    pub fn from_ms(ms: u64) -> Self {
        match ms {
            0..=99 => Self::Lt100Ms,
            100..=499 => Self::Ms100To500,
            500..=1_999 => Self::Ms500To2s,
            2_000..=9_999 => Self::S2To10,
            _ => Self::Gt10s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // —— 事件名钉死 ————————————————————————————————————————————

    #[test]
    fn event_names_match_schema_doc_v1() {
        // 与 schema doc §7.1 / §7.2 一一对应。任何变更都意味着破坏向后兼容。
        let cases: &[(Event, &str)] = &[
            (Event::AppFirstOpen, "app_first_open"),
            (
                Event::SetupStarted {
                    entry: SetupEntry::FirstRun,
                },
                "setup_started",
            ),
            (
                Event::DeviceNameSet {
                    name_length_bucket: NameLengthBucket::Lt8,
                },
                "device_name_set",
            ),
            (
                Event::PairingStarted {
                    method: PairingMethod::Qr,
                },
                "pairing_started",
            ),
            (
                Event::PairingSucceeded {
                    method: PairingMethod::Qr,
                    peer_os: None,
                    duration_ms: 0,
                },
                "pairing_succeeded",
            ),
            (
                Event::PairingFailed {
                    method: PairingMethod::Qr,
                    failure_reason: PairingFailureReason::Internal,
                },
                "pairing_failed",
            ),
            (
                Event::FirstClipboardSyncAttempted {
                    direction: Direction::Outbound,
                },
                "first_clipboard_sync_attempted",
            ),
            (
                Event::FirstClipboardSyncSucceeded {
                    direction: Direction::Outbound,
                    peer_os: None,
                    transport_type: TransportType::Local,
                    duration_ms: 0,
                },
                "first_clipboard_sync_succeeded",
            ),
            (
                Event::FirstFileSyncSucceeded {
                    peer_os: None,
                    transport_type: TransportType::Local,
                    payload_size_bucket: PayloadSizeBucket::Lt1Kb,
                },
                "first_file_sync_succeeded",
            ),
            (
                Event::SetupCompleted {
                    has_paired_in_same_flow: false,
                    duration_ms_since_setup_started: None,
                },
                "setup_completed",
            ),
            (Event::SpaceUnlocked, "space_unlocked"),
            (
                Event::SpaceUnlockFailed {
                    failure_reason: UnlockFailureReason::Internal,
                },
                "space_unlock_failed",
            ),
            (
                Event::ClipboardEntryCaptured {
                    origin: CaptureOrigin::SystemWatcher,
                    payload_type: PayloadType::Text,
                    payload_size_bucket: PayloadSizeBucket::Lt1Kb,
                },
                "clipboard_entry_captured",
            ),
        ];
        for (event, expected) in cases {
            assert_eq!(event.name(), *expected, "{event:?}");
        }

        let sample_props = SyncEventProps {
            direction: Direction::Outbound,
            payload_type: PayloadType::Text,
            payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            transport_type: TransportType::Local,
            peer_os: None,
            sync_latency_ms: None,
            failure_reason: None,
            failure_stage: None,
        };
        assert_eq!(
            Event::SyncAttempted(sample_props.clone()).name(),
            "sync_attempted"
        );
        assert_eq!(
            Event::SyncSucceeded(sample_props.clone()).name(),
            "sync_succeeded"
        );
        assert_eq!(Event::SyncFailed(sample_props).name(), "sync_failed");
        assert_eq!(
            Event::SyncDeferred(SyncDeferredProps {
                direction: Direction::Outbound,
                payload_type: PayloadType::Text,
                payload_size_bucket: PayloadSizeBucket::Lt1Kb,
                peer_os: None,
                defer_reason: SyncDeferReason::PeerKnownOffline,
            })
            .name(),
            "sync_deferred"
        );
    }

    // —— 区间边界 ————————————————————————————————————————————

    #[test]
    fn payload_size_bucket_boundaries() {
        assert_eq!(PayloadSizeBucket::from_bytes(0), PayloadSizeBucket::Lt1Kb);
        assert_eq!(
            PayloadSizeBucket::from_bytes(1023),
            PayloadSizeBucket::Lt1Kb
        );
        assert_eq!(
            PayloadSizeBucket::from_bytes(1024),
            PayloadSizeBucket::Kb1To100
        );
        assert_eq!(
            PayloadSizeBucket::from_bytes(100 * 1024 - 1),
            PayloadSizeBucket::Kb1To100
        );
        assert_eq!(
            PayloadSizeBucket::from_bytes(100 * 1024),
            PayloadSizeBucket::Kb100ToMb10
        );
        assert_eq!(
            PayloadSizeBucket::from_bytes(10 * 1024 * 1024 - 1),
            PayloadSizeBucket::Kb100ToMb10
        );
        assert_eq!(
            PayloadSizeBucket::from_bytes(10 * 1024 * 1024),
            PayloadSizeBucket::Gt10Mb
        );
    }

    #[test]
    fn latency_bucket_boundaries() {
        assert_eq!(LatencyBucket::from_ms(0), LatencyBucket::Lt100Ms);
        assert_eq!(LatencyBucket::from_ms(99), LatencyBucket::Lt100Ms);
        assert_eq!(LatencyBucket::from_ms(100), LatencyBucket::Ms100To500);
        assert_eq!(LatencyBucket::from_ms(499), LatencyBucket::Ms100To500);
        assert_eq!(LatencyBucket::from_ms(500), LatencyBucket::Ms500To2s);
        assert_eq!(LatencyBucket::from_ms(1_999), LatencyBucket::Ms500To2s);
        assert_eq!(LatencyBucket::from_ms(2_000), LatencyBucket::S2To10);
        assert_eq!(LatencyBucket::from_ms(9_999), LatencyBucket::S2To10);
        assert_eq!(LatencyBucket::from_ms(10_000), LatencyBucket::Gt10s);
    }

    #[test]
    fn name_length_bucket_boundaries() {
        assert_eq!(NameLengthBucket::from_char_count(0), NameLengthBucket::Lt8);
        assert_eq!(NameLengthBucket::from_char_count(7), NameLengthBucket::Lt8);
        assert_eq!(
            NameLengthBucket::from_char_count(8),
            NameLengthBucket::Range8To16
        );
        assert_eq!(
            NameLengthBucket::from_char_count(16),
            NameLengthBucket::Range8To16
        );
        assert_eq!(
            NameLengthBucket::from_char_count(17),
            NameLengthBucket::Gt16
        );
    }

    // —— wire 形态钉死 ————————————————————————————————————————————

    #[test]
    fn enum_variants_serialize_to_documented_strings() {
        // schema doc §7.2 / §7.3 中明确列出的取值。
        assert_eq!(
            serde_json::to_value(Direction::Outbound).unwrap(),
            "outbound"
        );
        assert_eq!(serde_json::to_value(Direction::Inbound).unwrap(), "inbound");

        assert_eq!(serde_json::to_value(PayloadType::Text).unwrap(), "text");
        assert_eq!(serde_json::to_value(PayloadType::Image).unwrap(), "image");
        assert_eq!(serde_json::to_value(PayloadType::File).unwrap(), "file");

        assert_eq!(serde_json::to_value(TransportType::Local).unwrap(), "local");
        assert_eq!(
            serde_json::to_value(TransportType::P2pDirect).unwrap(),
            "p2p_direct"
        );
        assert_eq!(serde_json::to_value(TransportType::Relay).unwrap(), "relay");
        assert_eq!(
            serde_json::to_value(TransportType::FallbackCloud).unwrap(),
            "fallback_cloud"
        );

        for (reason, expected) in [
            (FailureReason::PeerOffline, "peer_offline"),
            (FailureReason::Timeout, "timeout"),
            (FailureReason::PermissionDenied, "permission_denied"),
            (FailureReason::NetworkError, "network_error"),
            (FailureReason::FileTooLarge, "file_too_large"),
            (FailureReason::ClipboardPermission, "clipboard_permission"),
            (FailureReason::EncryptionMismatch, "encryption_mismatch"),
            (FailureReason::Unknown, "unknown"),
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                expected,
                "FailureReason::{reason:?}"
            );
        }

        for (stage, expected) in [
            (SyncFailureStage::ImmediateSend, "immediate_send"),
            (SyncFailureStage::LocalPolicy, "local_policy"),
            (SyncFailureStage::TerminalDelivery, "terminal_delivery"),
        ] {
            assert_eq!(
                serde_json::to_value(stage).unwrap(),
                expected,
                "SyncFailureStage::{stage:?}"
            );
        }

        assert_eq!(
            serde_json::to_value(SyncDeferReason::PeerKnownOffline).unwrap(),
            "peer_known_offline"
        );
    }

    #[test]
    fn pairing_failure_reason_wire_format() {
        // schema doc §7.4 中明确列出的取值。任何变更 = 破坏向后兼容。
        for (reason, expected) in [
            (
                PairingFailureReason::InvitationNotFound,
                "invitation_not_found",
            ),
            (
                PairingFailureReason::InvitationExpired,
                "invitation_expired",
            ),
            (
                PairingFailureReason::SponsorUnreachable,
                "sponsor_unreachable",
            ),
            (
                PairingFailureReason::ServiceUnavailable,
                "service_unavailable",
            ),
            (
                PairingFailureReason::PassphraseMismatch,
                "passphrase_mismatch",
            ),
            (
                PairingFailureReason::CorruptedKeyMaterial,
                "corrupted_key_material",
            ),
            (
                PairingFailureReason::DeviceNameRequired,
                "device_name_required",
            ),
            (
                PairingFailureReason::SponsorRejectedInvitation,
                "sponsor_rejected_invitation",
            ),
            (PairingFailureReason::SponsorDeclined, "sponsor_declined"),
            (PairingFailureReason::SponsorTimedOut, "sponsor_timed_out"),
            (PairingFailureReason::SponsorInternal, "sponsor_internal"),
            (PairingFailureReason::Timeout, "timeout"),
            (PairingFailureReason::ConnectionLost, "connection_lost"),
            (PairingFailureReason::Internal, "internal"),
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                expected,
                "PairingFailureReason::{reason:?}"
            );
        }
    }

    #[test]
    fn unlock_failure_reason_wire_format() {
        // schema doc §12.1 钉死的 5 个变体。
        for (reason, expected) in [
            (
                UnlockFailureReason::PassphraseMismatch,
                "passphrase_mismatch",
            ),
            (
                UnlockFailureReason::KeyringUnavailable,
                "keyring_unavailable",
            ),
            (UnlockFailureReason::KeyslotCorrupted, "keyslot_corrupted"),
            (UnlockFailureReason::SpaceNotFound, "space_not_found"),
            (UnlockFailureReason::Internal, "internal"),
        ] {
            assert_eq!(
                serde_json::to_value(reason).unwrap(),
                expected,
                "UnlockFailureReason::{reason:?}"
            );
        }
    }

    #[test]
    fn capture_origin_wire_format() {
        // schema doc §12.1：CaptureOrigin 不允许出现 Inbound。
        assert_eq!(
            serde_json::to_value(CaptureOrigin::SystemWatcher).unwrap(),
            "system_watcher"
        );
        assert_eq!(
            serde_json::to_value(CaptureOrigin::ManualRestore).unwrap(),
            "manual_restore"
        );
    }

    #[test]
    fn setup_completed_omits_unknown_duration() {
        // 没有观察到 SetupStarted 时缺省 duration——None 必须从 wire 完全消失，
        // 避免 PostHog 把 null 当显式清空。
        let event = Event::SetupCompleted {
            has_paired_in_same_flow: true,
            duration_ms_since_setup_started: None,
        };
        let props = event.properties();
        assert_eq!(props.get("has_paired_in_same_flow"), Some(&json!(true)));
        assert!(!props.contains_key("duration_ms_since_setup_started"));
    }

    #[test]
    fn setup_completed_serializes_duration_when_present() {
        let event = Event::SetupCompleted {
            has_paired_in_same_flow: false,
            duration_ms_since_setup_started: Some(8_421),
        };
        let props = event.properties();
        assert_eq!(props.get("has_paired_in_same_flow"), Some(&json!(false)));
        assert_eq!(
            props.get("duration_ms_since_setup_started"),
            Some(&json!(8_421))
        );
    }

    #[test]
    fn clipboard_entry_captured_properties() {
        let event = Event::ClipboardEntryCaptured {
            origin: CaptureOrigin::SystemWatcher,
            payload_type: PayloadType::Image,
            payload_size_bucket: PayloadSizeBucket::Kb100ToMb10,
        };
        let props = event.properties();
        assert_eq!(props.get("origin"), Some(&json!("system_watcher")));
        assert_eq!(props.get("payload_type"), Some(&json!("image")));
        assert_eq!(
            props.get("payload_size_bucket"),
            Some(&json!("100kb_to_10mb"))
        );
    }

    #[test]
    fn space_unlock_failed_carries_reason() {
        let event = Event::SpaceUnlockFailed {
            failure_reason: UnlockFailureReason::PassphraseMismatch,
        };
        let props = event.properties();
        assert_eq!(
            props.get("failure_reason"),
            Some(&json!("passphrase_mismatch"))
        );
    }

    #[test]
    fn pairing_method_wire_format() {
        // schema doc §7.1 列出的取值。
        assert_eq!(serde_json::to_value(PairingMethod::Qr).unwrap(), "qr");
        assert_eq!(serde_json::to_value(PairingMethod::Code).unwrap(), "code");
        assert_eq!(
            serde_json::to_value(PairingMethod::Discovery).unwrap(),
            "discovery"
        );
    }

    #[test]
    fn payload_size_bucket_wire_format() {
        assert_eq!(
            serde_json::to_value(PayloadSizeBucket::Lt1Kb).unwrap(),
            "lt_1kb"
        );
        assert_eq!(
            serde_json::to_value(PayloadSizeBucket::Kb1To100).unwrap(),
            "1kb_to_100kb"
        );
        assert_eq!(
            serde_json::to_value(PayloadSizeBucket::Kb100ToMb10).unwrap(),
            "100kb_to_10mb"
        );
        assert_eq!(
            serde_json::to_value(PayloadSizeBucket::Gt10Mb).unwrap(),
            "gt_10mb"
        );
    }

    // —— properties 形状 ————————————————————————————————————————

    #[test]
    fn sync_succeeded_omits_failure_reason() {
        let event = Event::SyncSucceeded(SyncEventProps {
            direction: Direction::Outbound,
            payload_type: PayloadType::Text,
            payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            transport_type: TransportType::Local,
            peer_os: Some(Os::Macos),
            sync_latency_ms: Some(42),
            failure_reason: None,
            failure_stage: None,
        });
        let props = event.properties();
        assert_eq!(props.get("sync_latency_ms"), Some(&json!(42)));
        // None 字段必须从 wire 完全消失，避免 PostHog 误判为"显式 null"。
        assert!(!props.contains_key("failure_reason"));
        assert!(!props.contains_key("failure_stage"));
    }

    #[test]
    fn sync_failed_marks_failure_sampling_scope() {
        let event = Event::SyncFailed(SyncEventProps {
            direction: Direction::Inbound,
            payload_type: PayloadType::Image,
            payload_size_bucket: PayloadSizeBucket::Kb100ToMb10,
            transport_type: TransportType::Relay,
            peer_os: None,
            sync_latency_ms: None,
            failure_reason: Some(FailureReason::Timeout),
            failure_stage: Some(SyncFailureStage::ImmediateSend),
        });
        let props = event.properties();
        assert_eq!(props.get("failure_reason"), Some(&json!("timeout")));
        assert_eq!(props.get("failure_stage"), Some(&json!("immediate_send")));
        assert!(!props.contains_key("sync_latency_ms"));
    }

    #[test]
    fn sync_deferred_uses_non_failure_reason_field() {
        let event = Event::SyncDeferred(SyncDeferredProps {
            direction: Direction::Outbound,
            payload_type: PayloadType::Text,
            payload_size_bucket: PayloadSizeBucket::Lt1Kb,
            peer_os: None,
            defer_reason: SyncDeferReason::PeerKnownOffline,
        });
        let props = event.properties();
        assert_eq!(
            props.get("defer_reason"),
            Some(&json!("peer_known_offline"))
        );
        assert!(!props.contains_key("failure_reason"));
        assert!(!props.contains_key("failure_stage"));
        assert!(!props.contains_key("sync_latency_ms"));
        // deferred 没有实际发送，不应携带 transport_type，避免 dashboard 误读。
        assert!(!props.contains_key("transport_type"));
    }

    #[test]
    fn properties_are_pure_event_fields_only() {
        // EventContext 字段一律不出现在 properties 里——是 sink 的职责。
        let event = Event::PairingSucceeded {
            method: PairingMethod::Qr,
            peer_os: Some(Os::Windows),
            duration_ms: 1200,
        };
        let props = event.properties();
        assert!(!props.contains_key("anonymous_user_id"));
        assert!(!props.contains_key("analytics_device_id"));
        assert!(!props.contains_key("session_id"));
        assert!(!props.contains_key("app_version"));
        assert!(!props.contains_key("os"));
    }
}
