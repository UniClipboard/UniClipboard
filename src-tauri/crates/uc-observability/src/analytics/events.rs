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

    /// 每次进程启动都触发一次（不论是否首次安装）。
    ///
    /// 与 [`Event::AppFirstOpen`] 的关系：`AppFirstOpen` 仅首次安装触发一次，
    /// 是 Activation 漏斗的起点；`AppOpened` 每次启动都发，是 PostHog
    /// `$pageview` / `$screen` 的桌面端等价物——DAU / WAU / MAU / 留存曲线
    /// 都依赖这条事件做"今天这个 person 出现过"的口径。
    ///
    /// 由 bootstrap `compose_event_context` 在 `set_global_event_context` 之后、
    /// 与 `AppFirstOpen` 同位置 emit；compose 自身的进程级幂等门控保证每次
    /// 进程启动只 fire 一次（GUI 内拉起 daemon 不重复计数）。
    ///
    /// 不带 properties——所有切片维度（os / app_version / app_channel / 等）
    /// 由 EventContext 自动注入，已覆盖 PostHog 默认 dashboard 所需字段。
    AppOpened,

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
        /// 加入方解析邀请码命中的发现通道（cloud 目录 vs LAN/mDNS）。
        /// 发起方侧无从得知对端走哪条通道，故恒为 `None`。
        discovery_channel: Option<PairingDiscoveryChannel>,
    },

    /// 配对中断或超时。
    PairingFailed {
        method: PairingMethod,
        failure_reason: PairingFailureReason,
    },

    /// 发起方成功签发了一张邀请——本地铸码或目录服务签发，且至少一条
    /// 发现通道已启动。补齐"发码结局"维度：现有 `pairing_*` 漏斗只覆盖
    /// 加入方握手，看不到发码方走了 cloud、本地铸码降级还是 LAN-only。
    PairingInvitationIssued {
        /// 邀请码来源：目录服务签发 vs 本地铸码。`locally_minted` +
        /// `lan_only_mode=false` 即代表 cloud 不可达降级路径。
        code_source: InvitationCodeSource,
        /// 签发时是否处于 LAN-only 模式（区分"用户主动选 LAN-only"与
        /// "cloud 恰好不可达"——两者都产出 `locally_minted`）。
        lan_only_mode: bool,
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

    // —— Mobile Sync ——————————————————————————————————————————————
    /// 一台 iPhone Shortcut 设备登记成功（`register_device::execute` happy path）。
    ///
    /// schema doc §7.6 / §12.2 P1。iPhone Shortcut 集成的启用计数 anchor——
    /// 当前完全 0 信号，仅靠日志推断。
    MobileDeviceRegistered,

    /// iPhone 与桌面之间剪贴板内容实际落地一次（`apply_incoming::execute`
    /// SyncDoc arm 的 `Applied` outcome）。
    ///
    /// **仅 `Applied` 分支 emit**：`Buffered` / `DuplicateSkipped` /
    /// `DecodeFailed` / `Err` 都不触发——`DuplicateSkipped` 已在本机存在
    /// （`ClipboardEntryCaptured` 的 RemotePush 红线同理），重复广播会
    /// 污染 dashboard 频率口径；`Buffered` 是文件两步 PUT 协议的中间态。
    ///
    /// v1 `direction` 恒为 [`Direction::Inbound`]——`GetLatestMobileSyncDoc`
    /// 出站埋点延后到 v2（iPhone 客户端的轮询频率会让 outbound 量级比
    /// inbound 高一个数量级，需要单独评估采样口径）。字段保留 enum 槽位
    /// 是为了 v2 直接扩展（schema doc §8：新增 property 非破坏式）。
    MobileClipboardSynced {
        direction: Direction,
        payload_size_bucket: PayloadSizeBucket,
    },

    /// iPhone Basic Auth 失败（`authenticate_basic::execute` 失败分支）。
    ///
    /// 401 响应对外**不**区分原因（侧信道防御）；telemetry 内部按
    /// [`MobileAuthFailureKind`] 切分，让 dashboard 区分"用户名错"和
    /// "密码错"——这是产品视角的 iPhone 端密码错误率指标。
    MobileAuthFailed { failure_kind: MobileAuthFailureKind },

    // —— Update Lifecycle ————————————————————————————————————————
    /// 一次更新检查完成（成功或失败均 emit）。schema doc §7.8。
    ///
    /// `source = manual` 由 `commands/updater.rs::check_for_update` Tauri command
    /// 自身 emit；`startup` / `scheduled` / `window_show` 全部由 `update_scheduler`
    /// emit——两类 source **绝不** 混用同一调用路径（schema doc §7.8 红线）。
    /// `do_check_for_update` 内部函数不 emit，由 caller 决定 source。
    ///
    /// `failure_kind` 只在 `outcome == Failed` 时填充，其他 outcome 必须为
    /// `None`——`None` 字段在 wire 上完全消失（不发 `null`）。
    UpdateCheckPerformed {
        source: UpdateCheckSource,
        outcome: UpdateCheckOutcome,
        failure_kind: Option<UpdateFailureKind>,
        install_kind: InstallKind,
    },

    /// 一次更新提示投递（已通过同版本去重）。schema doc §7.8。
    ///
    /// 由 `update_scheduler::scheduler::notify_if_new_version` 在调用
    /// `open_or_focus_updater_window`（Sparkle 风格独立窗口）之后 emit。
    /// 历史名 `UpdateNotificationShown` 来自 Phase 4A 的系统通知路径，
    /// 当前实现已切换到弹窗；`delivery_status` 字段被复用：`Sent` 表示
    /// 窗口成功创建，`SendFailed` 表示 `WebviewWindowBuilder::build` 失败，
    /// `PermissionDenied` 在新路径下不会再出现（保留以维持 schema 兼容）。
    ///
    /// `version` 是 updater manifest 返回的版本字符串原文（如 `0.12.0`、
    /// `0.13.0-alpha.1`）。低基数（每 channel 同时只有一个新版本），不需要
    /// bucketize。
    UpdateNotificationShown {
        version: String,
        delivery_status: NotificationDeliveryStatus,
        install_kind: InstallKind,
    },

    /// 用户打开了更新对话框（`UpdateDialog` 或 `PackageManagerUpdateDialog`）。
    /// schema doc §7.8。
    ///
    /// 由前端 `setUpdateDialogOpen(true)` / `setPackageManagerDialogOpen(true)`
    /// 之后通过 `capture_update_ui_event` Tauri command 转送到本事件。
    /// `install_kind` 在 backend 接收时反查 scheduler 缓存注入，前端不需要知道。
    UpdateDialogOpened {
        source: DialogOpenSource,
        phase: UpdatePhase,
        install_kind: InstallKind,
    },

    /// 用户主动放弃了更新对话框（"稍后" / 关闭 / 取消）。schema doc §7.8。
    ///
    /// `phase` 仅 `Available` / `Ready` 两值在产品语义上有效——`Downloading`
    /// 阶段没有用户可触发的 dismiss 入口（下载是自动后台行为）；类型上保留
    /// `UpdatePhase` 三值但运行时不会 emit `Downloading`。
    UpdateDismissed {
        phase: UpdatePhase,
        source: DismissSource,
    },

    /// 用户（或 scheduler 自动）触发了下载或安装动作。schema doc §7.8。
    ///
    /// 由 `commands/updater.rs::download_update` / `install_update` Tauri
    /// command body 在入口处 emit（`source` 字段**未** 引入——`action` 已把
    /// lifecycle stage 表达清楚，详见 schema doc §7.8 落地备注）。`outcome`
    /// 在动作开始时为 `Started`，完成 / 失败 / 取消时再次 emit 一条事件。
    ///
    /// `error_kind` 仅 `outcome == Failed` 时填充，是短标识符（< 32 字符，
    /// 形如 `io_error` / `signature_mismatch`）；**绝不** 含路径、URL、IP 或
    /// 其他可还原用户标识的内容（schema doc §6.1）。
    UpdateActionInvoked {
        action: UpdateAction,
        outcome: UpdateActionOutcome,
        error_kind: Option<String>,
    },
}

