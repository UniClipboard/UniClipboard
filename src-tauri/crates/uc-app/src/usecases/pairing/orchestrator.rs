//! Pairing protocol orchestrator
//!
//! 这个模块负责编排配对状态机,将网络事件、用户输入和定时器事件转换为状态机事件,
//! 并执行状态机返回的动作。
//!
//! # Architecture / 架构
//!
//! ```text
//! Network/User/Timer Events
//!   ↓
//! PairingOrchestrator (converts events)
//!   ↓
//! PairingStateMachine (pure state transitions)
//!   ↓
//! PairingActions (executed by orchestrator)
//!   ↓
//! Network/User/Persistence side effects
//! ```

use anyhow::{Context, Result};

use super::{PairingDomainEvent, PairingEventPort, PairingFacade};
use crate::usecases::pairing::staged_paired_device_store;
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, VecDeque};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::{info_span, Instrument};

use uc_core::{
    network::{
        pairing_state_machine::{
            FailureReason, PairingAction, PairingEvent, PairingPolicy, PairingRole, PairingState,
            PairingStateMachine, SessionId, TimeoutKind,
        },
        protocol::{
            PairingChallenge, PairingChallengeResponse, PairingConfirm, PairingKeyslotOffer,
            PairingRequest,
        },
    },
    ports::PairedDeviceRepositoryPort,
    settings::model::Settings,
};

/// 配对编排器配置
#[derive(Debug, Clone)]
pub struct PairingConfig {
    /// 步骤超时时间(秒)
    pub step_timeout_secs: i64,
    /// 用户确认超时时间(秒)
    pub user_verification_timeout_secs: i64,
    /// 会话超时时间(秒)
    pub session_timeout_secs: i64,
    /// 最大重试次数
    pub max_retries: u8,
    /// 协议版本
    pub protocol_version: String,
}

impl Default for PairingConfig {
    fn default() -> Self {
        Self::from_settings(&Settings::default())
    }
}

impl PairingConfig {
    pub fn from_settings(settings: &Settings) -> Self {
        let pairing = &settings.pairing;
        let step = pairing.step_timeout.as_secs().min(i64::MAX as u64) as i64;
        let verify = pairing
            .user_verification_timeout
            .as_secs()
            .min(i64::MAX as u64) as i64;
        let session = pairing.session_timeout.as_secs().min(i64::MAX as u64) as i64;

        Self {
            step_timeout_secs: step.max(1),
            user_verification_timeout_secs: verify.max(1),
            session_timeout_secs: session.max(1),
            max_retries: pairing.max_retries.max(1),
            protocol_version: pairing.protocol_version.clone(),
        }
    }
}

/// 配对编排器
///
/// 负责管理多个并发的配对会话,协调状态机与外部世界的交互。
#[derive(Clone)]
pub struct PairingOrchestrator {
    /// 配置
    config: PairingConfig,
    /// 活跃的配对会话 (session_id -> state machine)
    sessions: Arc<RwLock<HashMap<SessionId, PairingSessionContext>>>,
    /// 会话对应的对端信息
    session_peers: Arc<RwLock<HashMap<SessionId, PairingPeerInfo>>>,
    /// 配对设备仓库
    device_repo: Arc<dyn PairedDeviceRepositoryPort + Send + Sync + 'static>,
    /// 本地设备身份
    local_identity: LocalDeviceInfo,
    /// 动作发送器
    action_tx: mpsc::Sender<PairingAction>,
    /// 配对事件订阅者
    event_senders: Arc<Mutex<Vec<mpsc::Sender<PairingDomainEvent>>>>,
}

/// 配对会话上下文
struct PairingSessionContext {
    /// 状态机
    state_machine: PairingStateMachine,
    /// 会话创建时间
    created_at: DateTime<Utc>,
    /// 定时器句柄
    timers: Mutex<HashMap<TimeoutKind, tokio::task::AbortHandle>>,
}

#[derive(Debug, Clone)]
pub struct PairingPeerInfo {
    pub peer_id: String,
    pub device_name: Option<String>,
}

/// 本地设备信息
#[derive(Clone)]
struct LocalDeviceInfo {
    /// 设备名称
    device_name: String,
    /// 设备ID
    device_id: String,
    /// 本地身份公钥
    identity_pubkey: Vec<u8>,
    /// 本地 PeerID
    peer_id: String,
}

impl PairingOrchestrator {
    /// 创建新的配对编排器
    pub fn new(
        config: PairingConfig,
        device_repo: Arc<dyn PairedDeviceRepositoryPort + Send + Sync + 'static>,
        local_device_name: String,
        local_device_id: String,
        local_peer_id: String,
        local_identity_pubkey: Vec<u8>,
    ) -> (Self, mpsc::Receiver<PairingAction>) {
        let (action_tx, action_rx) = mpsc::channel(100);

        let orchestrator = Self {
            config,
            sessions: Arc::new(RwLock::new(HashMap::new())),
            session_peers: Arc::new(RwLock::new(HashMap::new())),
            device_repo,
            local_identity: LocalDeviceInfo {
                device_name: local_device_name,
                device_id: local_device_id,
                identity_pubkey: local_identity_pubkey,
                peer_id: local_peer_id,
            },
            action_tx,
            event_senders: Arc::new(Mutex::new(Vec::new())),
        };

        (orchestrator, action_rx)
    }

