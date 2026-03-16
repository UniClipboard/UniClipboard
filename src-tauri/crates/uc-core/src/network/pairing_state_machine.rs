//! Pairing protocol state machine
//!
//! 这个模块实现了设备配对的显式状态机,用于审计和可追溯的配对流程。
//!
//! # Design Principles / 设计原则
//!
//! - **显式状态**: 所有关键步骤都有明确状态,包括"等待用户确认""持久化中"等
//! - **审计友好**: 每次状态转换都记录旧状态、事件、新状态和会话ID
//! - **角色对称**: Initiator 和 Responder 使用同一状态机,通过 PairingRole 区分
//! - **可测试**: 纯函数式状态转换 `(state, event) -> (new_state, actions[])`
//!
//! # Architecture / 架构
//!
//! ```text
//! PairingStateMachine (uc-core)
//!   ├── State: 配对流程的当前状态
//!   ├── Event: 触发状态转换的事件
//!   └── Action: 状态转换产生的动作
//!
//! Orchestrator (uc-app)
//!   ├── 接收网络/用户/定时器输入
//!   ├── 转换为 PairingEvent
//!   ├── 调用状态机获取 actions
//!   └── 执行 actions (发送消息/启动定时器/持久化等)
//! ```

use crate::crypto::pin_hash::{hash_pin, verify_pin};
use crate::crypto::{IdentityFingerprint, ShortCodeGenerator};
use crate::network::{
    paired_device::{PairedDevice, PairingState as PairedDeviceState},
    protocol::{
        PairingCancel, PairingChallenge, PairingConfirm, PairingMessage, PairingReject,
        PairingRequest, PairingResponse,
    },
};
use crate::settings::model::PairingSettings;
use crate::PeerId;
use chrono::{DateTime, Duration, Utc};
use rand::Rng;
use serde::{Deserialize, Serialize};

/// 配对会话的唯一标识符
pub type SessionId = String;

/// 配对中的角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingRole {
    /// 发起方 (扫描/主动连接的一方)
    Initiator,
    /// 响应方 (被扫描/被动连接的一方)
    Responder,
}

/// 配对状态机的核心状态
///
/// Each state represents a specific stage in the pairing process,
/// with explicit handling for user verification, persistence, and error cases.
///
/// 每个状态代表配对流程中的一个特定阶段,
/// 对用户确认、持久化和错误情况都有显式处理。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingState {
    /// 空闲状态,未进行配对
    Idle,

    /// 已发送配对请求 (Initiator)
    RequestSent { session_id: SessionId },

    /// 等待用户确认 (Initiator, 显示短码/指纹)
    AwaitingUserConfirm {
        session_id: SessionId,
        short_code: String,
        peer_fingerprint: String,
        expires_at: DateTime<Utc>,
    },

    /// 已发送 Response (Initiator)
    ResponseSent { session_id: SessionId },

    /// 等待用户批准配对 (Responder)
    AwaitingUserApproval { session_id: SessionId },

    /// 已发送 Challenge (Responder)
    ChallengeSent { session_id: SessionId },

    /// 持久化中 (双方)
    Finalizing {
        session_id: SessionId,
        paired_device: PairedDevice,
    },

    /// 配对成功完成 (终态)
    Paired {
        session_id: SessionId,
        paired_device_id: String,
    },

    /// 配对失败 (终态)
    Failed {
        session_id: SessionId,
        reason: FailureReason,
    },

    /// 配对被取消/拒绝 (终态)
    Cancelled {
        session_id: SessionId,
        by: CancellationBy,
    },
}

/// 失败原因 (可审计)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    /// 传输层错误
    TransportError(String),

    /// 消息解析失败
    MessageParseError(String),

    /// 超时 (指定哪种类型的超时)
    Timeout(TimeoutKind),

    /// 重试次数耗尽
    RetryExhausted,

    /// 持久化失败
    PersistenceError(String),

    /// 加密操作失败
    CryptoError(String),

    /// 对端处于忙碌状态
    PeerBusy,

    /// 其他原因
    Other(String),
}

/// 超时类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeoutKind {
    /// 等待 Challenge 超时
    WaitingChallenge,
    /// 等待 Response 超时
    WaitingResponse,
    /// 等待 Confirm 超时
    WaitingConfirm,
    /// 用户确认超时
    UserVerification,
    /// 用户审批超时
    UserApproval,
    /// 持久化超时
    Persist,
}

/// 取消来源
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancellationBy {
    /// 本地用户取消/拒绝
    LocalUser,
    /// 远端用户取消/拒绝
    RemoteUser,
    /// 系统取消 (例如:应用关闭/资源不足)
    System,
}

/// 触发状态转换的事件
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingEvent {
    /// 开始配对 (用户或系统触发)
    StartPairing { role: PairingRole, peer_id: String },

    /// 收到配对请求
    RecvRequest {
        session_id: SessionId,
        /// 发送方 PeerID (从网络层获取,可信)
        sender_peer_id: String,
        request: crate::network::protocol::PairingRequest,
    },

    /// 收到 Challenge (包含PIN)
    RecvChallenge {
        session_id: SessionId,
        challenge: crate::network::protocol::PairingChallenge,
    },

    /// 收到 Response (包含PIN哈希)
    RecvResponse {
        session_id: SessionId,
        response: crate::network::protocol::PairingResponse,
    },

    /// 收到 Confirm
    RecvConfirm {
        session_id: SessionId,
        confirm: crate::network::protocol::PairingConfirm,
    },

    /// 收到拒绝
    RecvReject { session_id: SessionId },

    /// 收到取消
    RecvCancel { session_id: SessionId },

    /// 收到忙碌响应
    RecvBusy { session_id: SessionId },

    /// 用户接受配对 (确认短码匹配)
    UserAccept { session_id: SessionId },

    /// 用户拒绝配对
    UserReject { session_id: SessionId },

    /// 用户取消配对
    UserCancel { session_id: SessionId },

    /// 超时事件
    Timeout {
        session_id: SessionId,
        kind: TimeoutKind,
    },

    /// 传输层错误
    TransportError {
        session_id: SessionId,
        error: String,
    },

    /// 持久化成功
    PersistOk {
        session_id: SessionId,
        device_id: String,
    },

    /// 持久化失败
    PersistErr {
        session_id: SessionId,
        error: String,
    },
}

/// 状态转换产生的动作
///
/// 这些动作由 orchestrator 执行,实现状态机的副作用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingAction {
    /// 发送配对消息
    Send {
        peer_id: String,
        message: PairingMessage,
    },

    /// 启动定时器
    StartTimer {
        session_id: SessionId,
        kind: TimeoutKind,
        deadline: DateTime<Utc>,
    },

    /// 取消定时器
    CancelTimer {
        session_id: SessionId,
        kind: TimeoutKind,
    },

    /// 展示验证信息给用户 (短码 + 指纹)
    ShowVerification {
        session_id: SessionId,
        short_code: String,
        local_fingerprint: String,
        peer_fingerprint: String,
        peer_display_name: String,
    },

    /// 展示正在验证状态给用户 (用于等待持久化/完成)
    ShowVerifying {
        session_id: SessionId,
        peer_display_name: String,
    },

    /// 持久化配对设备
    PersistPairedDevice {
        session_id: SessionId,
        device: PairedDevice,
    },

    /// 记录状态转换日志 (用于审计)
    LogTransition {
        session_id: SessionId,
        old_state: String,
        event: String,
        new_state: String,
    },

    /// 发送配对结果事件
    EmitResult {
        session_id: SessionId,
        success: bool,
        error: Option<String>,
    },

    /// 无操作 (用于某些事件不需要动作的场景)
    NoOp,
}