impl Event {
    /// 事件名——一旦上线**永不重命名**。
    pub fn name(&self) -> &'static str {
        match self {
            Event::AppFirstOpen => "app_first_open",
            Event::AppOpened => "app_opened",
            Event::SetupStarted { .. } => "setup_started",
            Event::DeviceNameSet { .. } => "device_name_set",
            Event::PairingStarted { .. } => "pairing_started",
            Event::PairingSucceeded { .. } => "pairing_succeeded",
            Event::PairingFailed { .. } => "pairing_failed",
            Event::PairingInvitationIssued { .. } => "pairing_invitation_issued",
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
            Event::MobileDeviceRegistered => "mobile_device_registered",
            Event::MobileClipboardSynced { .. } => "mobile_clipboard_synced",
            Event::MobileAuthFailed { .. } => "mobile_auth_failed",
            Event::UpdateCheckPerformed { .. } => "update_check_performed",
            Event::UpdateNotificationShown { .. } => "update_notification_shown",
            Event::UpdateDialogOpened { .. } => "update_dialog_opened",
            Event::UpdateDismissed { .. } => "update_dismissed",
            Event::UpdateActionInvoked { .. } => "update_action_invoked",
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
            Event::AppOpened => Map::new(),
            Event::SetupStarted { entry } => to_map(json!({ "entry": entry })),
            Event::DeviceNameSet { name_length_bucket } => {
                to_map(json!({ "name_length_bucket": name_length_bucket }))
            }
            Event::PairingStarted { method } => to_map(json!({ "method": method })),
            Event::PairingSucceeded {
                method,
                peer_os,
                duration_ms,
                discovery_channel,
            } => to_map(json!({
                "method": method,
                "peer_os": peer_os,
                "duration_ms": duration_ms,
                "discovery_channel": discovery_channel,
            })),
            Event::PairingFailed {
                method,
                failure_reason,
            } => to_map(json!({
                "method": method,
                "failure_reason": failure_reason,
            })),
            Event::PairingInvitationIssued {
                code_source,
                lan_only_mode,
            } => to_map(json!({
                "code_source": code_source,
                "lan_only_mode": lan_only_mode,
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
            Event::MobileDeviceRegistered => Map::new(),
            Event::MobileClipboardSynced {
                direction,
                payload_size_bucket,
            } => to_map(json!({
                "direction": direction,
                "payload_size_bucket": payload_size_bucket,
            })),
            Event::MobileAuthFailed { failure_kind } => {
                to_map(json!({ "failure_kind": failure_kind }))
            }
            Event::UpdateCheckPerformed {
                source,
                outcome,
                failure_kind,
                install_kind,
            } => {
                let mut m = Map::new();
                m.insert("source".into(), json!(source));
                m.insert("outcome".into(), json!(outcome));
                m.insert("install_kind".into(), json!(install_kind));
                // None 不出现在 wire（schema doc §10.1：PostHog 把 null 当显式清空）。
                if let Some(kind) = failure_kind {
                    m.insert("failure_kind".into(), json!(kind));
                }
                m
            }
            Event::UpdateNotificationShown {
                version,
                delivery_status,
                install_kind,
            } => to_map(json!({
                "version": version,
                "delivery_status": delivery_status,
                "install_kind": install_kind,
            })),
            Event::UpdateDialogOpened {
                source,
                phase,
                install_kind,
            } => to_map(json!({
                "source": source,
                "phase": phase,
                "install_kind": install_kind,
            })),
            Event::UpdateDismissed { phase, source } => to_map(json!({
                "phase": phase,
                "source": source,
            })),
            Event::UpdateActionInvoked {
                action,
                outcome,
                error_kind,
            } => {
                let mut m = Map::new();
                m.insert("action".into(), json!(action));
                m.insert("outcome".into(), json!(outcome));
                if let Some(kind) = error_kind {
                    m.insert("error_kind".into(), json!(kind));
                }
                m
            }
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

/// 邀请码来源（`pairing_invitation_issued` 专用）。
///
/// 按 domain 独立、不跨事件共享——区分发码方签发时的网络可达性：
/// 目录服务签发意味着当时 WAN 可达（跨网加入方也能解析），本地铸码
/// 意味着只有同 LAN 加入方能经 mDNS 解析。schema doc §7.3 domain-specific 原则。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum InvitationCodeSource {
    /// 目录服务（rendezvous）签发。
    DirectoryIssued,
    /// 本地铸码——cloud 不可达降级，或 LAN-only 模式跳过 cloud。
    LocallyMinted,
}

/// 加入方解析邀请码命中的发现通道（`pairing_succeeded.discovery_channel` 专用）。
///
/// 头号指标维度：量化 LAN/mDNS 通道相对 cloud 目录的实际命中占比，
/// 回答"mDNS 首配对通道到底有没有用"。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum PairingDiscoveryChannel {
    /// cloud 目录（rendezvous HTTP）先解析成功。
    Cloud,
    /// LAN（mDNS）先解析成功。
    Lan,
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

/// 移动端鉴权失败原因（`mobile_auth_failed` 专用）。
///
/// 与 `sync` / `pairing` / `unlock` 失败枚举不共享——schema doc §7.3 末尾
/// 的 domain-specific failure enum 原则：不同 domain 的失败语义不重叠，
/// 共享 enum 会让 funnel 跨 domain 误聚合。
///
/// **隐私契约对响应保持统一 401**（避免侧信道枚举哪种凭据存在），
/// telemetry 这一面则按真实成因切分，让 dashboard 能定量回答"用户密码
/// 错"vs"用户名拼错"vs"服务端故障"三类问题。`Internal` 占比 > 5%
/// 视为本机持久化层 / hasher adapter 不稳定。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum MobileAuthFailureKind {
    /// 头解析失败 / base64 损坏 / 用户名不存在——iPhone 端用错了
    /// username 字段。
    UnknownUser,
    /// 用户名命中但密码校验失败，含 PHC 字符串本身损坏的兜底分支
    /// （后者数量为零时不与"真实密码错"区分）。
    PasswordMismatch,
    /// 仓储 / hasher adapter 内部错误（DB I/O 故障、spawn_blocking
    /// join 失败、Argon2 库内部异常等）。
    Internal,
}

// —— Update Lifecycle enums (schema doc §7.8 / §7.9) ————————————————

/// 更新检查的触发来源（`update_check_performed` 专用）。
///
/// **不**与 [`DialogOpenSource`] / [`DismissSource`] 共享——不同 lifecycle
/// 阶段的 source 语义不重叠，跨事件共享 enum 会让 funnel 误聚合
/// （schema doc §7.3 末尾 domain-specific 原则）。
///
/// `Manual` 由 `commands/updater.rs::check_for_update` Tauri command 自身
/// emit；其他三值仅由 `update_scheduler` emit（schema doc §7.8 红线：
/// scheduler-only 的 source 绝不混用同一调用路径）。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckSource {
    /// 进程启动后由 scheduler 第一次检查（曾经的前端启动检查搬到后端）。
    Startup,
    /// 周期性 6h ± 15min jitter 触发。
    Scheduled,
    /// 用户点击设置页"检查更新"按钮 / 命令行调用。
    Manual,
    /// 用户点开主窗口、且距上次任意 source 的 check > 30min 时顺手补查。
    WindowShow,
}

/// 更新检查的结果（`update_check_performed` 专用）。schema doc §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateCheckOutcome {
    /// 检查成功，发现新版本。
    Available,
    /// 检查成功，已是最新。
    UpToDate,
    /// 检查失败，详见 `failure_kind`。
    Failed,
}

/// 更新检查失败原因（`update_check_performed.failure_kind` 专用）。
///
/// 与 `sync` / `pairing` / `unlock` / `mobile_auth` 失败枚举不共享——延续
/// schema doc §7.3 末尾的 domain-specific failure enum 原则。`Other` 占比
/// > 10% 视为 manifest / 网络栈不稳定信号。schema doc §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateFailureKind {
    /// 连接失败 / DNS / TLS 握手。
    Network,
    /// 4xx / 5xx 响应。
    HttpError,
    /// manifest JSON 解析或 minisign 校验失败。
    ParseError,
    /// 其他（含 panic 兜底）。
    Other,
}

