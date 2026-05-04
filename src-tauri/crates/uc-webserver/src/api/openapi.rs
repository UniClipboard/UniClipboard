//! OpenAPI security definitions for the daemon HTTP API.
//!
//! L2 endpoints require a valid session JWT. The client obtains a session token
//! by calling `POST /auth/dev-token` (dev only) or through the pairing flow.
//! The token is passed via the `Authorization` header as `Session <token>`.

use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};
use utoipa::{Modify, OpenApi};

use crate::api::dto::clipboard::{
    ClearHistoryResponse, ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto,
    EntryProjectionResponseDto, EntryResourceDto, GetClipboardStatsResponse,
    GetEntryDetailResponse, GetEntryResourceResponse, ListEntriesResponse, ToggleFavoriteRequest,
    ToggleFavoriteResponse, ToggleFavoriteResultDto,
};
use crate::api::dto::device::{GetLocalDeviceInfoResponse, LocalDeviceInfoDto};
use crate::api::dto::encryption::{EncryptionStateResponse, KeychainAccessResponse};
use crate::api::dto::error::ApiErrorResponse;
use crate::api::dto::member::{
    GetMemberSyncPreferencesResponse, MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto,
    UpdateMemberSyncPreferencesResponse,
};
use crate::api::dto::pairing::UnpairDeviceRequest;
use crate::api::dto::search::{
    SearchQueryResponse, SearchRebuildAcceptedData, SearchRebuildAcceptedResponse, SearchResultDto,
    SearchStatusData, SearchStatusResponse,
};
use crate::api::dto::settings::{
    ContentTypesDto, FileSyncSettingsDto, GeneralSettingsDto, GetSettingsResponse,
    NetworkSettingsDto, PairingSettingsDto, RetentionPolicyDto, RetentionRuleDto,
    RuleEvaluationDto, SecuritySettingsDto, SettingsDto, ShortcutKeyDto, SyncFrequencyDto,
    SyncSettingsDto, ThemeDto, UpdateChannelDto, UpdateSettingsResponse,
};
use uc_daemon_contract::api::dto::upgrade::{
    AckUpgradePayload, AckUpgradeResponse, GetUpgradeStatusResponse, UpgradeStatusDto,
};
use uc_daemon_contract::api::dto::v2::setup::{
    CurrentInvitation as V2CurrentInvitation, InitializeSpaceRequest as V2InitializeSpaceRequest,
    InitializeSpaceResponse as V2InitializeSpaceResponse,
    IssueInvitationResponse as V2IssueInvitationResponse, RedeemRequest as V2RedeemRequest,
    RedeemResponse as V2RedeemResponse, SetupStateResponse as V2SetupStateResponse,
};

/// Adds a `session` Bearer security scheme to the OpenAPI document and applies it
/// to all L2+ paths.
///
/// The scheme uses the `Authorization` header with a `Session <token>` value.
/// This matches the middleware in `security/middleware.rs` which validates JWT
/// session tokens after stripping the `Session ` prefix.
struct SecurityAddon;

impl Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        let components = openapi.components.get_or_insert_with(Default::default);

        components.add_security_scheme(
            "session",
            SecurityScheme::ApiKey(ApiKey::Header(ApiKeyValue::new("Authorization"))),
        );

        // Batch-apply the security scheme to ALL paths.
        // L1 paths (health, dev-token) are in a separate OpenApi derivation in dev.rs.
        for (_, path_item) in openapi.paths.paths.iter_mut() {
            for op in path_item.operations.values_mut() {
                op.security.get_or_insert_with(Default::default).push(
                    utoipa::openapi::security::SecurityRequirement::new(
                        "session",
                        std::iter::empty::<String>(),
                    ),
                );
            }
        }
    }
}