    /// 发起配对 (Initiator)
    pub async fn initiate_pairing(&self, peer_id: String) -> Result<SessionId> {
        let span = info_span!(
            "pairing.initiate",
            peer_id = %peer_id
        );
        async {
            let policy = self.build_policy();
            let mut state_machine = PairingStateMachine::new_with_local_identity_and_policy(
                self.local_identity.device_name.clone(),
                self.local_identity.device_id.clone(),
                self.local_identity.identity_pubkey.clone(),
                policy,
            );
            let (state, actions) = state_machine.handle_event(
                PairingEvent::StartPairing {
                    role: PairingRole::Initiator,
                    peer_id: peer_id.clone(),
                },
                Utc::now(),
            );

            let session_id = match state {
                PairingState::RequestSent { session_id } => session_id,
                _ => {
                    return Err(anyhow::anyhow!(
                        "unexpected state after StartPairing: {:?}",
                        state
                    ))
                }
            };
            self.record_session_peer(&session_id, peer_id.clone(), None)
                .await;

            let context = PairingSessionContext {
                state_machine,
                created_at: Utc::now(),
                timers: Mutex::new(HashMap::new()),
            };

            self.sessions
                .write()
                .await
                .insert(session_id.clone(), context);

            for action in actions {
                self.execute_action(&session_id, &peer_id, action).await?;
            }

            Ok(session_id)
        }
        .instrument(span)
        .await
    }