/// 更新提示的投递状态（`update_notification_shown` 专用）。schema doc §7.9。
///
/// 历史 schema 来自系统通知路径，当前实现切换到 Sparkle 风格窗口后字段
/// 语义被复用：`Sent` 表示窗口成功打开，`SendFailed` 表示 builder 失败。
/// `PermissionDenied` 在新路径下不会再出现，保留枚举以维持 schema 兼容。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NotificationDeliveryStatus {
    /// 更新提示已对用户可见（窗口已弹出 / 历史上是系统通知已投递）。
    Sent,
    /// 历史路径下 macOS / Windows 用户拒绝通知权限；新窗口路径下不会 emit。
    PermissionDenied,
    /// 投递失败（窗口 builder 失败 / 历史路径下无 notification daemon 等）。
    SendFailed,
}

/// 用户打开更新对话框的入口（`update_dialog_opened` 专用）。schema doc §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DialogOpenSource {
    /// 用户点击系统通知打开。
    Notification,
    /// 用户点击 sidebar 的更新指示器。
    SidebarIcon,
}

/// 更新流程的阶段（`update_dialog_opened.phase` / `update_dismissed.phase`）。
///
/// `update_dismissed` 在产品语义上只会 emit `Available` / `Ready`——
/// `Downloading` 阶段没有用户可触发的 dismiss 入口；类型保留三值让两个事件
/// 共享 enum 简化代码 / dashboard slicing。schema doc §7.8 / §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdatePhase {
    /// 检查到新版本但尚未触发下载。
    Available,
    /// 下载中（仅 `update_dialog_opened` 会出现）。
    Downloading,
    /// 下载完成，等待"安装并重启"确认。
    Ready,
}

