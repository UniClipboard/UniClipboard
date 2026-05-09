//! Port bundle consumed by [`super::SpaceSetupFacade::new`].
//!
//! Kept as a `struct` with `pub` fields so callers build it with a plain
//! literal (`SpaceSetupDeps { space_access, local_identity, … }`) and
//! adding a new dependency in a future slice is one line here plus an
//! explicit field in the caller — no cascading constructor churn.

use std::sync::Arc;

use uc_core::membership::MemberRepositoryPort;
use uc_core::ports::clipboard::BlobMigrationRepoPort;
use uc_core::ports::pairing::{PairingEventPort, PairingSessionPort};
use uc_core::ports::pairing_invitation::PairingInvitationPort;
use uc_core::ports::security::{BlobCipherPort, KeyMigrationPort};
use uc_core::ports::setup::MigrationStatePort;
use uc_core::ports::space::{ProofPort, SpaceAccessPort};
use uc_core::ports::{
    ClockPort, DeviceIdentityPort, LocalIdentityPort, PeerAddressRepositoryPort, PresencePort,
    SettingsPort, SetupStatusPort,
};
use uc_core::trusted_peer::TrustedPeerRepositoryPort;
use uc_observability::analytics::AnalyticsPort;

/// Dependencies for [`super::SpaceSetupFacade`].
///
/// `SpaceAccessPort` / `SetupStatusPort` are shared between A1 and A2
/// because the underlying adapter keeps the active space / setup status
/// as process-wide singletons; the facade clones these `Arc`s when
/// constructing each use case.
pub struct SpaceSetupDeps {
    pub space_access: Arc<dyn SpaceAccessPort>,
    pub local_identity: Arc<dyn LocalIdentityPort>,
    pub device_identity: Arc<dyn DeviceIdentityPort>,
    pub member_repo: Arc<dyn MemberRepositoryPort>,
    pub setup_status: Arc<dyn SetupStatusPort>,
    pub settings: Arc<dyn SettingsPort>,
    pub clock: Arc<dyn ClockPort>,
    /// Sponsor-side rendezvous client for issuing invitation codes (B1)
    /// and notifying the rendezvous of successful consumes (P7e inbound
    /// path).
    ///
    /// The accompanying in-memory holder for parked invitations is
    /// constructed **inside** [`super::SpaceSetupFacade::new`] and kept
    /// `pub(crate)` so application-internal implementation details
    /// (`uc-application/AGENTS.md` §11.4) stay off the bootstrap surface.
    pub pairing_invitation: Arc<dyn PairingInvitationPort>,
    /// Session-level transport used by the sponsor-side inbound orchestrator
    /// to send `PairingReject` and close sessions that fail code matching.
    /// Joiner-side uses the same port to dial; Slice 1 wires a single
    /// adapter (`IrohPairingSessionAdapter`) for both roles.
    pub pairing_session: Arc<dyn PairingSessionPort>,
    /// Sponsor-side subscription to inbound pairing events. Drives the
    /// [`crate::pairing_inbound`] orchestrator; the facade spawns the event
    /// loop during construction and stops it on shutdown.
    pub pairing_events: Arc<dyn PairingEventPort>,
    /// HMAC proof verifier for the joiner's `ChallengeResponse` (P7f).
    /// Shared between the inbound handshake path and any future
    /// joiner-side flow that needs proof build/verify symmetry.
    pub proof_port: Arc<dyn ProofPort>,
    /// Persists a joiner as a `TrustedPeer` alongside the `SpaceMember`
    /// row when the P7f handshake succeeds (`PersistSponsorAccess`).
    pub trusted_peer_repo: Arc<dyn TrustedPeerRepositoryPort>,
    /// Slice 2 Phase 1 · T5：配对成功后把对端传输地址 blob 写入仓库。
    /// sponsor 端由 [`crate::pairing_inbound::PairingInboundOrchestrator`]
    /// 在 `finalise_verified` 里 best-effort 调用，joiner 端由
    /// [`crate::usecases::pairing::redeem_invitation::RedeemPairingInvitationUseCase`]
    /// 在 `persist` 收尾处调用。写失败不 fail 配对，presence 下轮兜底。
    pub peer_addr_repo: Arc<dyn PeerAddressRepositoryPort>,
    /// Slice 2 Phase 1 · T8：F1 hook 预连所有 paired peer(A1/A2/B2 成功后
    /// 自动触发),让 UI 查 roster 时 presence 状态立刻准。Facade 内部
    /// 会用它 + `peer_addr_repo` + `device_identity` 构造
    /// [`crate::usecases::presence::ensure_reachable_all::EnsureReachableAllUseCase`]
    /// (usecase 是 `pub(crate)`,bootstrap 不拿它直接 construct,
    /// 对齐 `uc-application/AGENTS.md` §11.4)。
    pub presence: Arc<dyn PresencePort>,
    /// Switch-space 重加密迁移：跨重启的阶段持久化点。`None` 表示无在飞迁移；
    /// `Prepared` / `HandshakeDone` / `Swapped` 各有续跑语义，由
    /// `SpaceSetupFacade::try_resume_session` 内部 `resume_pending` 自动处理。
    pub migration_state: Arc<dyn MigrationStatePort>,
    /// Switch-space 一次性 migration_key 的 keyring 管理。生命周期：phase 1
    /// `prepare_migration_key` 时生成，phase 4 `discard_migration_key` 时销毁。
    pub key_migration: Arc<dyn KeyMigrationPort>,
    /// Switch-space `clipboard_migration_backup` 表 + 主表 inline_data 批量
    /// 读写。常态业务代码不应触碰。
    pub blob_migration_repo: Arc<dyn BlobMigrationRepoPort>,
    /// 业务剪贴板加解密 port（既有 `EncryptingClipboardEventWriter` /
    /// `DecryptingClipboardRepresentationRepository` 已经持有同款 Arc）。
    /// switch-space phase 1 用它解旧密文 / phase 3 用它写新密文，所以
    /// facade 必须能拿到这一份。
    pub blob_cipher: Arc<dyn BlobCipherPort>,
    /// 产品 telemetry sink（横切关注点）。Slice 8b 起，joiner 端
    /// [`crate::usecases::pairing::redeem_invitation::RedeemPairingInvitationUseCase`]
    /// 在 execute 入口/收尾 fire `pairing_started` / `pairing_succeeded`
    /// / `pairing_failed`。Sink 装一次永不替换，gate 由 `GatedAnalyticsSink`
    /// wrapper 在 capture 入口守卫，业务侧无感。
    pub analytics: Arc<dyn AnalyticsPort>,
}