    /// 处理收到的配对请求 (Responder)
    pub async fn handle_incoming_request(
        &self,
        peer_id: String,
        request: PairingRequest,
    ) -> Result<()> {
        if request.peer_id != self.local_identity.peer_id {
            return Err(anyhow::anyhow!(
                "Request target peer_id mismatch: expected {}, got {}",
                self.local_identity.peer_id,
                request.peer_id
            ));
        }

        let session_id = request.session_id.clone();
        let span = info_span!(
            "pairing.handle_request",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            self.record_session_peer(
                &session_id,
                peer_id.clone(),
                Some(request.device_name.clone()),
            )
            .await;

            let policy = self.build_policy();
            let mut state_machine = PairingStateMachine::new_with_local_identity_and_policy(
                self.local_identity.device_name.clone(),
                self.local_identity.device_id.clone(),
                self.local_identity.identity_pubkey.clone(),
                policy,
            );
            let (_state, actions) = state_machine.handle_event(
                PairingEvent::RecvRequest {
                    session_id: session_id.clone(),
                    sender_peer_id: peer_id.clone(),
                    request,
                },
                Utc::now(),
            );

            let context = PairingSessionContext {
                state_machine,
                created_at: Utc::now(),
                timers: Mutex::new(HashMap::new()),
            };

            self.sessions
                .write()
                .await
                .insert(session_id.clone(), context);

            // 执行动作(如果有)
            for action in actions {
                self.execute_action(&session_id, &peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Challenge (Initiator)
    pub async fn handle_challenge(
        &self,
        session_id: &str,
        peer_id: &str,
        challenge: PairingChallenge,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_challenge",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            self.record_session_peer(
                session_id,
                peer_id.to_string(),
                Some(challenge.device_name.clone()),
            )
            .await;
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvChallenge {
                        session_id: session_id.to_string(),
                        challenge,
                    },
                    Utc::now(),
                );
                actions
            };

            // 执行动作(包括展示验证UI)
            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的KeyslotOffer (Initiator)
    pub async fn handle_keyslot_offer(
        &self,
        session_id: &str,
        peer_id: &str,
        offer: PairingKeyslotOffer,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_keyslot_offer",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let has_keyslot = offer.keyslot_file.as_ref().is_some();
            let has_challenge = offer.challenge.as_ref().is_some();
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                has_keyslot,
                has_challenge,
                "Handling pairing keyslot offer"
            );
            let keyslot_file = match offer.keyslot_file {
                Some(keyslot_file) => keyslot_file,
                None => {
                    tracing::warn!(
                        session_id = %session_id,
                        peer_id = %peer_id,
                        "Keyslot offer missing keyslot file"
                    );
                    return Ok(());
                }
            };
            let challenge = match offer.challenge {
                Some(challenge) => challenge,
                None => {
                    tracing::warn!(
                        session_id = %session_id,
                        peer_id = %peer_id,
                        "Keyslot offer missing challenge"
                    );
                    return Ok(());
                }
            };
            self.emit_event(PairingDomainEvent::KeyslotReceived {
                session_id: session_id.to_string(),
                peer_id: peer_id.to_string(),
                keyslot_file,
                challenge,
            })
            .await;
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的ChallengeResponse (Responder)
    pub async fn handle_challenge_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: PairingChallengeResponse,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_challenge_response",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let has_encrypted_challenge = response.encrypted_challenge.as_ref().is_some();
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                has_encrypted_challenge,
                "Handling pairing challenge response"
            );
            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Response (Responder)
    pub async fn handle_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: uc_core::network::protocol::PairingResponse,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_response",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                accepted = %response.accepted,
                "Handling pairing response from initiator"
            );
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (new_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvResponse {
                        session_id: session_id.to_string(),
                        response,
                    },
                    Utc::now(),
                );
                tracing::debug!(
                    session_id = %session_id,
                    new_state = ?new_state,
                    num_actions = actions.len(),
                    "Response handled, new state and actions generated"
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 用户接受配对 (验证短码匹配)
    pub async fn user_accept_pairing(&self, session_id: &str) -> Result<()> {
        let span = info_span!("pairing.user_accept", session_id = %session_id);
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::UserAccept {
                        session_id: session_id.to_string(),
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, "", action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 用户拒绝配对
    pub async fn user_reject_pairing(&self, session_id: &str) -> Result<()> {
        let span = info_span!("pairing.user_reject", session_id = %session_id);
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::UserReject {
                        session_id: session_id.to_string(),
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, "", action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Confirm (双方)
    pub async fn handle_confirm(
        &self,
        session_id: &str,
        peer_id: &str,
        confirm: PairingConfirm,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_confirm",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            tracing::info!(
                session_id = %session_id,
                peer_id = %peer_id,
                success = %confirm.success,
                error = ?confirm.error,
                "Handling pairing confirm message"
            );
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (new_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvConfirm {
                        session_id: session_id.to_string(),
                        confirm,
                    },
                    Utc::now(),
                );
                tracing::debug!(
                    session_id = %session_id,
                    new_state = ?new_state,
                    num_actions = actions.len(),
                    "Confirm handled, new state and actions generated"
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Reject (双方)
    pub async fn handle_reject(&self, session_id: &str, peer_id: &str) -> Result<()> {
        let span = info_span!(
            "pairing.handle_reject",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvReject {
                        session_id: session_id.to_string(),
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Cancel (双方)
    pub async fn handle_cancel(&self, session_id: &str, peer_id: &str) -> Result<()> {
        let span = info_span!(
            "pairing.handle_cancel",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvCancel {
                        session_id: session_id.to_string(),
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理收到的Busy (双方)
    pub async fn handle_busy(&self, session_id: &str, peer_id: &str) -> Result<()> {
        let span = info_span!(
            "pairing.handle_busy",
            session_id = %session_id,
            peer_id = %peer_id
        );
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                let context = sessions.get_mut(session_id).context("Session not found")?;
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::RecvBusy {
                        session_id: session_id.to_string(),
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 处理传输层错误
    pub async fn handle_transport_error(
        &self,
        session_id: &str,
        peer_id: &str,
        error: String,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_transport_error",
            session_id = %session_id,
            peer_id = %peer_id,
            error = %error
        );
        async {
            let actions = {
                let mut sessions = self.sessions.write().await;
                // If session is already gone, we don't care
                if let Some(context) = sessions.get_mut(session_id) {
                    let (_state, actions) = context.state_machine.handle_event(
                        PairingEvent::TransportError {
                            session_id: session_id.to_string(),
                            error: error.clone(),
                        },
                        Utc::now(),
                    );
                    actions
                } else {
                    vec![]
                }
            };

            for action in actions {
                self.execute_action(session_id, peer_id, action).await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    /// 执行单个动作
    async fn execute_action(
        &self,
        session_id: &str,
        _peer_id: &str,
        action: PairingAction,
    ) -> Result<()> {
        Self::execute_action_inner(
            self.action_tx.clone(),
            self.sessions.clone(),
            self.session_peers.clone(),
            self.event_senders.clone(),
            self.device_repo.clone(),
            session_id.to_string(),
            action,
        )
        .await
    }

    fn execute_action_inner(
        action_tx: mpsc::Sender<PairingAction>,
        sessions: Arc<RwLock<HashMap<SessionId, PairingSessionContext>>>,
        session_peers: Arc<RwLock<HashMap<SessionId, PairingPeerInfo>>>,
        event_senders: Arc<Mutex<Vec<mpsc::Sender<PairingDomainEvent>>>>,
        device_repo: Arc<dyn PairedDeviceRepositoryPort + Send + Sync + 'static>,
        session_id: String,
        action: PairingAction,
    ) -> impl Future<Output = Result<()>> + Send {
        async move {
            let mut queue = VecDeque::from([action]);

            while let Some(action) = queue.pop_front() {
                match action {
                    PairingAction::Send {
                        peer_id: target_peer,
                        message,
                    } => {
                        action_tx
                            .send(PairingAction::Send {
                                peer_id: target_peer,
                                message,
                            })
                            .await
                            .context("Failed to queue send action")?;
                    }
                    PairingAction::ShowVerification {
                        session_id: action_session_id,
                        short_code,
                        local_fingerprint,
                        peer_fingerprint,
                        peer_display_name,
                    } => {
                        let short_code_clone = short_code.clone();
                        let local_fingerprint_clone = local_fingerprint.clone();
                        let peer_fingerprint_clone = peer_fingerprint.clone();
                        let peer_id_for_event = {
                            let peers = session_peers.read().await;
                            peers
                                .get(&action_session_id)
                                .map(|info| info.peer_id.clone())
                        };
                        if let Some(peer_id) = peer_id_for_event {
                            Self::emit_event_to_senders(
                                event_senders.clone(),
                                PairingDomainEvent::PairingVerificationRequired {
                                    session_id: action_session_id.clone(),
                                    peer_id,
                                    short_code: short_code_clone,
                                    local_fingerprint: local_fingerprint_clone,
                                    peer_fingerprint: peer_fingerprint_clone,
                                },
                            )
                            .await;
                        } else {
                            tracing::warn!(
                                session_id = %action_session_id,
                                "Pairing verification event missing peer info"
                            );
                        }
                        tracing::debug!(
                            session_id = %action_session_id,
                            action = "ShowVerification",
                            "Sending UI action to frontend"
                        );
                        action_tx
                            .send(PairingAction::ShowVerification {
                                session_id: action_session_id,
                                short_code,
                                local_fingerprint,
                                peer_fingerprint,
                                peer_display_name,
                            })
                            .await
                            .context("Failed to queue ui action")?;
                    }
                    PairingAction::ShowVerifying {
                        session_id: verifying_session_id,
                        peer_display_name,
                    } => {
                        tracing::debug!(
                            session_id = %verifying_session_id,
                            action = "ShowVerifying",
                            "Sending UI action to frontend"
                        );
                        action_tx
                            .send(PairingAction::ShowVerifying {
                                session_id: verifying_session_id,
                                peer_display_name,
                            })
                            .await
                            .context("Failed to queue ui action")?;
                    }
                    PairingAction::EmitResult {
                        session_id: result_session_id,
                        success,
                        error,
                    } => {
                        let result_session_id_for_send = result_session_id.clone();
                        let error_for_send = error.clone();
                        tracing::info!(
                            session_id = %result_session_id,
                            success = %success,
                            error = ?error,
                            "Emitting pairing result to frontend"
                        );
                        action_tx
                            .send(PairingAction::EmitResult {
                                session_id: result_session_id_for_send,
                                success,
                                error: error_for_send,
                            })
                            .await
                            .context("Failed to queue emit result action")?;
                        let peer_id = {
                            let peers = session_peers.read().await;
                            peers
                                .get(&result_session_id)
                                .map(|peer| peer.peer_id.clone())
                                .unwrap_or_default()
                        };
                        if peer_id.is_empty() {
                            tracing::warn!(
                                session_id = %result_session_id,
                                "Pairing result emitted without peer id"
                            );
                        }
                        let event = if success {
                            PairingDomainEvent::PairingSucceeded {
                                session_id: result_session_id.clone(),
                                peer_id,
                            }
                        } else {
                            let reason =
                                error.clone().map(FailureReason::Other).unwrap_or_else(|| {
                                    FailureReason::Other("Pairing failed".to_string())
                                });
                            PairingDomainEvent::PairingFailed {
                                session_id: result_session_id.clone(),
                                peer_id,
                                reason,
                            }
                        };
                        Self::emit_event_to_senders(event_senders.clone(), event).await;
                    }
                    PairingAction::PersistPairedDevice {
                        session_id: _,
                        device,
                    } => {
                        tracing::info!(
                            session_id = %session_id,
                            peer_id = %device.peer_id,
                            "Persisting paired device before verification completion"
                        );
                        let peer_id = device.peer_id.to_string();
                        staged_paired_device_store::stage(&session_id, device.clone());

                        let persist_result = device_repo.upsert(device).await;

                        let actions = {
                            let mut sessions = sessions.write().await;
                            if let Some(context) = sessions.get_mut(&session_id) {
                                let event = match persist_result {
                                    Ok(()) => PairingEvent::PersistOk {
                                        session_id: session_id.clone(),
                                        device_id: peer_id,
                                    },
                                    Err(err) => PairingEvent::PersistErr {
                                        session_id: session_id.clone(),
                                        error: err.to_string(),
                                    },
                                };
                                let (_state, actions) =
                                    context.state_machine.handle_event(event, Utc::now());
                                tracing::debug!(
                                    session_id = %session_id,
                                    num_actions = actions.len(),
                                    "Persist event generated actions"
                                );
                                actions
                            } else {
                                vec![]
                            }
                        };
                        queue.extend(actions);
                    }
                    PairingAction::StartTimer {
                        session_id: action_session_id,
                        kind,
                        deadline,
                    } => {
                        let sessions_for_timer = sessions.clone();
                        let peers_for_timer = session_peers.clone();
                        let event_senders_for_timer = event_senders.clone();
                        let mut sessions = sessions.write().await;
                        let context = sessions
                            .get_mut(&action_session_id)
                            .context("Session not found")?;
                        {
                            let mut timers = context.timers.lock().await;
                            if let Some(handle) = timers.remove(&kind) {
                                handle.abort();
                            }
                        }

                        let action_tx = action_tx.clone();
                        let sessions = sessions_for_timer;
                        let session_peers = peers_for_timer;
                        let event_senders = event_senders_for_timer;
                        let device_repo = device_repo.clone();
                        let session_id_for_log = action_session_id.clone();
                        let sleep_duration = deadline
                            .signed_duration_since(Utc::now())
                            .to_std()
                            .unwrap_or_else(|_| std::time::Duration::from_secs(0));
                        let future = async move {
                            tokio::time::sleep(sleep_duration).await;
                            if let Err(error) = Self::handle_timeout(
                                action_tx,
                                sessions,
                                session_peers,
                                event_senders,
                                device_repo,
                                action_session_id,
                                kind,
                            )
                            .await
                            {
                                tracing::error!(
                                    %session_id_for_log,
                                    ?kind,
                                    error = ?error,
                                    "pairing timer handling failed"
                                );
                            }
                        };
                        let future: Pin<Box<dyn Future<Output = ()> + Send>> = Box::pin(future);
                        let handle = tokio::spawn(future);

                        let abort_handle = handle.abort_handle();
                        let mut timers = context.timers.lock().await;
                        timers.insert(kind, abort_handle);
                    }
                    PairingAction::CancelTimer {
                        session_id: action_session_id,
                        kind,
                    } => {
                        let mut sessions = sessions.write().await;
                        let context = sessions
                            .get_mut(&action_session_id)
                            .context("Session not found")?;
                        let mut timers = context.timers.lock().await;
                        if let Some(handle) = timers.remove(&kind) {
                            handle.abort();
                        }
                    }
                    PairingAction::LogTransition { .. } => {
                        // 日志已记录,无需额外操作
                    }
                    PairingAction::NoOp => {}
                }
            }

            Ok(())
        }
    }

    async fn record_session_peer(
        &self,
        session_id: &str,
        peer_id: String,
        device_name: Option<String>,
    ) {
        let mut peers = self.session_peers.write().await;
        let entry = peers
            .entry(session_id.to_string())
            .or_insert_with(|| PairingPeerInfo {
                peer_id: peer_id.clone(),
                device_name: None,
            });
        entry.peer_id = peer_id;
        if device_name.is_some() {
            entry.device_name = device_name;
        }
    }

    pub async fn get_session_peer(&self, session_id: &str) -> Option<PairingPeerInfo> {
        let peers = self.session_peers.read().await;
        peers.get(session_id).cloned()
    }

    pub async fn get_session_role(&self, session_id: &str) -> Option<PairingRole> {
        let sessions = self.sessions.read().await;
        sessions
            .get(session_id)
            .and_then(|ctx| ctx.state_machine.role())
    }

    fn build_policy(&self) -> PairingPolicy {
        PairingPolicy {
            step_timeout_secs: self.config.step_timeout_secs,
            user_verification_timeout_secs: self.config.user_verification_timeout_secs,
            max_retries: self.config.max_retries,
            protocol_version: self.config.protocol_version.clone(),
        }
    }

    /// 清理过期会话
    pub async fn cleanup_expired_sessions(&self) {
        let span = info_span!("pairing.cleanup_expired_sessions");
        async {
            let mut sessions = self.sessions.write().await;
            let now = Utc::now();
            let timeout = Duration::seconds(self.config.session_timeout_secs);
            let expired_ids: Vec<SessionId> = sessions
                .iter()
                .filter_map(|(session_id, context)| {
                    let expired = now.signed_duration_since(context.created_at).num_seconds()
                        >= timeout.num_seconds();
                    if expired {
                        Some(session_id.clone())
                    } else {
                        None
                    }
                })
                .collect();

            let mut expired_contexts = Vec::with_capacity(expired_ids.len());
            for session_id in &expired_ids {
                if let Some(context) = sessions.remove(session_id) {
                    expired_contexts.push((session_id.clone(), context));
                }
            }
            drop(sessions);

            if !expired_ids.is_empty() {
                let mut peers = self.session_peers.write().await;
                for session_id in &expired_ids {
                    peers.remove(session_id);
                }
            }

            for (_session_id, context) in expired_contexts {
                let mut timers = context.timers.lock().await;
                for (_kind, handle) in timers.drain() {
                    handle.abort();
                }
            }
        }
        .instrument(span)
        .await
    }

    async fn handle_timeout(
        action_tx: mpsc::Sender<PairingAction>,
        sessions: Arc<RwLock<HashMap<SessionId, PairingSessionContext>>>,
        session_peers: Arc<RwLock<HashMap<SessionId, PairingPeerInfo>>>,
        event_senders: Arc<Mutex<Vec<mpsc::Sender<PairingDomainEvent>>>>,
        device_repo: Arc<dyn PairedDeviceRepositoryPort + Send + Sync + 'static>,
        session_id: String,
        kind: TimeoutKind,
    ) -> Result<()> {
        let span = info_span!(
            "pairing.handle_timeout",
            session_id = %session_id,
            kind = ?kind
        );
        async {
            let actions = {
                let mut sessions = sessions.write().await;
                let context = sessions.get_mut(&session_id).context("Session not found")?;
                {
                    let mut timers = context.timers.lock().await;
                    timers.remove(&kind);
                }
                let (_state, actions) = context.state_machine.handle_event(
                    PairingEvent::Timeout {
                        session_id: session_id.clone(),
                        kind,
                    },
                    Utc::now(),
                );
                actions
            };

            for action in actions {
                Self::execute_action_inner(
                    action_tx.clone(),
                    sessions.clone(),
                    session_peers.clone(),
                    event_senders.clone(),
                    device_repo.clone(),
                    session_id.clone(),
                    action,
                )
                .await?;
            }

            Ok(())
        }
        .instrument(span)
        .await
    }

    async fn emit_event(&self, event: PairingDomainEvent) {
        Self::emit_event_to_senders(self.event_senders.clone(), event).await;
    }

    async fn emit_event_to_senders(
        event_senders: Arc<Mutex<Vec<mpsc::Sender<PairingDomainEvent>>>>,
        event: PairingDomainEvent,
    ) {
        let senders = { event_senders.lock().await.clone() };
        for sender in senders {
            if sender.send(event.clone()).await.is_err() {
                tracing::debug!("Pairing event receiver dropped");
            }
        }
    }
}

#[async_trait::async_trait]
impl PairingFacade for PairingOrchestrator {
    async fn initiate_pairing(&self, peer_id: String) -> anyhow::Result<SessionId> {
        Self::initiate_pairing(self, peer_id).await
    }

    async fn user_accept_pairing(&self, session_id: &str) -> anyhow::Result<()> {
        Self::user_accept_pairing(self, session_id).await
    }

    async fn user_reject_pairing(&self, session_id: &str) -> anyhow::Result<()> {
        Self::user_reject_pairing(self, session_id).await
    }

    async fn handle_challenge_response(
        &self,
        session_id: &str,
        peer_id: &str,
        response: PairingChallengeResponse,
    ) -> anyhow::Result<()> {
        Self::handle_challenge_response(self, session_id, peer_id, response).await
    }
}

#[async_trait::async_trait]
impl PairingEventPort for PairingOrchestrator {
    async fn subscribe(&self) -> anyhow::Result<mpsc::Receiver<PairingDomainEvent>> {
        let (event_tx, event_rx) = mpsc::channel(100);
        let mut senders = self.event_senders.lock().await;
        senders.push(event_tx);
        Ok(event_rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::usecases::pairing::staged_paired_device_store;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;
    use tokio::time::timeout;
    use uc_core::crypto::pin_hash::hash_pin;
    use uc_core::network::paired_device::{PairedDevice, PairingState};
    use uc_core::network::pairing_state_machine::FailureReason;
    use uc_core::network::protocol::{PairingRequest, PairingResponse};
    use uc_core::network::PairingMessage;
    use uc_core::security::model::{
        EncryptedBlob, EncryptionAlgo, EncryptionFormatVersion, KdfAlgorithm, KdfParams,
        KdfParamsV1, KeyScope, KeySlotFile, KeySlotVersion,
    };

    struct MockDeviceRepository;

    #[derive(Default)]
    struct CountingDeviceRepository {
        upsert_calls: AtomicUsize,
    }

    struct FailingDeviceRepository;

    impl CountingDeviceRepository {
        fn upsert_calls(&self) -> usize {
            self.upsert_calls.load(Ordering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for MockDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for CountingDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            self.upsert_calls.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl PairedDeviceRepositoryPort for FailingDeviceRepository {
        async fn get_by_peer_id(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<Option<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(None)
        }

        async fn list_all(
            &self,
        ) -> Result<Vec<PairedDevice>, uc_core::ports::errors::PairedDeviceRepositoryError>
        {
            Ok(vec![])
        }

        async fn upsert(
            &self,
            _device: PairedDevice,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Err(
                uc_core::ports::errors::PairedDeviceRepositoryError::Storage(
                    "injected upsert failure".to_string(),
                ),
            )
        }

        async fn set_state(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _state: PairingState,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn update_last_seen(
            &self,
            _peer_id: &uc_core::ids::PeerId,
            _last_seen_at: chrono::DateTime<chrono::Utc>,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }

        async fn delete(
            &self,
            _peer_id: &uc_core::ids::PeerId,
        ) -> Result<(), uc_core::ports::errors::PairedDeviceRepositoryError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn pairing_persists_device_before_marking_persist_ok() {
        staged_paired_device_store::clear();
        let device_repo = Arc::new(CountingDeviceRepository::default());
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            PairingConfig::default(),
            device_repo.clone(),
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        orchestrator
            .execute_action(
                "session-staged",
                "peer-remote",
                PairingAction::PersistPairedDevice {
                    session_id: "session-staged".to_string(),
                    device: PairedDevice {
                        peer_id: uc_core::ids::PeerId::from("peer-remote"),
                        pairing_state: PairingState::Pending,
                        identity_fingerprint: "fp-remote".to_string(),
                        paired_at: Utc::now(),
                        last_seen_at: None,
                        device_name: "Remote Device".to_string(),
                    },
                },
            )
            .await
            .expect("stage paired device");

        assert_eq!(device_repo.upsert_calls(), 1);

        let staged = staged_paired_device_store::take_by_peer_id("peer-remote");
        assert!(staged.is_some());
    }

    #[tokio::test]
    async fn test_orchestrator_creation() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (_orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
        );
    }

    #[tokio::test]
    async fn test_initiate_pairing() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
        );

        let session_id = orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .unwrap();
        assert!(!session_id.is_empty());
    }

    #[tokio::test]
    async fn initiate_pairing_emits_request_action() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
        );

        let _session_id = orchestrator
            .initiate_pairing("peer-remote".to_string())
            .await
            .expect("initiate pairing");

        let action = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        assert!(matches!(
            action,
            PairingAction::Send {
                message: PairingMessage::Request(_),
                ..
            }
        ));
    }

    #[tokio::test]
    async fn test_cleanup_uses_configured_session_timeout() {
        let config = PairingConfig {
            step_timeout_secs: 1,
            user_verification_timeout_secs: 1,
            session_timeout_secs: 1,
            max_retries: 1,
            protocol_version: "1.0.0".to_string(),
        };
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
        );

        orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        orchestrator.cleanup_expired_sessions().await;

        let sessions = orchestrator.sessions.read().await;
        assert!(sessions.is_empty());

        let peers = orchestrator.session_peers.read().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_aborts_expired_session_timers() {
        let config = PairingConfig {
            step_timeout_secs: 1,
            user_verification_timeout_secs: 1,
            session_timeout_secs: 1,
            max_retries: 1,
            protocol_version: "1.0.0".to_string(),
        };
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "TestDevice".to_string(),
            "device-123".to_string(),
            "peer-456".to_string(),
            vec![0u8; 32],
        );

        let session_id = orchestrator
            .initiate_pairing("remote-peer".to_string())
            .await
            .expect("initiate pairing");

        let join_handle = tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        let abort_handle = join_handle.abort_handle();

        {
            let mut sessions = orchestrator.sessions.write().await;
            let context = sessions.get_mut(&session_id).expect("session context");
            let mut timers = context.timers.lock().await;
            timers.insert(TimeoutKind::Persist, abort_handle);
        }

        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        orchestrator.cleanup_expired_sessions().await;

        let join_result = join_handle.await;
        assert!(join_result.is_err());
    }

    #[test]
    fn test_pairing_config_from_settings() {
        let mut settings = Settings::default();
        settings.pairing.step_timeout = std::time::Duration::from_secs(20);
        settings.pairing.user_verification_timeout = std::time::Duration::from_secs(90);
        settings.pairing.session_timeout = std::time::Duration::from_secs(400);
        settings.pairing.max_retries = 5;
        settings.pairing.protocol_version = "2.0.0".to_string();

        let config = PairingConfig::from_settings(&settings);

        assert_eq!(config.step_timeout_secs, 20);
        assert_eq!(config.user_verification_timeout_secs, 90);
        assert_eq!(config.session_timeout_secs, 400);
        assert_eq!(config.max_retries, 5);
        assert_eq!(config.protocol_version, "2.0.0");
    }

    #[tokio::test]
    async fn test_handle_response_emits_confirm_action() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };

        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle request");
        orchestrator
            .user_accept_pairing("session-1")
            .await
            .expect("accept pairing");

        let challenge = loop {
            let challenge_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("challenge action timeout")
                .expect("challenge action missing");
            if let PairingAction::Send {
                message: PairingMessage::Challenge(challenge),
                ..
            } = challenge_action
            {
                break challenge;
            }
        };

        let pin_hash = hash_pin(&challenge.pin).expect("hash pin");
        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash,
            accepted: true,
        };

        orchestrator
            .handle_response(&challenge.session_id, "peer-remote", response)
            .await
            .expect("handle response");

        let confirm = loop {
            let confirm_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("confirm action timeout")
                .expect("confirm action missing");
            if let PairingAction::Send {
                message: PairingMessage::Confirm(confirm),
                ..
            } = confirm_action
            {
                break confirm;
            }
        };

        assert!(confirm.success);
        assert_eq!(confirm.sender_device_name, "LocalDevice");
        assert_eq!(confirm.device_id, "device-123");
    }

    #[tokio::test]
    async fn pairing_emits_failed_event_when_persist_upsert_fails() {
        staged_paired_device_store::clear();
        let config = PairingConfig::default();
        let device_repo = Arc::new(FailingDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        let request = PairingRequest {
            session_id: "session-persist-fail".to_string(),
            device_name: "PeerDevice".to_string(),
            device_id: "device-999".to_string(),
            peer_id: "peer-local".to_string(),
            identity_pubkey: vec![1; 32],
            nonce: vec![2; 16],
        };

        orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await
            .expect("handle request");
        orchestrator
            .user_accept_pairing("session-persist-fail")
            .await
            .expect("accept pairing");

        let challenge = loop {
            let challenge_action = timeout(Duration::from_secs(1), action_rx.recv())
                .await
                .expect("challenge action timeout")
                .expect("challenge action missing");
            if let PairingAction::Send {
                message: PairingMessage::Challenge(challenge),
                ..
            } = challenge_action
            {
                break challenge;
            }
        };

        let pin_hash = hash_pin(&challenge.pin).expect("hash pin");
        let response = PairingResponse {
            session_id: challenge.session_id.clone(),
            pin_hash,
            accepted: true,
        };

        orchestrator
            .handle_response(&challenge.session_id, "peer-remote", response)
            .await
            .expect("handle response");

        let mut reason: Option<FailureReason> = None;
        for _ in 0..4 {
            let event = timeout(Duration::from_secs(1), event_rx.recv())
                .await
                .expect("event timeout")
                .expect("event missing");
            if let crate::usecases::pairing::PairingDomainEvent::PairingFailed {
                reason: failed_reason,
                ..
            } = event
            {
                reason = Some(failed_reason);
                break;
            }
        }

        let reason = reason.expect("expected PairingFailed event");
        match reason {
            FailureReason::Other(message) => {
                assert!(message.contains("injected upsert failure"));
            }
            other => panic!("expected FailureReason::Other, got {:?}", other),
        }

        for _ in 0..2 {
            let maybe_event = timeout(Duration::from_millis(200), event_rx.recv()).await;
            match maybe_event {
                Ok(Some(crate::usecases::pairing::PairingDomainEvent::PairingSucceeded {
                    ..
                })) => {
                    panic!("unexpected PairingSucceeded event after persist failure");
                }
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => break,
            }
        }
    }

    #[tokio::test]
    async fn test_show_verification_is_forwarded_to_action_channel() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
        );

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::ShowVerification {
                    session_id: "session-1".to_string(),
                    short_code: "ABC123".to_string(),
                    local_fingerprint: "LOCAL".to_string(),
                    peer_fingerprint: "PEER".to_string(),
                    peer_display_name: "Peer".to_string(),
                },
            )
            .await
            .expect("execute action");

        let action = timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        assert!(matches!(action, PairingAction::ShowVerification { .. }));
    }

    #[tokio::test]
    async fn test_start_timer_records_handle() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-1".to_string(),
            vec![1; 32],
        );

        let session_id = orchestrator
            .initiate_pairing("peer-2".to_string())
            .await
            .unwrap();
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::StartTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                    deadline: Utc::now() + chrono::Duration::seconds(1),
                },
            )
            .await
            .unwrap();

        let sessions = orchestrator.sessions.read().await;
        let context = sessions.get(&session_id).expect("session");
        let timers = context.timers.lock().await;
        assert!(timers.contains_key(&TimeoutKind::WaitingChallenge));
    }

    #[tokio::test]
    async fn test_cancel_timer_removes_handle() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-1".to_string(),
            vec![1; 32],
        );

        let session_id = orchestrator
            .initiate_pairing("peer-2".to_string())
            .await
            .unwrap();
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::StartTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                    deadline: Utc::now() + chrono::Duration::seconds(1),
                },
            )
            .await
            .unwrap();

        {
            let sessions = orchestrator.sessions.read().await;
            let context = sessions.get(&session_id).expect("session");
            let timers = context.timers.lock().await;
            assert!(timers.contains_key(&TimeoutKind::WaitingChallenge));
        }
        orchestrator
            .execute_action(
                &session_id,
                "peer-2",
                PairingAction::CancelTimer {
                    session_id: session_id.clone(),
                    kind: TimeoutKind::WaitingChallenge,
                },
            )
            .await
            .unwrap();

        let sessions = orchestrator.sessions.read().await;
        let context = sessions.get(&session_id).expect("session");
        let timers = context.timers.lock().await;
        assert!(!timers.contains_key(&TimeoutKind::WaitingChallenge));
    }

    #[tokio::test]
    async fn test_handle_incoming_request_validates_target_peer_id() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "Local".to_string(),
            "device-1".to_string(),
            "peer-local".to_string(),
            vec![1; 32],
        );

        let request = PairingRequest {
            session_id: "session-1".to_string(),
            device_name: "Peer".to_string(),
            device_id: "device-2".to_string(),
            peer_id: "wrong-peer-id".to_string(),
            identity_pubkey: vec![2; 32],
            nonce: vec![3; 16],
        };

        let result = orchestrator
            .handle_incoming_request("peer-remote".to_string(), request)
            .await;

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Request target peer_id mismatch: expected peer-local, got wrong-peer-id"
        );
    }

    fn sample_keyslot_file() -> KeySlotFile {
        KeySlotFile {
            version: KeySlotVersion::V1,
            scope: KeyScope {
                profile_id: "profile-1".to_string(),
            },
            kdf: KdfParams {
                alg: KdfAlgorithm::Argon2id,
                params: KdfParamsV1 {
                    mem_kib: 1024,
                    iters: 2,
                    parallelism: 1,
                },
            },
            salt: vec![1, 2, 3],
            wrapped_master_key: EncryptedBlob {
                version: EncryptionFormatVersion::V1,
                aead: EncryptionAlgo::XChaCha20Poly1305,
                nonce: vec![9, 8, 7],
                ciphertext: vec![6, 5, 4],
                aad_fingerprint: None,
            },
            created_at: None,
            updated_at: None,
        }
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_keyslot_received_event() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");
        let offer = PairingKeyslotOffer {
            session_id: "session-1".to_string(),
            keyslot_file: Some(sample_keyslot_file()),
            challenge: Some(vec![9, 9, 9]),
        };

        orchestrator
            .handle_keyslot_offer("session-1", "peer-remote", offer)
            .await
            .expect("handle keyslot offer");

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");

        assert!(matches!(
            event,
            crate::usecases::pairing::PairingDomainEvent::KeyslotReceived { .. }
        ));
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_pairing_result_events() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, _action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        orchestrator
            .record_session_peer(
                "session-1",
                "peer-remote".to_string(),
                Some("Remote".to_string()),
            )
            .await;

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::EmitResult {
                    session_id: "session-1".to_string(),
                    success: true,
                    error: None,
                },
            )
            .await
            .expect("emit success result");

        let success_event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");
        assert!(matches!(
            success_event,
            crate::usecases::pairing::PairingDomainEvent::PairingSucceeded { .. }
        ));

        orchestrator
            .execute_action(
                "session-1",
                "peer-remote",
                PairingAction::EmitResult {
                    session_id: "session-1".to_string(),
                    success: false,
                    error: Some("failed".to_string()),
                },
            )
            .await
            .expect("emit failure result");

        let failed_event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");
        match failed_event {
            crate::usecases::pairing::PairingDomainEvent::PairingFailed { reason, .. } => {
                assert!(matches!(reason, FailureReason::Other(_)));
            }
            _ => panic!("expected PairingFailed event"),
        }
    }