/// 用户放弃更新的入口（`update_dismissed.source` 专用）。schema doc §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DismissSource {
    /// 用户点 "稍后" 按钮。
    DialogLater,
    /// X / ESC / 点击对话框外部关闭。
    DialogClosed,
    /// Linux deb/rpm 路径专属（关闭 `PackageManagerUpdateDialog`）。
    PackageManagerDialogClosed,
}

/// 用户触发的更新动作类型（`update_action_invoked.action` 专用）。
///
/// 未引入 `source` 字段——`action` 已表达 lifecycle stage（schema doc §7.8
/// 落地备注）；scheduler 自动下载与用户手动下载共用 `DownloadBg`，caller
/// 类型从 `EventContext.session_id` 与时间序列推断。schema doc §7.9。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateAction {
    /// 用户点 "后台下载" 或 scheduler 自动下载触发。
    DownloadBg,
    /// 用户点 "安装并重启"。
    Install,
}

/// 更新动作的完成态（`update_action_invoked.outcome` 专用）。schema doc §7.9。
///
/// 一次动作生命周期会 emit 至少两条事件：`Started` 入口一次，之后
/// `Succeeded` / `Failed` / `Cancelled` 任一终态一次。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum UpdateActionOutcome {
    /// 动作刚开始（download 或 install 入口被调用）。
    Started,
    /// 终态：成功（仅 download；install 走 Tauri restart 流程，事件 sink
    /// 在 install 触发前已 fire-and-forget，restart 后无 telemetry）。
    Succeeded,
    /// 终态：失败（详见 `error_kind`）。
    Failed,
    /// 终态：用户主动取消。
    Cancelled,
}

