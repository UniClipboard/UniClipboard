//! Canonical success envelope for the daemon HTTP API (ADR-008).
//!
//! A single generic `ApiEnvelope<T> { data: T, ts: i64 }` is the wire shape for
//! every enveloped endpoint. utoipa v4 cannot register a bare generic as a named
//! OpenAPI component, so each concrete `ApiEnvelope<Concrete>` is surfaced via a
//! derive-level `#[aliases(...)]` entry below. "Pure generic" (per §0.1) means
//! there are NO bespoke `{data,ts}` wrapper structs — NOT that there are no
//! aliases. The alias registry is the single source of truth for which payloads
//! get a reusable named schema.

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

// Payload DTOs that get wrapped (imported from their own modules):
use crate::api::dto::auth::SessionTokenResponse;
use crate::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto, ToggleFavoriteResultDto,
};
use crate::api::dto::clipboard_command::{
    CancelTransferResponse, DispatchOutcomeResponse, ResendResponse, RestoreEntryResponse,
};
use crate::api::dto::clipboard_delivery::EntryDeliveryViewDto;
use crate::api::dto::device::LocalDeviceInfoDto;
use crate::api::dto::encryption::{
    EncryptionActionResponse, EncryptionStateResponse, KeychainAccessResponse, UnlockSpaceResponse,
};
use crate::api::dto::member::{MemberSyncPreferencesDto, MemberSyncResultDto};
use crate::api::dto::mobile_sync::{
    LanInterfaceViewDto, MobileDeviceViewDto, MobileSyncActionResultDto, MobileSyncSettingsViewDto,
    RegisterMobileDeviceResultDto, RotateMobilePasswordResultDto,
    UpdateMobileSyncSettingsResultDto,
};
use crate::api::dto::search::{SearchQueryResultDto, SearchRebuildAcceptedData, SearchStatusData};
use crate::api::dto::settings::{SettingsDto, SettingsUpdateResultDto};
use crate::api::dto::storage::{ClearCacheResponse, StorageStatsDto};
use crate::api::dto::upgrade::{AckUpgradePayload, UpgradeStatusDto};
use crate::api::dto::v2::setup::{
    InitializeSpaceResponse, IssueInvitationResponse, MigrationProgressResponse, RedeemResponse,
    SetupStateResponse, SwitchSpaceResponse,
};
use crate::api::types::{
    HealthResponse, LifecycleStatusResponse, PeerSnapshotDto, PresenceRefreshResponse,
    SpaceMemberDto, StatusResponse,
};