/// 配对策略配置
#[derive(Debug, Clone)]
pub struct PairingPolicy {
    /// 步骤超时时间(秒)
    pub step_timeout_secs: i64,
    /// 用户确认超时时间(秒)
    pub user_verification_timeout_secs: i64,
    /// 最大重试次数
    pub max_retries: u8,
    /// 协议版本
    pub protocol_version: String,
}

impl Default for PairingPolicy {
    fn default() -> Self {
        let defaults = PairingSettings::default();
        Self {
            step_timeout_secs: defaults.step_timeout.as_secs().min(i64::MAX as u64) as i64,
            user_verification_timeout_secs: defaults
                .user_verification_timeout
                .as_secs()
                .min(i64::MAX as u64) as i64,
            max_retries: defaults.max_retries,
            protocol_version: defaults.protocol_version,
        }
    }
}

/// 配对状态机
///
/// 维护配对会话的状态,并根据事件产生状态转换和动作。
///
/// # Example / 示例
///
/// ```ignore
/// let mut sm = PairingStateMachine::new();
/// let (new_state, actions) = sm.handle_event(
///     PairingEvent::StartPairing {
///         role: PairingRole::Initiator,
///         peer_id: "12D3KooW...".to_string(),
///     },
///     Utc::now(),
/// );
/// ```
#[derive(Debug, Clone)]
pub struct PairingStateMachine {
    /// 当前状态
    state: PairingState,
    /// 配对上下文 (nonce、会话ID等)
    context: PairingContext,
    /// 配对策略
    policy: PairingPolicy,
}

/// 配对流程的上下文信息
#[derive(Debug, Clone)]
struct PairingContext {
    /// 会话ID
    session_id: Option<SessionId>,
    /// 本地角色
    role: Option<PairingRole>,
    /// 本地设备名称
    local_device_name: Option<String>,
    /// 本地设备ID
    local_device_id: Option<String>,
    /// 对端 PeerID
    peer_id: Option<String>,
    /// 本地 nonce (用于短码计算)
    local_nonce: Option<Vec<u8>>,
    /// 对端 nonce
    peer_nonce: Option<Vec<u8>>,
    /// 本地身份公钥
    local_identity_pubkey: Option<Vec<u8>>,
    /// 对端身份公钥
    peer_identity_pubkey: Option<Vec<u8>>,
    /// 对端设备名称
    peer_device_name: Option<String>,
    /// 短码 (用户确认码)
    short_code: Option<String>,
    /// 当前 PIN
    pin: Option<String>,
    /// 本地指纹
    local_fingerprint: Option<String>,
    /// 对端指纹
    peer_fingerprint: Option<String>,
    /// 会话创建时间
    created_at: Option<DateTime<Utc>>,
}

impl Default for PairingContext {
    fn default() -> Self {
        Self {
            session_id: None,
            role: None,
            local_device_name: None,
            local_device_id: None,
            peer_id: None,
            local_nonce: None,
            peer_nonce: None,
            local_identity_pubkey: None,
            peer_identity_pubkey: None,
            peer_device_name: None,
            short_code: None,
            pin: None,
            local_fingerprint: None,
            peer_fingerprint: None,
            created_at: None,
        }
    }
}

impl PairingStateMachine {
    /// 创建新的状态机实例
    pub fn new() -> Self {
        let policy = PairingPolicy::default();
        let context = PairingContext::default();
        Self {
            state: PairingState::Idle,
            context,
            policy,
        }
    }

    /// 创建新的状态机实例并注入本地设备信息
    pub fn new_with_local_identity(
        local_device_name: String,
        local_device_id: String,
        local_identity_pubkey: Vec<u8>,
    ) -> Self {
        let policy = PairingPolicy::default();
        let mut context = PairingContext::default();
        context.local_device_name = Some(local_device_name);
        context.local_device_id = Some(local_device_id);
        context.local_identity_pubkey = Some(local_identity_pubkey);
        Self {
            state: PairingState::Idle,
            context,
            policy,
        }
    }

    /// 创建新的状态机实例并注入本地设备信息与策略
    pub fn new_with_local_identity_and_policy(
        local_device_name: String,
        local_device_id: String,
        local_identity_pubkey: Vec<u8>,
        policy: PairingPolicy,
    ) -> Self {
        let mut context = PairingContext::default();
        context.local_device_name = Some(local_device_name);
        context.local_device_id = Some(local_device_id);
        context.local_identity_pubkey = Some(local_identity_pubkey);
        Self {
            state: PairingState::Idle,
            context,
            policy,
        }
    }

    /// 获取当前状态
    pub fn state(&self) -> &PairingState {
        &self.state
    }

    /// 获取当前角色
    pub fn role(&self) -> Option<PairingRole> {
        self.context.role
    }

    /// 处理事件并返回新状态和动作列表
    ///
    /// 这是状态机的核心方法,实现了纯函数式状态转换。
    pub fn handle_event(
        &mut self,
        event: PairingEvent,
        now: DateTime<Utc>,
    ) -> (PairingState, Vec<PairingAction>) {
        let old_state = self.state.clone();
        let session_id = self.extract_session_id(&event);
        let event_debug = format!("{:?}", event);

        let (new_state, actions) = self.transition(event, now);

        // 记录状态转换 (用于审计)
        let log_action = PairingAction::LogTransition {
            session_id,
            old_state: format!("{:?}", old_state),
            event: event_debug,
            new_state: format!("{:?}", new_state),
        };

        let mut all_actions = vec![log_action];
        all_actions.extend(actions);

        self.state = new_state.clone();
        (new_state, all_actions)
    }

    /// 从事件中提取会话ID
    fn extract_session_id(&self, event: &PairingEvent) -> SessionId {
        match event {
            PairingEvent::StartPairing {
                role: _,
                peer_id: _,
            } => self.context.session_id.clone().unwrap_or_default(),
            PairingEvent::RecvRequest { session_id, .. } => session_id.clone(),
            PairingEvent::RecvChallenge { session_id, .. } => session_id.clone(),
            PairingEvent::RecvResponse { session_id, .. } => session_id.clone(),
            PairingEvent::RecvConfirm { session_id, .. } => session_id.clone(),
            PairingEvent::RecvReject { session_id } => session_id.clone(),
            PairingEvent::RecvCancel { session_id } => session_id.clone(),
            PairingEvent::RecvBusy { session_id } => session_id.clone(),
            PairingEvent::UserAccept { session_id } => session_id.clone(),
            PairingEvent::UserReject { session_id } => session_id.clone(),
            PairingEvent::UserCancel { session_id } => session_id.clone(),
            PairingEvent::Timeout { session_id, .. } => session_id.clone(),
            PairingEvent::TransportError { session_id, .. } => session_id.clone(),
            PairingEvent::PersistOk { session_id, .. } => session_id.clone(),
            PairingEvent::PersistErr { session_id, .. } => session_id.clone(),
        }
    }

