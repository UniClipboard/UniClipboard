//! Command / result payloads for the Slice 1 application facade.
//!
//! Each pair models one external-facing application action; keep them free
//! of cross-cutting domain types and do not add query shape here.

use chrono::{DateTime, Utc};

use uc_core::crypto::domain::Passphrase;
use uc_core::ids::{DeviceId, SpaceId};
use uc_core::pairing::InvitationCode;
pub use uc_core::ports::pairing_invitation::PairingInvitationAddressCandidate;
use uc_core::security::IdentityFingerprint;

// ---------------------------------------------------------------------------
// A1 · InitializeSpace
// ---------------------------------------------------------------------------

/// Public application input for initializing a space.
#[derive(Debug)]
pub struct InitializeSpaceInput {
    pub passphrase: String,
    pub passphrase_confirm: String,
    pub device_name: Option<String>,
}

/// Internal command for [`crate::usecases::setup::initialize_space::InitializeSpaceUseCase`].
#[derive(Debug)]
pub(crate) struct InitializeSpaceCommand {
    /// User-entered passphrase protecting the new space.
    pub passphrase: Passphrase,
    /// Confirmation copy — must equal [`passphrase`](Self::passphrase).
    pub passphrase_confirm: Passphrase,
    /// Display name for this device as seen by future members.
    ///
    /// * `Some(name)` — persist to `Settings.general.device_name` and use
    ///   for the owner `SpaceMember` record.
    /// * `None` — fall back to the currently-persisted `device_name`;
    ///   caller-level UI must have collected it beforehand.
    pub device_name: Option<String>,
}

impl From<InitializeSpaceInput> for InitializeSpaceCommand {
    fn from(input: InitializeSpaceInput) -> Self {
        Self {
            passphrase: Passphrase::new(input.passphrase),
            passphrase_confirm: Passphrase::new(input.passphrase_confirm),
            device_name: input.device_name,
        }
    }
}

/// Output of a successful A1 initialise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitializeSpaceResult {
    pub space_id: SpaceId,
    pub self_device_id: DeviceId,
    pub fingerprint: IdentityFingerprint,
}

// ---------------------------------------------------------------------------
// A2 · UnlockSpace
// ---------------------------------------------------------------------------

/// Public application input for unlocking a space.
#[derive(Debug)]
pub struct UnlockSpaceInput {
    pub passphrase: String,
}

/// Internal command for [`crate::usecases::setup::unlock_space::UnlockSpaceUseCase`].
#[derive(Debug)]
pub(crate) struct UnlockSpaceCommand {
    pub passphrase: Passphrase,
}

impl From<UnlockSpaceInput> for UnlockSpaceCommand {
    fn from(input: UnlockSpaceInput) -> Self {
        Self {
            passphrase: Passphrase::new(input.passphrase),
        }
    }
}

/// Output of a successful A2 unlock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlockSpaceResult {
    pub space_id: SpaceId,
}

// ---------------------------------------------------------------------------
// B1 · IssuePairingInvitation
// ---------------------------------------------------------------------------

/// Output of a successful B1 invitation issuance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssuePairingInvitationResult {
    /// Short human-typable code the sponsor shows to the joiner.
    pub code: InvitationCode,
    /// Server-authoritative expiry; UI should display a countdown from
    /// this value rather than computing its own.
    pub expires_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// B2 · RedeemPairingInvitation  (joiner side)
// ---------------------------------------------------------------------------

/// Public application input for redeeming a pairing invitation.
#[derive(Debug)]
pub struct RedeemPairingInvitationInput {
    pub code: String,
    pub passphrase: String,
}

/// Internal command for [`crate::usecases::pairing::redeem_invitation::RedeemPairingInvitationUseCase`].
///
/// Joiner-side UX gathers both fields up front: the user types the
/// invitation code the sponsor shared and the space passphrase the sponsor
/// chose during A1. Slice 1 does not support a two-step flow where the
/// passphrase is entered after receiving the keyslot offer.
#[derive(Debug)]
pub(crate) struct RedeemPairingInvitationCommand {
    /// Invitation code the user typed (or scanned from the sponsor's UI).
    pub code: InvitationCode,
    /// Same passphrase the sponsor used in A1 `InitializeSpace`.
    pub passphrase: Passphrase,
}

impl From<RedeemPairingInvitationInput> for RedeemPairingInvitationCommand {
    fn from(input: RedeemPairingInvitationInput) -> Self {
        Self {
            code: InvitationCode::new(input.code),
            passphrase: Passphrase::new(input.passphrase),
        }
    }
}

// ---------------------------------------------------------------------------
// Setup state query (Slice4 P3 T3.2)
// ---------------------------------------------------------------------------

