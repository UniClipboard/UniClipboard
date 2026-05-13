//! Application-layer errors for the Slice 1 facade.

use std::net::IpAddr;

use thiserror::Error;

/// Failure modes of A1 `InitializeSpaceUseCase`.
///
/// Kept narrower than the ports' native error types so callers can branch
/// on **what action to take next** (ask user again / surface a support
/// message / crash-logs) without having to understand cryptographic
/// details.
#[derive(Debug, Error)]
pub enum InitializeSpaceError {
    /// `passphrase` and `passphrase_confirm` differed. UI should keep the
    /// user on the current form.
    #[error("passphrase and confirmation do not match")]
    PassphraseMismatch,

    /// No device name available — neither in the command nor in
    /// `Settings.general.device_name`.
    #[error("device name is required but not provided")]
    DeviceNameRequired,

    /// The local space has already been initialised. User should unlock
    /// (A2) instead, or run a factory reset first.
    #[error("space is already initialised")]
    AlreadyInitialized,

    /// Setup was already completed for this device. Distinct from
    /// [`AlreadyInitialized`](Self::AlreadyInitialized) at the port layer:
    /// this variant is raised up-front by the use case when
    /// `SetupStatus.has_completed == true`, so it fires even if the
    /// keyslot is somehow missing. Identity lifetime is a bootstrap-time
    /// concern (the iroh endpoint binds its Ed25519 secret before any A1
    /// can run), so the "fresh install" guard keys off setup status
    /// rather than identity existence.
    #[error("setup has already been completed on this device")]
    AlreadySetup,

    /// Failed to read or persist settings / membership / setup-status —
    /// message carries adapter-level context for logs.
    #[error("storage failure: {0}")]
    StorageFailed(String),

    /// Any other uncategorised failure (adapter internal / infra-layer
    /// bug). Treat as fatal for the current action.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of B1 `IssuePairingInvitationUseCase`.
///
/// Mirrors
/// [`uc_core::ports::pairing_invitation::InvitationError`] at the
/// application boundary, keeping the upstream-port variant names so UI
/// can branch on intent ("start network" vs. "retry later") without
/// having to import the infra-port enum.
#[derive(Debug, Error)]
pub enum IssuePairingInvitationError {
    /// Underlying network runtime has not been started. UI should surface
    /// "start network first" (A1/A2 completing auto-starts it, so this
    /// typically means startup failed earlier and the user needs to retry).
    #[error("network is not started")]
    NetworkNotStarted,

    /// Rendezvous service unreachable / transient failure. UI may offer a
    /// manual retry.
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,

    /// 调用方指定的本机地址当前不能用于配对邀请。
    #[error("requested address is not available: {0}")]
    AddressNotAvailable(IpAddr),

    /// Uncategorised adapter-side failure; message for logs only.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of B2 `RedeemPairingInvitationUseCase` (joiner side).
///
/// 应用层把三类来源的失败统一成"下一步动作"导向:
///
/// * 本机参数/状态问题（`DeviceNameRequired`）→ UI 让用户补齐再试。
/// * 网络/凭证问题（`InvitationNotFound/Expired` / `PassphraseMismatch` /
///   `SponsorUnreachable` 等）→ UI 展示具体原因，用户决定改口令 /
///   重新要邀请 / 等对方上线再试。
/// * sponsor 主动拒绝（`SponsorRejectedInvitation` / `SponsorDeclined` /
///   `SponsorTimedOut` / `SponsorInternal`）→ 不是本机错，信息告知并让
///   用户重新开始。
#[derive(Debug, Error)]
pub enum RedeemPairingInvitationError {
    /// Rendezvous 服务端没有这条邀请（typo / 从未 issue / 已经被消费）。
    #[error("invitation not found")]
    InvitationNotFound,

    /// 邀请已过 TTL — 让用户重新找 sponsor 要一份。
    #[error("invitation has expired")]
    InvitationExpired,

    /// Sponsor 在线广告过 address，但连接没打通（NAT / relay / 对方掉线）。
    #[error("sponsor is not reachable")]
    SponsorUnreachable,