    /// 状态转换逻辑 (核心实现)
    fn transition(
        &mut self,
        event: PairingEvent,
        now: DateTime<Utc>,
    ) -> (PairingState, Vec<PairingAction>) {
        match (self.state.clone(), event) {
            (PairingState::Idle, PairingEvent::StartPairing { role, peer_id }) => {
                if role != PairingRole::Initiator {
                    return self.fail_with_reason(
                        self.context.session_id.clone().unwrap_or_default(),
                        FailureReason::Other("Invalid role for StartPairing".to_string()),
                    );
                }

                let session_id = uuid::Uuid::new_v4().to_string();
                let local_nonce = generate_nonce();
                self.context.session_id = Some(session_id.clone());
                self.context.role = Some(role);
                self.context.peer_id = Some(peer_id.clone());
                self.context.local_nonce = Some(local_nonce.clone());
                self.context.created_at = Some(now);

                let local_device_name = match self.context.local_device_name.clone() {
                    Some(name) => name,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local device name".to_string()),
                        )
                    }
                };
                let local_device_id = match self.context.local_device_id.clone() {
                    Some(id) => id,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local device id".to_string()),
                        )
                    }
                };
                let local_identity_pubkey = match self.context.local_identity_pubkey.clone() {
                    Some(pubkey) => pubkey,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local identity pubkey".to_string()),
                        )
                    }
                };

                let request = PairingRequest {
                    session_id: session_id.clone(),
                    device_name: local_device_name,
                    device_id: local_device_id,
                    peer_id: peer_id.clone(),
                    identity_pubkey: local_identity_pubkey,
                    nonce: local_nonce,
                };

                let deadline = now + Duration::seconds(self.policy.step_timeout_secs);
                let actions = vec![
                    PairingAction::Send {
                        peer_id,
                        message: PairingMessage::Request(request),
                    },
                    PairingAction::StartTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::WaitingChallenge,
                        deadline,
                    },
                ];

                (
                    PairingState::RequestSent {
                        session_id: session_id.clone(),
                    },
                    actions,
                )
            }
            (
                PairingState::Idle,
                PairingEvent::RecvRequest {
                    request,
                    sender_peer_id,
                    ..
                },
            ) => {
                self.context.session_id = Some(request.session_id.clone());
                self.context.role = Some(PairingRole::Responder);
                self.context.peer_id = Some(sender_peer_id);
                self.context.peer_nonce = Some(request.nonce.clone());
                self.context.peer_identity_pubkey = Some(request.identity_pubkey.clone());
                self.context.peer_device_name = Some(request.device_name.clone());
                self.context.created_at = Some(now);

                let deadline = now + Duration::seconds(self.policy.user_verification_timeout_secs);
                let actions = vec![PairingAction::StartTimer {
                    session_id: request.session_id.clone(),
                    kind: TimeoutKind::UserApproval,
                    deadline,
                }];

                (
                    PairingState::AwaitingUserApproval {
                        session_id: request.session_id,
                    },
                    actions,
                )
            }
            (
                PairingState::RequestSent { session_id },
                PairingEvent::RecvChallenge { challenge, .. },
            ) => {
                let local_identity_pubkey = match self.context.local_identity_pubkey.clone() {
                    Some(pubkey) => pubkey,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local identity pubkey".to_string()),
                        )
                    }
                };

                self.context.peer_nonce = Some(challenge.nonce.clone());
                self.context.peer_identity_pubkey = Some(challenge.identity_pubkey.clone());
                self.context.pin = Some(challenge.pin.clone());
                self.context.peer_device_name = Some(challenge.device_name.clone());
                self.context.created_at = Some(now);

                let local_nonce = self
                    .context
                    .local_nonce
                    .clone()
                    .unwrap_or_else(generate_nonce);
                self.context.local_nonce = Some(local_nonce.clone());

                let local_fingerprint =
                    match IdentityFingerprint::from_public_key(&local_identity_pubkey) {
                        Ok(fingerprint) => fingerprint.to_string(),
                        Err(err) => {
                            return self.fail_with_reason(
                                session_id,
                                FailureReason::CryptoError(err.to_string()),
                            )
                        }
                    };
                let peer_fingerprint =
                    match IdentityFingerprint::from_public_key(&challenge.identity_pubkey) {
                        Ok(fingerprint) => fingerprint.to_string(),
                        Err(err) => {
                            return self.fail_with_reason(
                                session_id,
                                FailureReason::CryptoError(err.to_string()),
                            )
                        }
                    };
                let short_code = match ShortCodeGenerator::generate(
                    &challenge.session_id,
                    &local_nonce,
                    &challenge.nonce,
                    &local_identity_pubkey,
                    &challenge.identity_pubkey,
                    &self.policy.protocol_version,
                ) {
                    Ok(code) => code,
                    Err(err) => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::CryptoError(err.to_string()),
                        )
                    }
                };

                self.context.short_code = Some(short_code.clone());
                self.context.local_fingerprint = Some(local_fingerprint.clone());
                self.context.peer_fingerprint = Some(peer_fingerprint.clone());

                let expires_at =
                    now + Duration::seconds(self.policy.user_verification_timeout_secs);
                let actions = vec![
                    PairingAction::CancelTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::WaitingChallenge,
                    },
                    PairingAction::ShowVerification {
                        session_id: session_id.clone(),
                        short_code: short_code.clone(),
                        local_fingerprint,
                        peer_fingerprint: peer_fingerprint.clone(),
                        peer_display_name: challenge.device_name,
                    },
                    PairingAction::StartTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::UserVerification,
                        deadline: expires_at,
                    },
                ];

                (
                    PairingState::AwaitingUserConfirm {
                        session_id,
                        short_code,
                        peer_fingerprint,
                        expires_at,
                    },
                    actions,
                )
            }
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::UserAccept { .. },
            ) => {
                let peer_id = match self.context.peer_id.clone() {
                    Some(id) => id,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing peer id".to_string()),
                        )
                    }
                };
                let pin = match self.context.pin.clone() {
                    Some(pin) => pin,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing PIN".to_string()),
                        )
                    }
                };

                let pin_hash = match hash_pin(&pin) {
                    Ok(hash) => hash,
                    Err(err) => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::CryptoError(err.to_string()),
                        )
                    }
                };
                self.context.pin = None;

                let response = PairingResponse {
                    session_id: session_id.clone(),
                    pin_hash,
                    accepted: true,
                };

                let deadline = now + Duration::seconds(self.policy.step_timeout_secs);
                let actions = vec![
                    PairingAction::CancelTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::UserVerification,
                    },
                    PairingAction::Send {
                        peer_id,
                        message: PairingMessage::Response(response),
                    },
                    PairingAction::StartTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::WaitingConfirm,
                        deadline,
                    },
                ];

                (
                    PairingState::ResponseSent {
                        session_id: session_id.clone(),
                    },
                    actions,
                )
            }
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::UserReject { .. },
            ) => self.cancel_with_reason(
                session_id.clone(),
                CancellationBy::LocalUser,
                Some("User rejected pairing".to_string()),
                Some(PairingAction::Send {
                    peer_id: self.context.peer_id.clone().unwrap_or_default(),
                    message: PairingMessage::Reject(PairingReject {
                        session_id: session_id.clone(),
                        reason: Some("user_reject".to_string()),
                    }),
                }),
                Some(TimeoutKind::UserVerification),
            ),
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::UserCancel { .. },
            ) => self.cancel_with_reason(
                session_id.clone(),
                CancellationBy::LocalUser,
                Some("User cancelled pairing".to_string()),
                Some(PairingAction::Send {
                    peer_id: self.context.peer_id.clone().unwrap_or_default(),
                    message: PairingMessage::Cancel(PairingCancel {
                        session_id: session_id.clone(),
                        reason: Some("user_cancel".to_string()),
                    }),
                }),
                Some(TimeoutKind::UserVerification),
            ),
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::Timeout {
                    kind: TimeoutKind::UserVerification,
                    ..
                },
            ) => self.fail_with_reason(
                session_id,
                FailureReason::Timeout(TimeoutKind::UserVerification),
            ),
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::RecvCancel { .. },
            ) => self.cancel_with_reason(
                session_id,
                CancellationBy::RemoteUser,
                Some("Peer cancelled pairing".to_string()),
                None,
                Some(TimeoutKind::UserVerification),
            ),
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::RecvReject { .. },
            ) => self.cancel_with_reason(
                session_id,
                CancellationBy::RemoteUser,
                Some("Peer rejected pairing".to_string()),
                None,
                Some(TimeoutKind::UserVerification),
            ),
            (
                PairingState::AwaitingUserConfirm { session_id, .. },
                PairingEvent::RecvBusy { .. },
            ) => self.fail_with_reason(session_id, FailureReason::PeerBusy),
            (
                PairingState::RequestSent { session_id },
                PairingEvent::Timeout {
                    kind: TimeoutKind::WaitingChallenge,
                    ..
                },
            ) => self.fail_with_reason(
                session_id,
                FailureReason::Timeout(TimeoutKind::WaitingChallenge),
            ),
            (PairingState::RequestSent { session_id }, PairingEvent::RecvReject { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer rejected pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingChallenge),
                ),
            (PairingState::RequestSent { session_id }, PairingEvent::RecvCancel { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer cancelled pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingChallenge),
                ),
            (PairingState::RequestSent { session_id }, PairingEvent::RecvBusy { .. }) => {
                self.fail_with_reason(session_id, FailureReason::PeerBusy)
            }
            (PairingState::RequestSent { session_id }, PairingEvent::UserReject { .. }) => self
                .cancel_with_reason(
                    session_id.clone(),
                    CancellationBy::LocalUser,
                    Some("User rejected pairing".to_string()),
                    Some(PairingAction::Send {
                        peer_id: self.context.peer_id.clone().unwrap_or_default(),
                        message: PairingMessage::Reject(PairingReject {
                            session_id: session_id.clone(),
                            reason: Some("user_reject".to_string()),
                        }),
                    }),
                    Some(TimeoutKind::WaitingChallenge),
                ),
            (PairingState::RequestSent { session_id }, PairingEvent::UserCancel { .. }) => self
                .cancel_with_reason(
                    session_id.clone(),
                    CancellationBy::LocalUser,
                    Some("User cancelled pairing".to_string()),
                    Some(PairingAction::Send {
                        peer_id: self.context.peer_id.clone().unwrap_or_default(),
                        message: PairingMessage::Cancel(PairingCancel {
                            session_id: session_id.clone(),
                            reason: Some("user_cancel".to_string()),
                        }),
                    }),
                    Some(TimeoutKind::WaitingChallenge),
                ),
            (
                PairingState::ResponseSent { session_id },
                PairingEvent::RecvConfirm { confirm, .. },
            ) => {
                if !confirm.sender_device_name.is_empty() {
                    self.context.peer_device_name = Some(confirm.sender_device_name.clone());
                }

                let cancel_timer = PairingAction::CancelTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingConfirm,
                };

                if !confirm.success {
                    let error = confirm
                        .error
                        .unwrap_or_else(|| "Pairing rejected".to_string());
                    let (state, mut actions) =
                        self.fail_with_reason(session_id, FailureReason::Other(error));
                    actions.insert(0, cancel_timer);
                    return (state, actions);
                }

                let paired_device = match self.build_paired_device(now) {
                    Ok(device) => device,
                    Err(reason) => {
                        let (state, mut actions) = self.fail_with_reason(session_id, reason);
                        actions.insert(0, cancel_timer);
                        return (state, actions);
                    }
                };

                let deadline = now + Duration::seconds(self.policy.step_timeout_secs);
                let actions = vec![
                    cancel_timer,
                    PairingAction::PersistPairedDevice {
                        session_id: session_id.clone(),
                        device: paired_device.clone(),
                    },
                    PairingAction::StartTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::Persist,
                        deadline,
                    },
                ];

                (
                    PairingState::Finalizing {
                        session_id,
                        paired_device,
                    },
                    actions,
                )
            }
            (
                PairingState::ResponseSent { session_id },
                PairingEvent::Timeout {
                    kind: TimeoutKind::WaitingConfirm,
                    ..
                },
            ) => self.fail_with_reason(
                session_id,
                FailureReason::Timeout(TimeoutKind::WaitingConfirm),
            ),
            (PairingState::ResponseSent { session_id }, PairingEvent::RecvCancel { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer cancelled pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingConfirm),
                ),
            (PairingState::ResponseSent { session_id }, PairingEvent::RecvReject { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer rejected pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingConfirm),
                ),
            (PairingState::ResponseSent { session_id }, PairingEvent::RecvBusy { .. }) => {
                self.fail_with_reason(session_id, FailureReason::PeerBusy)
            }
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::UserAccept { .. },
            ) => {
                let local_device_name = match self.context.local_device_name.clone() {
                    Some(name) => name,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local device name".to_string()),
                        )
                    }
                };
                let local_device_id = match self.context.local_device_id.clone() {
                    Some(id) => id,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local device id".to_string()),
                        )
                    }
                };
                let local_identity_pubkey = match self.context.local_identity_pubkey.clone() {
                    Some(pubkey) => pubkey,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing local identity pubkey".to_string()),
                        )
                    }
                };
                let peer_identity_pubkey = match self.context.peer_identity_pubkey.clone() {
                    Some(pubkey) => pubkey,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing peer identity pubkey".to_string()),
                        )
                    }
                };
                let peer_id = match self.context.peer_id.clone() {
                    Some(id) => id,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing peer id".to_string()),
                        )
                    }
                };

                let pin = generate_pin();
                let nonce = generate_nonce();
                self.context.pin = Some(pin.clone());
                self.context.local_nonce = Some(nonce.clone());

                let peer_nonce = match self.context.peer_nonce.clone() {
                    Some(value) => value,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing peer nonce".to_string()),
                        )
                    }
                };
                let local_fingerprint =
                    match IdentityFingerprint::from_public_key(&local_identity_pubkey) {
                        Ok(fingerprint) => fingerprint.to_string(),
                        Err(err) => {
                            return self.fail_with_reason(
                                session_id,
                                FailureReason::CryptoError(err.to_string()),
                            )
                        }
                    };
                let peer_fingerprint =
                    match IdentityFingerprint::from_public_key(&peer_identity_pubkey) {
                        Ok(fingerprint) => fingerprint.to_string(),
                        Err(err) => {
                            return self.fail_with_reason(
                                session_id,
                                FailureReason::CryptoError(err.to_string()),
                            )
                        }
                    };
                let short_code = match ShortCodeGenerator::generate(
                    &session_id,
                    &peer_nonce,
                    &nonce,
                    &peer_identity_pubkey,
                    &local_identity_pubkey,
                    &self.policy.protocol_version,
                ) {
                    Ok(code) => code,
                    Err(err) => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::CryptoError(err.to_string()),
                        )
                    }
                };

                self.context.short_code = Some(short_code.clone());
                self.context.local_fingerprint = Some(local_fingerprint.clone());
                self.context.peer_fingerprint = Some(peer_fingerprint.clone());

                let challenge = PairingChallenge {
                    session_id: session_id.clone(),
                    pin,
                    device_name: local_device_name,
                    device_id: local_device_id,
                    identity_pubkey: local_identity_pubkey.clone(),
                    nonce,
                };

                let deadline = now + Duration::seconds(self.policy.step_timeout_secs);
                let peer_display_name = self
                    .context
                    .peer_device_name
                    .clone()
                    .unwrap_or_else(|| "Unknown Device".to_string());
                let actions = vec![
                    PairingAction::CancelTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::UserApproval,
                    },
                    PairingAction::ShowVerification {
                        session_id: session_id.clone(),
                        short_code,
                        local_fingerprint,
                        peer_fingerprint,
                        peer_display_name,
                    },
                    PairingAction::Send {
                        peer_id,
                        message: PairingMessage::Challenge(challenge),
                    },
                    PairingAction::StartTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::WaitingResponse,
                        deadline,
                    },
                ];

                (
                    PairingState::ChallengeSent {
                        session_id: session_id.clone(),
                    },
                    actions,
                )
            }
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::UserReject { .. },
            ) => self.cancel_with_reason(
                session_id.clone(),
                CancellationBy::LocalUser,
                Some("User rejected pairing".to_string()),
                Some(PairingAction::Send {
                    peer_id: self.context.peer_id.clone().unwrap_or_default(),
                    message: PairingMessage::Reject(PairingReject {
                        session_id: session_id.clone(),
                        reason: Some("user_reject".to_string()),
                    }),
                }),
                Some(TimeoutKind::UserApproval),
            ),
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::UserCancel { .. },
            ) => self.cancel_with_reason(
                session_id.clone(),
                CancellationBy::LocalUser,
                Some("User cancelled pairing".to_string()),
                Some(PairingAction::Send {
                    peer_id: self.context.peer_id.clone().unwrap_or_default(),
                    message: PairingMessage::Cancel(PairingCancel {
                        session_id: session_id.clone(),
                        reason: Some("user_cancel".to_string()),
                    }),
                }),
                Some(TimeoutKind::UserApproval),
            ),
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::Timeout {
                    kind: TimeoutKind::UserApproval,
                    ..
                },
            ) => self.fail_with_reason(
                session_id,
                FailureReason::Timeout(TimeoutKind::UserApproval),
            ),
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::RecvCancel { .. },
            ) => self.cancel_with_reason(
                session_id,
                CancellationBy::RemoteUser,
                Some("Peer cancelled pairing".to_string()),
                None,
                Some(TimeoutKind::UserApproval),
            ),
            (
                PairingState::AwaitingUserApproval { session_id },
                PairingEvent::RecvReject { .. },
            ) => self.cancel_with_reason(
                session_id,
                CancellationBy::RemoteUser,
                Some("Peer rejected pairing".to_string()),
                None,
                Some(TimeoutKind::UserApproval),
            ),
            (PairingState::AwaitingUserApproval { session_id }, PairingEvent::RecvBusy { .. }) => {
                self.fail_with_reason(session_id, FailureReason::PeerBusy)
            }
            (
                PairingState::ChallengeSent { session_id },
                PairingEvent::RecvResponse { response, .. },
            ) => {
                let peer_id = match self.context.peer_id.clone() {
                    Some(id) => id,
                    None => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::Other("Missing peer id".to_string()),
                        )
                    }
                };
                let local_device_name = self.context.local_device_name.clone().unwrap_or_default();
                let local_device_id = self.context.local_device_id.clone().unwrap_or_default();

                let mut actions = vec![PairingAction::CancelTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingResponse,
                }];

                let peer_display_name = self
                    .context
                    .peer_device_name
                    .clone()
                    .unwrap_or_else(|| "".to_string());
                actions.push(PairingAction::ShowVerifying {
                    session_id: session_id.clone(),
                    peer_display_name,
                });

                if !response.accepted {
                    actions.push(PairingAction::Send {
                        peer_id,
                        message: PairingMessage::Confirm(PairingConfirm {
                            session_id: session_id.clone(),
                            success: false,
                            error: Some("Pairing rejected".to_string()),
                            sender_device_name: local_device_name,
                            device_id: local_device_id,
                        }),
                    });
                    let (state, mut cancel_actions) = self.cancel_with_reason(
                        session_id,
                        CancellationBy::RemoteUser,
                        Some("Peer rejected pairing".to_string()),
                        None,
                        None,
                    );
                    cancel_actions.splice(0..0, actions);
                    return (state, cancel_actions);
                }

                let pin = self.context.pin.as_deref().ok_or_else(|| {
                    FailureReason::Other("PIN not available for verification".to_string())
                });
                let pin = match pin {
                    Ok(value) => value,
                    Err(reason) => return self.fail_with_reason(session_id, reason),
                };
                let verified = match verify_pin(pin, &response.pin_hash) {
                    Ok(result) => result,
                    Err(err) => {
                        return self.fail_with_reason(
                            session_id,
                            FailureReason::CryptoError(err.to_string()),
                        )
                    }
                };
                self.context.pin = None;

                if !verified {
                    actions.push(PairingAction::Send {
                        peer_id,
                        message: PairingMessage::Confirm(PairingConfirm {
                            session_id: session_id.clone(),
                            success: false,
                            error: Some("PIN verification failed".to_string()),
                            sender_device_name: local_device_name,
                            device_id: local_device_id,
                        }),
                    });
                    let (state, mut fail_actions) = self.fail_with_reason(
                        session_id,
                        FailureReason::CryptoError("PIN verification failed".to_string()),
                    );
                    fail_actions.splice(0..0, actions);
                    return (state, fail_actions);
                }

                let confirm = PairingConfirm {
                    session_id: session_id.clone(),
                    success: true,
                    error: None,
                    sender_device_name: local_device_name,
                    device_id: local_device_id,
                };
                actions.push(PairingAction::Send {
                    peer_id,
                    message: PairingMessage::Confirm(confirm),
                });

                let paired_device = match self.build_paired_device(now) {
                    Ok(device) => device,
                    Err(reason) => return self.fail_with_reason(session_id, reason),
                };
                let deadline = now + Duration::seconds(self.policy.step_timeout_secs);
                actions.push(PairingAction::PersistPairedDevice {
                    session_id: session_id.clone(),
                    device: paired_device.clone(),
                });
                actions.push(PairingAction::StartTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::Persist,
                    deadline,
                });

                (
                    PairingState::Finalizing {
                        session_id,
                        paired_device,
                    },
                    actions,
                )
            }
            (
                PairingState::ChallengeSent { session_id },
                PairingEvent::Timeout {
                    kind: TimeoutKind::WaitingResponse,
                    ..
                },
            ) => self.fail_with_reason(
                session_id,
                FailureReason::Timeout(TimeoutKind::WaitingResponse),
            ),
            (PairingState::ChallengeSent { session_id }, PairingEvent::RecvCancel { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer cancelled pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingResponse),
                ),
            (PairingState::ChallengeSent { session_id }, PairingEvent::RecvReject { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer rejected pairing".to_string()),
                    None,
                    Some(TimeoutKind::WaitingResponse),
                ),
            (PairingState::ChallengeSent { session_id }, PairingEvent::RecvBusy { .. }) => {
                self.fail_with_reason(session_id, FailureReason::PeerBusy)
            }
            (
                PairingState::Finalizing { session_id, .. },
                PairingEvent::PersistOk { device_id, .. },
            ) => (
                PairingState::Paired {
                    session_id: session_id.clone(),
                    paired_device_id: device_id,
                },
                vec![
                    PairingAction::CancelTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::Persist,
                    },
                    PairingAction::EmitResult {
                        session_id,
                        success: true,
                        error: None,
                    },
                ],
            ),
            (
                PairingState::Finalizing { session_id, .. },
                PairingEvent::PersistErr { error, .. },
            ) => (
                PairingState::Failed {
                    session_id: session_id.clone(),
                    reason: FailureReason::PersistenceError(error.clone()),
                },
                vec![
                    PairingAction::CancelTimer {
                        session_id: session_id.clone(),
                        kind: TimeoutKind::Persist,
                    },
                    PairingAction::EmitResult {
                        session_id,
                        success: false,
                        error: Some(error),
                    },
                ],
            ),
            (
                PairingState::Finalizing { session_id, .. },
                PairingEvent::Timeout {
                    kind: TimeoutKind::Persist,
                    ..
                },
            ) => self.fail_with_reason(session_id, FailureReason::Timeout(TimeoutKind::Persist)),
            (PairingState::Finalizing { session_id, .. }, PairingEvent::RecvCancel { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer cancelled pairing".to_string()),
                    None,
                    Some(TimeoutKind::Persist),
                ),
            (PairingState::Finalizing { session_id, .. }, PairingEvent::RecvReject { .. }) => self
                .cancel_with_reason(
                    session_id,
                    CancellationBy::RemoteUser,
                    Some("Peer rejected pairing".to_string()),
                    None,
                    Some(TimeoutKind::Persist),
                ),
            (PairingState::Finalizing { session_id, .. }, PairingEvent::RecvBusy { .. }) => {
                self.fail_with_reason(session_id, FailureReason::PeerBusy)
            }
            (state, PairingEvent::TransportError { error, .. })
                if !matches!(
                    state,
                    PairingState::Paired { .. }
                        | PairingState::Failed { .. }
                        | PairingState::Cancelled { .. }
                ) =>
            {
                let session_id = self.context.session_id.clone().unwrap_or_default();
                self.fail_with_reason(session_id, FailureReason::TransportError(error))
            }
            _ => (
                PairingState::Failed {
                    session_id: self.context.session_id.clone().unwrap_or_default(),
                    reason: FailureReason::Other("Unexpected state transition".to_string()),
                },
                vec![],
            ),
        }
    }

    fn fail_with_reason(
        &self,
        session_id: SessionId,
        reason: FailureReason,
    ) -> (PairingState, Vec<PairingAction>) {
        let error_msg = format!("{:?}", reason);
        (
            PairingState::Failed {
                session_id: session_id.clone(),
                reason,
            },
            vec![PairingAction::EmitResult {
                session_id,
                success: false,
                error: Some(error_msg),
            }],
        )
    }

    fn cancel_with_reason(
        &self,
        session_id: SessionId,
        by: CancellationBy,
        error: Option<String>,
        send_action: Option<PairingAction>,
        cancel_timer: Option<TimeoutKind>,
    ) -> (PairingState, Vec<PairingAction>) {
        let mut actions = Vec::new();
        if let Some(kind) = cancel_timer {
            actions.push(PairingAction::CancelTimer {
                session_id: session_id.clone(),
                kind,
            });
        }
        if let Some(action) = send_action {
            actions.push(action);
        }
        actions.push(PairingAction::EmitResult {
            session_id: session_id.clone(),
            success: false,
            error: error.clone(),
        });

        (PairingState::Cancelled { session_id, by }, actions)
    }

    fn build_paired_device(&self, now: DateTime<Utc>) -> Result<PairedDevice, FailureReason> {
        let peer_id = self
            .context
            .peer_id
            .clone()
            .ok_or_else(|| FailureReason::Other("Missing peer id".to_string()))?;
        let peer_identity_pubkey = self
            .context
            .peer_identity_pubkey
            .clone()
            .ok_or_else(|| FailureReason::Other("Missing peer identity pubkey".to_string()))?;
        let fingerprint = match IdentityFingerprint::from_public_key(&peer_identity_pubkey) {
            Ok(value) => value.to_string(),
            Err(err) => return Err(FailureReason::CryptoError(err.to_string())),
        };

        let device_name = self
            .context
            .peer_device_name
            .clone()
            .unwrap_or_else(|| "Unknown Device".to_string());

        Ok(PairedDevice {
            peer_id: PeerId::from(peer_id),
            pairing_state: PairedDeviceState::Pending,
            identity_fingerprint: fingerprint,
            paired_at: now,
            last_seen_at: None,
            device_name,
            sync_settings: None,
        })
    }
}