/// Read-only view of setup state surfaced by
/// [`crate::facade::space_setup::SpaceSetupFacade::query_setup_state`].
///
/// Replaces the legacy stateful FSM snapshot exposed via
/// `SetupFacade::get_state`. The new shape carries only what the
/// stateless v2 UI flow needs: whether onboarding is done, what
/// invitation (if any) is currently parked on the sponsor side, and
/// the local device name to prefill confirmation copy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupStateView {
    /// `true` when this device has completed A1 (`InitializeSpace`) or
    /// B2 (`RedeemPairingInvitation`); `false` on a fresh install.
    pub has_completed: bool,
    /// `Some(_)` when the sponsor has a Pending invitation parked in
    /// the in-memory holder; `None` when there is no in-flight code.
    /// Multi-pending policy is "earliest-expiring wins".
    pub current_invitation: Option<CurrentInvitation>,
    /// Display name persisted in `Settings.general.device_name`, or
    /// `None` when unset on a fresh install.
    pub device_name: Option<String>,
}

/// Companion to [`SetupStateView::current_invitation`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentInvitation {
    pub code: InvitationCode,
    pub expires_at: DateTime<Utc>,
}

/// Output of a successful B2 redemption.
///
/// Returned fields let the UI show a "you are connected to X" confirmation
/// without having to re-read the freshly-persisted `SpaceMember` /
/// `TrustedPeer` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedeemPairingInvitationResult {
    /// Sponsor device now persisted locally as both `SpaceMember` and
    /// `TrustedPeer`.
    pub sponsor_device_id: DeviceId,
    /// Sponsor's stable identity fingerprint (F-036 concept 2).
    pub sponsor_identity_fingerprint: IdentityFingerprint,
    /// Sponsor's space id, adopted as the joiner's local space id.
    pub space_id: SpaceId,
    /// This device's own id, as persisted on the sponsor side through the
    /// in-flight `JoinerRequest` — surfaced here so the UI does not need
    /// to query `DeviceIdentityPort` separately for the confirmation
    /// screen.
    pub self_device_id: DeviceId,
    /// This device's stable identity fingerprint.
    pub self_identity_fingerprint: IdentityFingerprint,
}

// ---------------------------------------------------------------------------
// SwitchSpace (joiner that has already completed setup)
// ---------------------------------------------------------------------------

/// Public application input for switching this device to another sponsor's
/// space while preserving local clipboard history (4-phase re-encryption
/// migration). Mirrors [`RedeemPairingInvitationInput`] shape on purpose:
/// the UI flow only differs in pre-conditions (already-setup device).
#[derive(Debug)]
pub struct SwitchSpaceInput {
    pub code: String,
    pub new_passphrase: String,
}

/// Internal command for [`crate::usecases::setup::switch_space::SwitchSpaceUseCase`].
#[derive(Debug)]
pub(crate) struct SwitchSpaceCommand {
    pub code: InvitationCode,
    pub new_passphrase: Passphrase,
}

impl From<SwitchSpaceInput> for SwitchSpaceCommand {
    fn from(input: SwitchSpaceInput) -> Self {
        Self {
            code: InvitationCode::new(input.code),
            new_passphrase: Passphrase::new(input.new_passphrase),
        }
    }
}

/// Output of a successful switch-space migration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchSpaceResult {
    pub sponsor_device_id: DeviceId,
    pub sponsor_identity_fingerprint: IdentityFingerprint,
    pub space_id: SpaceId,
    pub self_device_id: DeviceId,
    pub self_identity_fingerprint: IdentityFingerprint,
    /// 实际被重加密回写的 representation 行数，用于 UI 显示"迁移了 N 条历史"。
    pub migrated_records: u64,
}

// ---------------------------------------------------------------------------
// Switch-space progress query (polling endpoint)
// ---------------------------------------------------------------------------

/// 4 阶段迁移当前所处的粗粒度状态。比 `MigrationPhase` 简洁——不暴露
/// `run_id` / `target_space_id` 这些 UI 不该感知的内部细节。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhaseKind {
    Prepared,
    HandshakeDone,
    Swapped,
}

impl From<&uc_core::setup::MigrationPhase> for MigrationPhaseKind {
    fn from(phase: &uc_core::setup::MigrationPhase) -> Self {
        match phase {
            uc_core::setup::MigrationPhase::Prepared { .. } => Self::Prepared,
            uc_core::setup::MigrationPhase::HandshakeDone { .. } => Self::HandshakeDone,
            uc_core::setup::MigrationPhase::Swapped { .. } => Self::Swapped,
        }
    }
}

/// Read-only snapshot of switch-space progress for UI polling.
///
/// 粗粒度——不写"x of N records"这种 phase 3 实时进度。phase 3 在
/// `SwitchSpaceUseCase::execute` / `resume_pending` 内部流式跑完，期间
/// 不暴露增量计数器；UI 看到 `phase = HandshakeDone` 就知道"还在跑
/// phase 3"，看到 `phase = Swapped` 就知道"快做完了"。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationProgress {
    /// `None` 表示当前没有进行中的迁移（空闲态 / 已完成）。
    pub phase: Option<MigrationPhaseKind>,
    /// `clipboard_migration_backup` 表里当前存的条目数；phase 1 完成后
    /// 等同于"待回写到主表的总条数"，phase 4 cleanup 后回到 0。
    pub backup_record_count: u64,
}