    /// Rendezvous 服务不可达。
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,

    /// 口令错。覆盖两种来源:(a) 本机 `derive_master_key_for_proof` 解
    /// keyslot 失败；(b) sponsor 收到 proof 后 `verify_proof` 拒绝后发
    /// `Reject(PassphraseMismatch)`。两者语义相同 — UI 提示"再试一次
    /// 口令"。
    #[error("wrong passphrase")]
    PassphraseMismatch,

    /// Sponsor 发来的 keyslot 字节无法解析或版本不支持。属于数据/版本
    /// 故障，和 A2 `UnlockSpaceError::CorruptedKeyMaterial` 同义。
    #[error("space key material corrupted")]
    CorruptedKeyMaterial,

    /// 本机 `Settings.general.device_name` 为空且 command 里也没给 —
    /// UI 应该在进入 join flow 前先收集 device name（和 A1 一致）。
    #[error("device name is required but not provided")]
    DeviceNameRequired,

    /// sponsor 收到 `JoinerRequest` 后 code 未命中任何 pending 邀请，回
    /// `Reject(InvitationMismatch)`。多半 race：code 在 sponsor 这边已
    /// 过期或被别的 joiner 消费。
    #[error("sponsor did not recognise the invitation code")]
    SponsorRejectedInvitation,

    /// sponsor UI 明确拒绝本次配对（Slice 1 未暴露审批 UI，保留语义位）。
    #[error("sponsor declined the pairing request")]
    SponsorDeclined,

    /// sponsor 侧 TTL watchdog 先触发（P7g）— 对方还没看到本机的
    /// `ChallengeResponse`。UI 应提示"网络慢或 sponsor 没响应，重新试"。
    #[error("sponsor timed out the handshake")]
    SponsorTimedOut,

    /// sponsor 回 `Reject(Internal(..))` — 对方本地 persist / settings 出
    /// 问题。消息面向日志。
    #[error("sponsor internal error: {0}")]
    SponsorInternal(String),

    /// 本机等 sponsor 回消息时 TTL 耗尽（recv 超时）。
    #[error("pairing handshake timed out")]
    Timeout,

    /// 握手中途 transport 掉线（sponsor 关闭 stream / iroh connection
    /// 中断 / recv 收到 EOF）。
    #[error("connection lost mid-handshake")]
    ConnectionLost,

    /// 非预期消息、adapter 内部错、序列化等兜底。消息面向日志。
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of A2 `UnlockSpaceUseCase`.
#[derive(Debug, Error)]
pub enum UnlockSpaceError {
    /// Setup has not been completed — there is no space to unlock yet.
    #[error("setup has not been completed")]
    SetupNotCompleted,

    /// Space exists only logically (setup marked complete) but the
    /// underlying keyslot is missing / corrupted.
    #[error("space is not initialised")]
    SpaceNotInitialized,

    /// Passphrase did not unwrap the stored master key.
    #[error("wrong passphrase")]
    WrongPassphrase,

    /// Stored keyslot was corrupted or in an unsupported format.
    #[error("space key material corrupted")]
    CorruptedKeyMaterial,

    /// Uncategorised infra / adapter failure.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::cancel_invitation`]
/// (Slice4 P3 T3.2).
#[derive(Debug, Error)]
pub enum CancelInvitationError {
    /// No in-flight invitation to cancel — the holder is empty. Maps
    /// to HTTP 409 Conflict at the daemon boundary so the UI can
    /// distinguish "nothing to cancel" from a transport error.
    #[error("no in-flight invitation to cancel")]
    NotIssued,

    /// Uncategorised infra / adapter failure.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::reset`]
/// (Slice4 P3 T3.2).
#[derive(Debug, Error)]
pub enum ResetSpaceError {
    /// Failed to clear `SetupStatus` — the device may be in an
    /// inconsistent state. Caller should surface to the operator.
    #[error("failed to clear setup status: {0}")]
    StorageFailed(String),