#[derive(OpenApi)]
#[openapi(
    modifiers(&SecurityAddon),
    paths(
        crate::api::clipboard::list_entries,
        crate::api::clipboard::get_entry,
        crate::api::clipboard::delete_entry,
        crate::api::clipboard::toggle_favorite,
        crate::api::clipboard::get_stats,
        crate::api::clipboard::get_entry_resource,
        crate::api::clipboard::clear_history,
        crate::api::settings::get_settings_handler,
        crate::api::settings::update_settings_handler,
        crate::api::encryption::get_encryption_state_handler,
        crate::api::encryption::unlock_handler,
        crate::api::encryption::lock_handler,
        crate::api::encryption::verify_keychain_access_handler,
        crate::api::device::get_local_device_info_handler,
        crate::api::member::get_member_sync_preferences_handler,
        crate::api::member::update_member_sync_preferences_handler,
        crate::api::v2::setup::initialize,
        crate::api::v2::setup::issue_invitation,
        crate::api::v2::setup::redeem,
        crate::api::v2::setup::cancel,
        crate::api::v2::setup::reset,
        crate::api::v2::setup::get_state,
        crate::api::pairing::handle_unpair_device,
        crate::api::upgrade::get_upgrade_status_handler,
        crate::api::upgrade::ack_upgrade_handler,
    ),
    components(
        schemas(
            // Clipboard
            ListEntriesResponse,
            EntryProjectionResponseDto,
            GetEntryDetailResponse,
            EntryDetailDto,
            GetEntryResourceResponse,
            EntryResourceDto,
            GetClipboardStatsResponse,
            ClipboardStatsDto,
            ClearHistoryResponse,
            ClearHistoryResultDto,
            ToggleFavoriteRequest,
            ToggleFavoriteResponse,
            ToggleFavoriteResultDto,
            // Common
            ContentTypesDto,
            ApiErrorResponse,
            GetLocalDeviceInfoResponse,
            LocalDeviceInfoDto,
            GetSettingsResponse,
            UpdateSettingsResponse,
            SettingsDto,
            GeneralSettingsDto,
            SyncSettingsDto,
            SyncFrequencyDto,
            RetentionPolicyDto,
            RetentionRuleDto,
            RuleEvaluationDto,
            SecuritySettingsDto,
            PairingSettingsDto,
            FileSyncSettingsDto,
            NetworkSettingsDto,
            ShortcutKeyDto,
            ThemeDto,
            UpdateChannelDto,
            EncryptionStateResponse,
            KeychainAccessResponse,
            UnpairDeviceRequest,
            // Search
            SearchQueryResponse,
            SearchResultDto,
            SearchStatusResponse,
            SearchStatusData,
            SearchRebuildAcceptedResponse,
            SearchRebuildAcceptedData,
            // Member sync preferences (phase 4b PR-2)
            MemberSyncPreferencesDto,
            MemberSyncPreferencesPatchDto,
            GetMemberSyncPreferencesResponse,
            UpdateMemberSyncPreferencesResponse,
            // setup-v2 (Slice4 P3 T3.2)
            V2InitializeSpaceRequest,
            V2InitializeSpaceResponse,
            V2IssueInvitationResponse,
            V2RedeemRequest,
            V2RedeemResponse,
            V2SetupStateResponse,
            V2CurrentInvitation,
            // Upgrade detection (P1 thin module)
            GetUpgradeStatusResponse,
            UpgradeStatusDto,
            AckUpgradeResponse,
            AckUpgradePayload,
        )
    ),
    tags(
        (name = "clipboard", description = "Clipboard entry CRUD and statistics"),
        (name = "device", description = "Local device identity"),
        (name = "member", description = "Space member sync preferences (phase 4b)"),
        (name = "settings", description = "Settings management APIs"),
        (name = "encryption", description = "Encryption state and session management"),
        (name = "setup-v2", description = "Stateless v2 setup pairing endpoints (Slice4 P3 T3.2)"),
        (name = "pairing", description = "Pairing lifecycle management"),
        (name = "search", description = "Local encrypted search endpoints"),
        (name = "upgrade", description = "Application upgrade detection (P1 thin module)"),
    )
)]
pub struct ApiDoc;