/// 桌面端安装来源（多个 update_* 事件共享）。schema doc §7.9。
///
/// **与 `commands/updater.rs::InstallKind` Tauri command 共享 wire 形态**——
/// 后者通过 specta 暴露给前端，wire 形态被前端 API 锁住。本枚举必须维持
/// `macos` / `windows` / `appimage` / `deb` / `rpm` / `unknown` 等价；任何
/// 一侧新增变体必须同步另一侧（schema doc §8 演化策略：新增允许，重命名禁止）。
///
/// `Snap` / `Copr` 等 Linux 包源在 v1 一律归类 `Unknown`——dpkg-query / rpm
/// 不会认领 Snap 路径下的二进制；若 dashboard 显示 `Unknown` 占比 > 10%
/// 再独立拆分。
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum InstallKind {
    /// macOS `.app`（走 Tauri updater）。
    Macos,
    /// Windows `.exe` / `.msi`（走 Tauri updater）。
    Windows,
    /// Linux AppImage（走 Tauri updater）。
    AppImage,
    /// Debian / Ubuntu 包（走 `PackageManagerUpdateDialog` 引导）。
    Deb,
    /// RHEL / Fedora 包。
    Rpm,
    /// probe 失败兜底（含 Snap / COPR / 源码构建等）。
    Unknown,
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
            (Event::AppOpened, "app_opened"),
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
                    discovery_channel: None,
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
                Event::PairingInvitationIssued {
                    code_source: InvitationCodeSource::LocallyMinted,
                    lan_only_mode: true,
                },
                "pairing_invitation_issued",
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
            (Event::MobileDeviceRegistered, "mobile_device_registered"),
            (
                Event::MobileClipboardSynced {
                    direction: Direction::Inbound,
                    payload_size_bucket: PayloadSizeBucket::Lt1Kb,
                },
                "mobile_clipboard_synced",
            ),
            (
                Event::MobileAuthFailed {
                    failure_kind: MobileAuthFailureKind::PasswordMismatch,
                },
                "mobile_auth_failed",
            ),
            (
                Event::UpdateCheckPerformed {
                    source: UpdateCheckSource::Scheduled,
                    outcome: UpdateCheckOutcome::UpToDate,
                    failure_kind: None,
                    install_kind: InstallKind::Macos,
                },
                "update_check_performed",
            ),
            (
                Event::UpdateNotificationShown {
                    version: "0.12.0".into(),
                    delivery_status: NotificationDeliveryStatus::Sent,
                    install_kind: InstallKind::Macos,
                },
                "update_notification_shown",
            ),
            (
                Event::UpdateDialogOpened {
                    source: DialogOpenSource::Notification,
                    phase: UpdatePhase::Available,
                    install_kind: InstallKind::Macos,
                },
                "update_dialog_opened",
            ),
            (
                Event::UpdateDismissed {
                    phase: UpdatePhase::Available,
                    source: DismissSource::DialogLater,
                },
                "update_dismissed",
            ),
            (
                Event::UpdateActionInvoked {
                    action: UpdateAction::DownloadBg,
                    outcome: UpdateActionOutcome::Started,
                    error_kind: None,
                },
                "update_action_invoked",
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
    fn mobile_auth_failure_kind_wire_format() {
        // schema doc §7.6：3 个变体，不含 RateLimited（未实装速率限制）。
        for (kind, expected) in [
            (MobileAuthFailureKind::UnknownUser, "unknown_user"),
            (MobileAuthFailureKind::PasswordMismatch, "password_mismatch"),
            (MobileAuthFailureKind::Internal, "internal"),
        ] {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                expected,
                "MobileAuthFailureKind::{kind:?}"
            );
        }
    }

    #[test]
    fn mobile_clipboard_synced_properties() {
        // v1 direction 恒为 inbound；保留字段为 v2 扩展非破坏式埋点 outbound。
        let event = Event::MobileClipboardSynced {
            direction: Direction::Inbound,
            payload_size_bucket: PayloadSizeBucket::Kb1To100,
        };
        let props = event.properties();
        assert_eq!(props.get("direction"), Some(&json!("inbound")));
        assert_eq!(
            props.get("payload_size_bucket"),
            Some(&json!("1kb_to_100kb"))
        );
        // 没有 payload_type / transport_type / peer_os——这些与 inbound mobile
        // 路径无关，避免误导 dashboard 分析。
        assert!(!props.contains_key("payload_type"));
        assert!(!props.contains_key("transport_type"));
        assert!(!props.contains_key("peer_os"));
    }

    #[test]
    fn app_opened_has_empty_properties() {
        // 所有切片维度都靠 EventContext 提供（os / app_version / app_channel /
        // 等）。事件本身不带字段：避免后续误以为可以塞进 properties 而破坏
        // 与 PostHog `$pageview` 等价物的最小契约。
        let props = Event::AppOpened.properties();
        assert!(props.is_empty(), "{props:?}");
    }

    #[test]
    fn mobile_device_registered_has_empty_properties() {
        // 仅靠 EventContext 携带身份维度；事件 properties 为空。
        let props = Event::MobileDeviceRegistered.properties();
        assert!(props.is_empty(), "{props:?}");
    }

    #[test]
    fn mobile_auth_failed_carries_failure_kind() {
        let event = Event::MobileAuthFailed {
            failure_kind: MobileAuthFailureKind::UnknownUser,
        };
        let props = event.properties();
        assert_eq!(props.get("failure_kind"), Some(&json!("unknown_user")));
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
    fn pairing_channel_enums_wire_format() {
        // 上线后钉死，任何变更 = 破坏向后兼容。
        assert_eq!(
            serde_json::to_value(InvitationCodeSource::DirectoryIssued).unwrap(),
            "directory_issued"
        );
        assert_eq!(
            serde_json::to_value(InvitationCodeSource::LocallyMinted).unwrap(),
            "locally_minted"
        );
        assert_eq!(
            serde_json::to_value(PairingDiscoveryChannel::Cloud).unwrap(),
            "cloud"
        );
        assert_eq!(
            serde_json::to_value(PairingDiscoveryChannel::Lan).unwrap(),
            "lan"
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
            discovery_channel: Some(PairingDiscoveryChannel::Lan),
        };
        let props = event.properties();
        assert!(!props.contains_key("anonymous_user_id"));
        assert!(!props.contains_key("analytics_device_id"));
        assert!(!props.contains_key("session_id"));
        assert!(!props.contains_key("app_version"));
        assert!(!props.contains_key("os"));
    }

    // —— Update Lifecycle wire 形态 ————————————————————————————————

    #[test]
    fn update_lifecycle_enums_wire_format() {
        // schema doc §7.9 钉死的全部取值。任何变更 = 破坏向后兼容。
        for (val, expected) in [
            (UpdateCheckSource::Startup, "startup"),
            (UpdateCheckSource::Scheduled, "scheduled"),
            (UpdateCheckSource::Manual, "manual"),
            (UpdateCheckSource::WindowShow, "window_show"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdateCheckSource::{val:?}"
            );
        }

        for (val, expected) in [
            (UpdateCheckOutcome::Available, "available"),
            (UpdateCheckOutcome::UpToDate, "up_to_date"),
            (UpdateCheckOutcome::Failed, "failed"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdateCheckOutcome::{val:?}"
            );
        }

        for (val, expected) in [
            (UpdateFailureKind::Network, "network"),
            (UpdateFailureKind::HttpError, "http_error"),
            (UpdateFailureKind::ParseError, "parse_error"),
            (UpdateFailureKind::Other, "other"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdateFailureKind::{val:?}"
            );
        }

        for (val, expected) in [
            (NotificationDeliveryStatus::Sent, "sent"),
            (
                NotificationDeliveryStatus::PermissionDenied,
                "permission_denied",
            ),
            (NotificationDeliveryStatus::SendFailed, "send_failed"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "NotificationDeliveryStatus::{val:?}"
            );
        }

        for (val, expected) in [
            (DialogOpenSource::Notification, "notification"),
            (DialogOpenSource::SidebarIcon, "sidebar_icon"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "DialogOpenSource::{val:?}"
            );
        }

        for (val, expected) in [
            (UpdatePhase::Available, "available"),
            (UpdatePhase::Downloading, "downloading"),
            (UpdatePhase::Ready, "ready"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdatePhase::{val:?}"
            );
        }

        for (val, expected) in [
            (DismissSource::DialogLater, "dialog_later"),
            (DismissSource::DialogClosed, "dialog_closed"),
            (
                DismissSource::PackageManagerDialogClosed,
                "package_manager_dialog_closed",
            ),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "DismissSource::{val:?}"
            );
        }

        for (val, expected) in [
            (UpdateAction::DownloadBg, "download_bg"),
            (UpdateAction::Install, "install"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdateAction::{val:?}"
            );
        }

        for (val, expected) in [
            (UpdateActionOutcome::Started, "started"),
            (UpdateActionOutcome::Succeeded, "succeeded"),
            (UpdateActionOutcome::Failed, "failed"),
            (UpdateActionOutcome::Cancelled, "cancelled"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "UpdateActionOutcome::{val:?}"
            );
        }

        // InstallKind 与 commands/updater.rs 的 wire 形态等价（schema doc §7.9）。
        for (val, expected) in [
            (InstallKind::Macos, "macos"),
            (InstallKind::Windows, "windows"),
            (InstallKind::AppImage, "appimage"),
            (InstallKind::Deb, "deb"),
            (InstallKind::Rpm, "rpm"),
            (InstallKind::Unknown, "unknown"),
        ] {
            assert_eq!(
                serde_json::to_value(val).unwrap(),
                expected,
                "InstallKind::{val:?}"
            );
        }
    }

    #[test]
    fn update_check_performed_omits_failure_kind_on_success() {
        // outcome != Failed 时 failure_kind 必须从 wire 完全消失，避免 PostHog
        // 把 null 当显式清空指令（schema doc §10.1）。
        let event = Event::UpdateCheckPerformed {
            source: UpdateCheckSource::Scheduled,
            outcome: UpdateCheckOutcome::UpToDate,
            failure_kind: None,
            install_kind: InstallKind::Windows,
        };
        let props = event.properties();
        assert_eq!(props.get("source"), Some(&json!("scheduled")));
        assert_eq!(props.get("outcome"), Some(&json!("up_to_date")));
        assert_eq!(props.get("install_kind"), Some(&json!("windows")));
        assert!(!props.contains_key("failure_kind"));
    }

    #[test]
    fn update_check_performed_carries_failure_kind_on_failure() {
        let event = Event::UpdateCheckPerformed {
            source: UpdateCheckSource::Startup,
            outcome: UpdateCheckOutcome::Failed,
            failure_kind: Some(UpdateFailureKind::Network),
            install_kind: InstallKind::AppImage,
        };
        let props = event.properties();
        assert_eq!(props.get("source"), Some(&json!("startup")));
        assert_eq!(props.get("outcome"), Some(&json!("failed")));
        assert_eq!(props.get("failure_kind"), Some(&json!("network")));
        assert_eq!(props.get("install_kind"), Some(&json!("appimage")));
    }

    #[test]
    fn update_notification_shown_properties() {
        let event = Event::UpdateNotificationShown {
            version: "0.13.0-alpha.1".into(),
            delivery_status: NotificationDeliveryStatus::PermissionDenied,
            install_kind: InstallKind::Macos,
        };
        let props = event.properties();
        assert_eq!(props.get("version"), Some(&json!("0.13.0-alpha.1")));
        assert_eq!(
            props.get("delivery_status"),
            Some(&json!("permission_denied"))
        );
        assert_eq!(props.get("install_kind"), Some(&json!("macos")));
    }

    #[test]
    fn update_dialog_opened_properties() {
        let event = Event::UpdateDialogOpened {
            source: DialogOpenSource::SidebarIcon,
            phase: UpdatePhase::Ready,
            install_kind: InstallKind::Deb,
        };
        let props = event.properties();
        assert_eq!(props.get("source"), Some(&json!("sidebar_icon")));
        assert_eq!(props.get("phase"), Some(&json!("ready")));
        assert_eq!(props.get("install_kind"), Some(&json!("deb")));
    }

    #[test]
    fn update_dismissed_properties() {
        let event = Event::UpdateDismissed {
            phase: UpdatePhase::Available,
            source: DismissSource::PackageManagerDialogClosed,
        };
        let props = event.properties();
        assert_eq!(props.get("phase"), Some(&json!("available")));
        assert_eq!(
            props.get("source"),
            Some(&json!("package_manager_dialog_closed"))
        );
        // install_kind 不在 dismiss 事件里——schema doc §7.8 没列。
        assert!(!props.contains_key("install_kind"));
    }

    #[test]
    fn update_action_invoked_omits_error_kind_on_success() {
        let event = Event::UpdateActionInvoked {
            action: UpdateAction::Install,
            outcome: UpdateActionOutcome::Started,
            error_kind: None,
        };
        let props = event.properties();
        assert_eq!(props.get("action"), Some(&json!("install")));
        assert_eq!(props.get("outcome"), Some(&json!("started")));
        assert!(!props.contains_key("error_kind"));
    }

    #[test]
    fn update_action_invoked_carries_error_kind_on_failure() {
        let event = Event::UpdateActionInvoked {
            action: UpdateAction::DownloadBg,
            outcome: UpdateActionOutcome::Failed,
            error_kind: Some("io_error".into()),
        };
        let props = event.properties();
        assert_eq!(props.get("action"), Some(&json!("download_bg")));
        assert_eq!(props.get("outcome"), Some(&json!("failed")));
        assert_eq!(props.get("error_kind"), Some(&json!("io_error")));
    }

    #[test]
    fn update_events_have_no_context_fields() {
        // 与 properties_are_pure_event_fields_only 同一红线，针对 update 域
        // 单独 cover——避免 EventContext 字段被误塞进 update 事件 properties。
        let event = Event::UpdateCheckPerformed {
            source: UpdateCheckSource::WindowShow,
            outcome: UpdateCheckOutcome::Available,
            failure_kind: None,
            install_kind: InstallKind::Rpm,
        };
        let props = event.properties();
        assert!(!props.contains_key("anonymous_user_id"));
        assert!(!props.contains_key("app_version"));
        assert!(!props.contains_key("os"));
        assert!(!props.contains_key("session_id"));
    }
}