    /// Uncategorised infra / adapter failure.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::query_setup_state`]
/// (Slice4 P3 T3.2).
#[derive(Debug, Error)]
pub enum QuerySetupStateError {
    /// Failed to read `SetupStatus` from persistent storage.
    #[error("failed to read setup status: {0}")]
    StorageFailed(String),

    /// Uncategorised infra / adapter failure.
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::switch_space`].
///
/// 已 setup 设备加入另一个空间的 4 阶段重加密流程的失败原因。语义粒度与
/// `RedeemPairingInvitationError` 对齐——大部分变体是 redeem 错误的
/// 1:1 映射，再加上 switch-space 特有的 pre-flight / migration 状态分支：
///
/// * `NotSetup` — 设备还没完成首次 setup，应该走 redeem 而不是 switch-space。
/// * `PendingMigration` — 之前有迁移没跑完；UI 应让用户选择"恢复"或"放弃"。
/// * `NotUnlocked` — session 还没解锁；调用 switch-space 前必须先 unlock。
/// * `InvalidCiphertext` — 备份记录解密失败（一般是 daemon 重启后 keyring
///   migration_key 被清掉导致）。
/// * `Storage` / `Internal` — 持久化 / adapter 兜底。
#[derive(Debug, Error)]
pub enum SwitchSpaceError {
    #[error("device has not completed first-time setup yet")]
    NotSetup,
    #[error("a previous switch-space migration is still in flight")]
    PendingMigration(uc_core::setup::MigrationPhase),
    #[error("space session is locked; unlock before switching spaces")]
    NotUnlocked,
    #[error("invitation not found")]
    InvitationNotFound,
    #[error("invitation has expired")]
    InvitationExpired,
    #[error("sponsor is not reachable")]
    SponsorUnreachable,
    #[error("sponsor declined the pairing request")]
    SponsorDeclined,
    #[error("sponsor did not recognise the invitation code")]
    SponsorRejectedInvitation,
    #[error("sponsor handshake timed out")]
    Timeout,
    #[error("connection lost mid-handshake")]
    ConnectionLost,
    #[error("wrong passphrase")]
    PassphraseMismatch,
    #[error("space key material corrupted")]
    CorruptedKeyMaterial,
    #[error("device name is required but not provided")]
    DeviceNameRequired,
    #[error("pairing invitation service unavailable")]
    ServiceUnavailable,
    #[error("backup record decryption failed (corrupted ciphertext)")]
    InvalidCiphertext,
    #[error("storage failure: {0}")]
    Storage(String),
    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::query_migration_progress`].
///
/// 进度查询是只读操作，唯一失败来源是底层持久化故障——粒度与
/// `QuerySetupStateError` 对齐。
#[derive(Debug, Error)]
pub enum QueryMigrationProgressError {
    #[error("failed to read migration state: {0}")]
    StorageFailed(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Failure modes of [`crate::facade::space_setup::SpaceSetupFacade::try_resume_session`].
///
/// Kept narrow on purpose: "nothing to resume" (setup never completed
/// or keyslot absent) is a **normal** signal returned as `Ok(false)`,
/// not an error. Only genuine problems — corrupt key material, a
/// missing keyring entry that blocks silent resume despite the keyslot
/// being on disk, or an unexpected adapter fault — surface as errors.
#[derive(Debug, Error)]
pub enum TryResumeSessionError {
    /// The keyslot exists on disk but the keyring entry needed to
    /// unwrap it is missing or rejected (typically: user wiped the
    /// system keychain item, or permission was denied). Caller should
    /// fall back to a passphrase-based `unlock` once that CLI surface
    /// exists; until then the operator must re-init the profile.
    #[error("cached master key is not available from the keyring")]
    KeyringMiss,

    /// Stored keyslot was corrupted or in an unsupported format.
    #[error("space key material corrupted")]
    CorruptedKeyMaterial,

    /// Uncategorised infra / adapter failure.
    #[error("internal error: {0}")]
    Internal(String),
}