const PIN_LENGTH: usize = 6;

fn generate_pin() -> String {
    let mut rng = rand::rng();
    (0..PIN_LENGTH)
        .map(|_| rng.random_range(0..10).to_string())
        .collect()
}

fn generate_nonce() -> Vec<u8> {
    uuid::Uuid::new_v4().as_bytes().to_vec()
}

impl Default for PairingStateMachine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::pin_hash::hash_pin;
    use crate::network::protocol::{PairingChallenge, PairingRequest, PairingResponse};

    fn build_request(session_id: &str) -> PairingRequest {
        PairingRequest {
            session_id: session_id.to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-2".to_string(),
            peer_id: "peer-remote".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        }
    }

    fn build_challenge(session_id: &str) -> PairingChallenge {
        PairingChallenge {
            session_id: session_id.to_string(),
            pin: "123456".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-2".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        }
    }

    #[test]
    fn responder_short_code_matches_initiator_ordering() {
        let policy = PairingPolicy {
            step_timeout_secs: 10,
            user_verification_timeout_secs: 10,
            max_retries: 1,
            protocol_version: "1.0.0".to_string(),
        };

        // Local identity = responder
        let responder_pubkey = vec![1; 32];
        let mut sm = PairingStateMachine::new_with_local_identity_and_policy(
            "ResponderDevice".to_string(),
            "device-responder".to_string(),
            responder_pubkey.clone(),
            policy.clone(),
        );

        // Peer identity = initiator
        let initiator_nonce = vec![9; 16];
        let initiator_pubkey = vec![2; 32];
        let session_id = "session-1";
        let request = PairingRequest {
            session_id: session_id.to_string(),
            device_name: "InitiatorDevice".to_string(),
            device_id: "device-initiator".to_string(),
            peer_id: "peer-initiator".to_string(),
            identity_pubkey: initiator_pubkey.clone(),
            nonce: initiator_nonce.clone(),
        };

        let (state, _actions) = sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: session_id.to_string(),
                sender_peer_id: "peer-initiator".to_string(),
                request: request.clone(),
            },
            Utc::now(),
        );
        assert!(matches!(state, PairingState::AwaitingUserApproval { .. }));

        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: session_id.to_string(),
            },
            Utc::now(),
        );

        let show_code = actions.iter().find_map(|action| match action {
            PairingAction::ShowVerification { short_code, .. } => Some(short_code.clone()),
            _ => None,
        });
        let show_code = show_code.expect("ShowVerification action missing");

        let challenge = actions.iter().find_map(|action| match action {
            PairingAction::Send {
                message: PairingMessage::Challenge(challenge),
                ..
            } => Some(challenge.clone()),
            _ => None,
        });
        let challenge = challenge.expect("Challenge message missing");

        // Expected transcript ordering is ALWAYS initiator-first, responder-second.
        let expected = ShortCodeGenerator::generate(
            session_id,
            &initiator_nonce,
            &challenge.nonce,
            &initiator_pubkey,
            &challenge.identity_pubkey,
            &policy.protocol_version,
        )
        .expect("generate short code");

        assert_eq!(show_code, expected);
    }

    #[test]
    fn initiator_start_transitions_to_request_sent() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        let (state, actions) = sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );

        assert!(matches!(state, PairingState::RequestSent { .. }));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Request(_),
                ..
            }
        )));
    }

    #[test]
    fn initiator_challenge_enters_user_confirm() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );

        let (state, actions) = sm.handle_event(
            PairingEvent::RecvChallenge {
                session_id: "session-1".to_string(),
                challenge: build_challenge("session-1"),
            },
            Utc::now(),
        );

        assert!(matches!(state, PairingState::AwaitingUserConfirm { .. }));
        assert!(actions
            .iter()
            .any(|action| matches!(action, PairingAction::ShowVerification { .. })));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::StartTimer {
                kind: TimeoutKind::UserVerification,
                ..
            }
        )));
    }

    #[test]
    fn initiator_accept_sends_response() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );

        sm.handle_event(
            PairingEvent::RecvChallenge {
                session_id: "session-1".to_string(),
                challenge: build_challenge("session-1"),
            },
            Utc::now(),
        );

        let (state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );

        assert!(matches!(state, PairingState::ResponseSent { .. }));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Response(_),
                ..
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::StartTimer {
                kind: TimeoutKind::WaitingConfirm,
                ..
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::CancelTimer {
                kind: TimeoutKind::UserVerification,
                ..
            }
        )));
    }

    #[test]
    fn responder_accept_sends_challenge() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request: build_request("session-1"),
            },
            Utc::now(),
        );

        let (state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );

        assert!(matches!(state, PairingState::ChallengeSent { .. }));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Challenge(_),
                ..
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::StartTimer {
                kind: TimeoutKind::WaitingResponse,
                ..
            }
        )));
    }

    #[test]
    fn responder_recv_response_emits_show_verifying() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request: build_request("session-1"),
            },
            Utc::now(),
        );

        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );

        let challenge_pin = actions
            .iter()
            .find_map(|action| match action {
                PairingAction::Send {
                    message: PairingMessage::Challenge(challenge),
                    ..
                } => Some(challenge.pin.clone()),
                _ => None,
            })
            .expect("challenge pin");

        let response = PairingResponse {
            session_id: "session-1".to_string(),
            pin_hash: hash_pin(&challenge_pin).expect("hash pin"),
            accepted: true,
        };

        let (state, actions) = sm.handle_event(
            PairingEvent::RecvResponse {
                session_id: "session-1".to_string(),
                response,
            },
            Utc::now(),
        );

        assert!(
            matches!(state, PairingState::Finalizing { .. }),
            "state: {:?}",
            state
        );
        assert!(actions
            .iter()
            .any(|action| matches!(action, PairingAction::ShowVerifying { .. })));
    }

    #[test]
    fn responder_response_success_persists() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request: build_request("session-1"),
            },
            Utc::now(),
        );
        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );
        let challenge = actions
            .iter()
            .find_map(|action| match action {
                PairingAction::Send {
                    message: PairingMessage::Challenge(challenge),
                    ..
                } => Some(challenge.clone()),
                _ => None,
            })
            .expect("challenge action");

        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash: hash_pin(&challenge.pin).expect("hash pin"),
            accepted: true,
        };
        let (state, actions) = sm.handle_event(
            PairingEvent::RecvResponse {
                session_id: challenge.session_id.clone(),
                response,
            },
            Utc::now(),
        );

        assert!(matches!(state, PairingState::Finalizing { .. }));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Confirm(_),
                ..
            }
        )));
        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::PersistPairedDevice { device, .. }
                if device.pairing_state == PairedDeviceState::Pending
        )));
    }

    #[test]
    fn finalizing_persist_ok_cancels_timer() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request: build_request("session-1"),
            },
            Utc::now(),
        );
        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );
        let challenge = actions
            .iter()
            .find_map(|action| match action {
                PairingAction::Send {
                    message: PairingMessage::Challenge(challenge),
                    ..
                } => Some(challenge.clone()),
                _ => None,
            })
            .expect("challenge action");

        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash: hash_pin(&challenge.pin).expect("hash pin"),
            accepted: true,
        };
        let (state, _actions) = sm.handle_event(
            PairingEvent::RecvResponse {
                session_id: challenge.session_id.clone(),
                response,
            },
            Utc::now(),
        );

        let session_id = match state {
            PairingState::Finalizing { session_id, .. } => session_id,
            _ => panic!("Expected Finalizing state, got {:?}", state),
        };

        let (_state, actions) = sm.handle_event(
            PairingEvent::PersistOk {
                session_id,
                device_id: "device-remote".to_string(),
            },
            Utc::now(),
        );

        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::CancelTimer {
                kind: TimeoutKind::Persist,
                ..
            }
        )));
    }

    #[test]
    fn finalizing_persist_err_cancels_timer() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request: build_request("session-1"),
            },
            Utc::now(),
        );
        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );
        let challenge = actions
            .iter()
            .find_map(|action| match action {
                PairingAction::Send {
                    message: PairingMessage::Challenge(challenge),
                    ..
                } => Some(challenge.clone()),
                _ => None,
            })
            .expect("challenge action");

        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash: hash_pin(&challenge.pin).expect("hash pin"),
            accepted: true,
        };
        let (state, _actions) = sm.handle_event(
            PairingEvent::RecvResponse {
                session_id: challenge.session_id.clone(),
                response,
            },
            Utc::now(),
        );

        let session_id = match state {
            PairingState::Finalizing { session_id, .. } => session_id,
            _ => panic!("Expected Finalizing state, got {:?}", state),
        };

        let (_state, actions) = sm.handle_event(
            PairingEvent::PersistErr {
                session_id,
                error: "persist failed".to_string(),
            },
            Utc::now(),
        );

        assert!(actions.iter().any(|action| matches!(
            action,
            PairingAction::CancelTimer {
                kind: TimeoutKind::Persist,
                ..
            }
        )));
    }

    #[test]
    fn build_paired_device_includes_peer_device_name() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        // Simulate receiving a request which sets peer_device_name
        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-2".to_string(),
            peer_id: "peer-remote".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        };

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: "peer-remote".to_string(),
                request,
            },
            Utc::now(),
        );

        // Accept and send challenge
        let (_state, actions) = sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );

        let challenge = actions
            .iter()
            .find_map(|action| match action {
                PairingAction::Send {
                    message: PairingMessage::Challenge(challenge),
                    ..
                } => Some(challenge.clone()),
                _ => None,
            })
            .expect("challenge action");

        // Receive response to trigger build_paired_device
        let response = PairingResponse {
            session_id: "session-1".to_string(),
            pin_hash: hash_pin(&challenge.pin).expect("hash pin"),
            accepted: true,
        };

        let (state, _actions) = sm.handle_event(
            PairingEvent::RecvResponse {
                session_id: "session-1".to_string(),
                response,
            },
            Utc::now(),
        );

        // Extract the paired_device from Finalizing state
        if let PairingState::Finalizing { paired_device, .. } = state {
            assert_eq!(paired_device.device_name, "PeerDevice");
        } else {
            panic!("Expected Finalizing state, got {:?}", state);
        }
    }

    #[test]
    fn build_paired_device_uses_unknown_device_when_name_missing() {
        // Create a state machine and manually set required context fields
        // without setting peer_device_name to test the fallback
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        // Manually set context fields to simulate a state where peer_device_name is None
        sm.context.peer_id = Some("peer-remote".to_string());
        sm.context.peer_identity_pubkey = Some(vec![2; 32]);
        // peer_device_name is intentionally left as None

        // Call build_paired_device directly to test the fallback
        let now = Utc::now();
        let result = sm.build_paired_device(now);

        match result {
            Ok(device) => {
                assert_eq!(device.device_name, "Unknown Device");
                assert_eq!(device.pairing_state, PairedDeviceState::Pending);
            }
            Err(e) => panic!("Expected Ok, got Err: {:?}", e),
        }
    }

    #[test]
    fn initiator_updates_peer_device_name_on_challenge() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );

        let challenge = PairingChallenge {
            session_id: "session-1".to_string(),
            pin: "123456".to_string(),
            device_name: "ResponderDevice".to_string(),
            device_id: "device-2".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        };

        sm.handle_event(
            PairingEvent::RecvChallenge {
                session_id: "session-1".to_string(),
                challenge,
            },
            Utc::now(),
        );

        assert_eq!(
            sm.context.peer_device_name,
            Some("ResponderDevice".to_string())
        );
    }

    #[test]
    fn initiator_updates_peer_device_name_on_confirm() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );

        let challenge = PairingChallenge {
            session_id: "session-1".to_string(),
            pin: "123456".to_string(),
            device_name: "OldName".to_string(),
            device_id: "device-2".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        };
        sm.handle_event(
            PairingEvent::RecvChallenge {
                session_id: "session-1".to_string(),
                challenge,
            },
            Utc::now(),
        );
        sm.handle_event(
            PairingEvent::UserAccept {
                session_id: "session-1".to_string(),
            },
            Utc::now(),
        );

        let confirm = PairingConfirm {
            session_id: "session-1".to_string(),
            success: true,
            error: None,
            sender_device_name: "NewName".to_string(),
            device_id: "device-2".to_string(),
        };

        sm.handle_event(
            PairingEvent::RecvConfirm {
                session_id: "session-1".to_string(),
                confirm,
            },
            Utc::now(),
        );

        assert_eq!(sm.context.peer_device_name, Some("NewName".to_string()));
    }

    #[test]
    fn recv_request_uses_sender_peer_id_not_request_peer_id() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-2".to_string(),
            peer_id: "spoofed-peer-id".to_string(), // Malicious/Wrong ID
            identity_pubkey: vec![2; 32],
            nonce: vec![9; 16],
        };

        let sender_peer_id = "trusted-sender-id".to_string();

        sm.handle_event(
            PairingEvent::RecvRequest {
                session_id: "session-1".to_string(),
                sender_peer_id: sender_peer_id.clone(),
                request,
            },
            Utc::now(),
        );

        // The context should reflect the trusted sender_peer_id, not the spoofed one
        assert_eq!(sm.context.peer_id, Some(sender_peer_id));
    }

    #[test]
    fn initiator_user_reject_from_request_sent_sends_reject_and_cancels() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        let (state, _) = sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );
        assert!(matches!(state, PairingState::RequestSent { .. }));

        let session_id = match &state {
            PairingState::RequestSent { session_id } => session_id.clone(),
            _ => panic!("expected RequestSent"),
        };

        let (state, actions) = sm.handle_event(
            PairingEvent::UserReject {
                session_id: session_id.clone(),
            },
            Utc::now(),
        );

        assert!(
            matches!(state, PairingState::Cancelled { .. }),
            "expected Cancelled, got {:?}",
            state
        );

        let has_send = actions.iter().any(|a| {
            matches!(
                a,
                PairingAction::Send {
                    message: PairingMessage::Reject(_),
                    ..
                }
            )
        });
        assert!(has_send, "should send Reject message to peer");

        let has_cancel_timer = actions.iter().any(|a| {
            matches!(
                a,
                PairingAction::CancelTimer {
                    kind: TimeoutKind::WaitingChallenge,
                    ..
                }
            )
        });
        assert!(has_cancel_timer, "should cancel WaitingChallenge timer");

        let has_emit = actions
            .iter()
            .any(|a| matches!(a, PairingAction::EmitResult { success: false, .. }));
        assert!(has_emit, "should emit failure result");
    }

    #[test]
    fn initiator_user_cancel_from_request_sent_sends_cancel_and_cancels() {
        let mut sm = PairingStateMachine::new_with_local_identity(
            "LocalDevice".to_string(),
            "device-1".to_string(),
            vec![1; 32],
        );

        let (state, _) = sm.handle_event(
            PairingEvent::StartPairing {
                role: PairingRole::Initiator,
                peer_id: "peer-2".to_string(),
            },
            Utc::now(),
        );
        let session_id = match &state {
            PairingState::RequestSent { session_id } => session_id.clone(),
            _ => panic!("expected RequestSent"),
        };

        let (state, actions) = sm.handle_event(
            PairingEvent::UserCancel {
                session_id: session_id.clone(),
            },
            Utc::now(),
        );

        assert!(
            matches!(state, PairingState::Cancelled { .. }),
            "expected Cancelled, got {:?}",
            state
        );

        let has_send = actions.iter().any(|a| {
            matches!(
                a,
                PairingAction::Send {
                    message: PairingMessage::Cancel(_),
                    ..
                }
            )
        });
        assert!(has_send, "should send Cancel message to peer");
    }
}