    #[tokio::test]
    async fn pairing_orchestrator_emits_verification_required_event() {
        let config = PairingConfig::default();
        let device_repo = Arc::new(MockDeviceRepository);
        let (orchestrator, mut action_rx) = PairingOrchestrator::new(
            config,
            device_repo,
            "LocalDevice".to_string(),
            "device-123".to_string(),
            "peer-local".to_string(),
            vec![0u8; 32],
        );

        orchestrator
            .record_session_peer(
                "session-verify",
                "peer-remote".to_string(),
                Some("Remote".to_string()),
            )
            .await;

        let mut event_rx = crate::usecases::pairing::PairingEventPort::subscribe(&orchestrator)
            .await
            .expect("subscribe event port");

        orchestrator
            .execute_action(
                "session-verify",
                "peer-remote",
                PairingAction::ShowVerification {
                    session_id: "session-verify".to_string(),
                    short_code: "111-222".to_string(),
                    local_fingerprint: "LOCAL".to_string(),
                    peer_fingerprint: "PEER".to_string(),
                    peer_display_name: "Remote".to_string(),
                },
            )
            .await
            .expect("execute action");

        // consume action forwarded to UI channel
        timeout(Duration::from_secs(1), action_rx.recv())
            .await
            .expect("action timeout")
            .expect("action missing");

        let event = timeout(Duration::from_secs(1), event_rx.recv())
            .await
            .expect("event timeout")
            .expect("event missing");

        match event {
            crate::usecases::pairing::PairingDomainEvent::PairingVerificationRequired {
                session_id,
                peer_id,
                short_code,
                local_fingerprint,
                peer_fingerprint,
            } => {
                assert_eq!(session_id, "session-verify");
                assert_eq!(peer_id, "peer-remote");
                assert_eq!(short_code, "111-222");
                assert_eq!(local_fingerprint, "LOCAL");
                assert_eq!(peer_fingerprint, "PEER");
            }
            _ => panic!("expected PairingVerificationRequired event"),
        }
    }
}