/// Canonical success envelope: `{ "data": T, "ts": <unix millis i64> }`.
///
/// `ts` is `chrono::Utc::now().timestamp_millis()`, set in the webserver handler
/// via [`ApiEnvelope::now`] (the contract carries only the type + the clock
/// helper, not a hard dependency on when the handler reads the clock).
/// `rename_all = "camelCase"` is a no-op for the single-word fields here but is
/// declared for forward-compat.
///
/// IMPORTANT (utoipa v4): every concrete `ApiEnvelope<X>` that needs a named
/// OpenAPI component is declared in the `#[aliases(...)]` block below. Add a new
/// alias line whenever a new payload type needs enveloping. NEVER register the
/// bare `ApiEnvelope` in `components(schemas(...))` — utoipa errors on a bare
/// generic, and an un-aliased generic inlines an anonymous schema.
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "camelCase")]
#[aliases(
    // ── clipboard history ──────────────────────────────────────────
    ListEntriesEnvelope = ApiEnvelope<Vec<EntryProjectionResponseDto>>,
    EntryDetailEnvelope = ApiEnvelope<EntryDetailDto>,
    EntryResourceEnvelope = ApiEnvelope<EntryResourceDto>,
    ClipboardStatsEnvelope = ApiEnvelope<ClipboardStatsDto>,
    ClearHistoryEnvelope = ApiEnvelope<ClearHistoryResultDto>,
    ToggleFavoriteEnvelope = ApiEnvelope<ToggleFavoriteResultDto>,
    // ── clipboard delivery (newly enveloped, §H) ───────────────────
    DispatchOutcomeEnvelope = ApiEnvelope<DispatchOutcomeResponse>,
    ResendEnvelope = ApiEnvelope<ResendResponse>,
    CancelTransferEnvelope = ApiEnvelope<CancelTransferResponse>,
    RestoreEntryEnvelope = ApiEnvelope<RestoreEntryResponse>,
    EntryDeliveryViewEnvelope = ApiEnvelope<EntryDeliveryViewDto>,
    // ── settings (GET + PUT both enveloped per §0.1) ───────────────
    SettingsEnvelope = ApiEnvelope<SettingsDto>,
    SettingsUpdateResultEnvelope = ApiEnvelope<SettingsUpdateResultDto>,
    // ── device / member ────────────────────────────────────────────
    LocalDeviceInfoEnvelope = ApiEnvelope<LocalDeviceInfoDto>,
    MemberSyncPreferencesEnvelope = ApiEnvelope<MemberSyncPreferencesDto>,
    MemberSyncResultEnvelope = ApiEnvelope<MemberSyncResultDto>,
    // ── mobile sync ────────────────────────────────────────────────
    RegisterMobileDeviceEnvelope = ApiEnvelope<RegisterMobileDeviceResultDto>,
    RotateMobilePasswordEnvelope = ApiEnvelope<RotateMobilePasswordResultDto>,
    MobileSyncActionEnvelope = ApiEnvelope<MobileSyncActionResultDto>,
    MobileDeviceListEnvelope = ApiEnvelope<Vec<MobileDeviceViewDto>>,
    MobileSyncSettingsEnvelope = ApiEnvelope<MobileSyncSettingsViewDto>,
    UpdateMobileSyncSettingsEnvelope = ApiEnvelope<UpdateMobileSyncSettingsResultDto>,
    LanInterfaceListEnvelope = ApiEnvelope<Vec<LanInterfaceViewDto>>,
    // ── encryption ─────────────────────────────────────────────────
    EncryptionStateEnvelope = ApiEnvelope<EncryptionStateResponse>,
    KeychainAccessEnvelope = ApiEnvelope<KeychainAccessResponse>,
    EncryptionActionEnvelope = ApiEnvelope<EncryptionActionResponse>,
    UnlockSpaceEnvelope = ApiEnvelope<UnlockSpaceResponse>,
    // ── upgrade ─────────────────────────────────────────────────────
    UpgradeStatusEnvelope = ApiEnvelope<UpgradeStatusDto>,
    AckUpgradeEnvelope = ApiEnvelope<AckUpgradePayload>,
    // ── search (status + rebuild + query all enveloped per §0.1) ───
    SearchStatusEnvelope = ApiEnvelope<SearchStatusData>,
    SearchRebuildEnvelope = ApiEnvelope<SearchRebuildAcceptedData>,
    SearchQueryEnvelope = ApiEnvelope<SearchQueryResultDto>,
    // ── storage ────────────────────────────────────────────────────
    StorageStatsEnvelope = ApiEnvelope<StorageStatsDto>,
    ClearCacheEnvelope = ApiEnvelope<ClearCacheResponse>,
    // ── system diagnostics & topology (newly enveloped, §H) ────────
    HealthEnvelope = ApiEnvelope<HealthResponse>,
    StatusEnvelope = ApiEnvelope<StatusResponse>,
    LifecycleStatusEnvelope = ApiEnvelope<LifecycleStatusResponse>,
    PeerSnapshotListEnvelope = ApiEnvelope<Vec<PeerSnapshotDto>>,
    SpaceMemberListEnvelope = ApiEnvelope<Vec<SpaceMemberDto>>,
    PresenceRefreshEnvelope = ApiEnvelope<PresenceRefreshResponse>,
    // ── auth/connect (newly enveloped, §H) ─────────────────────────
    SessionTokenEnvelope = ApiEnvelope<SessionTokenResponse>,
    // ── setup-v2 (all 6 bodies newly enveloped, §H) ────────────────
    SetupInitializeEnvelope = ApiEnvelope<InitializeSpaceResponse>,
    SetupIssueInvitationEnvelope = ApiEnvelope<IssueInvitationResponse>,
    SetupRedeemEnvelope = ApiEnvelope<RedeemResponse>,
    SetupStateEnvelope = ApiEnvelope<SetupStateResponse>,
    SetupSwitchSpaceEnvelope = ApiEnvelope<SwitchSpaceResponse>,
    SetupMigrationProgressEnvelope = ApiEnvelope<MigrationProgressResponse>,
)]
pub struct ApiEnvelope<T> {
    pub data: T,
    /// Server time when the response was built (unix epoch milliseconds).
    pub ts: i64,
}

impl<T> ApiEnvelope<T> {
    /// Wrap `data` and stamp `ts` with the current wall-clock time.
    pub fn now(data: T) -> Self {
        Self {
            data,
            ts: chrono::Utc::now().timestamp_millis(),
        }
    }

    /// Wrap `data` with a caller-supplied timestamp (unix epoch milliseconds).
    pub fn with_ts(data: T, ts: i64) -> Self {
        Self { data, ts }
    }
}
