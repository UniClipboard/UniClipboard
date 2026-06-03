//! OpenAPI document assembly for the daemon HTTP API (ADR-008 §C.5 / §D).
//!
//! The `#[derive(OpenApi)] ApiDoc` stays in the webserver because its
//! `paths(...)` list references handler fns that live here. All cross-cutting
//! metadata (info/servers/tags + the dual `session_query` / `session_header`
//! security schemes + the `PUBLIC_PATHS` allowlist) is owned by the contract's
//! `openapi_meta` module and applied via `modifiers(&ContractMeta)`.
//!
//! Response bodies are the `#[aliases(...)]`-registered `ApiEnvelope<T>` schemas
//! (declared in `uc_daemon_contract::api::dto::envelope`), errors are the shared
//! `ApiErrorResponse`. There are no bespoke `{data,ts}` wrapper structs anymore
//! (per §0.1 — they were deleted by the per-domain P2 agents).

use utoipa::{Modify, OpenApi};

// ── Payload + request DTOs referenced by the enveloped aliases ──────────────
// (utoipa requires the inner payload schemas to be registered alongside the
// alias so each `$ref` resolves.)
use crate::api::dto::clipboard::{
    ClearHistoryResultDto, ClipboardStatsDto, EntryDetailDto, EntryProjectionResponseDto,
    EntryResourceDto, ToggleFavoriteRequest, ToggleFavoriteResultDto,
};
use crate::api::dto::device::LocalDeviceInfoDto;
use crate::api::dto::encryption::{
    EncryptionActionResponse, EncryptionStateResponse, KeychainAccessResponse, UnlockSpaceRequest,
    UnlockSpaceResponse,
};
use crate::api::dto::error::ApiErrorResponse;
use crate::api::dto::member::{
    MemberSyncPreferencesDto, MemberSyncPreferencesPatchDto, MemberSyncResultDto,
};
use crate::api::dto::pairing::UnpairDeviceRequest;
use crate::api::dto::search::{
    SearchQueryResultDto, SearchRebuildAcceptedData, SearchResultDto, SearchStatusData,
};
use crate::api::dto::settings::{
    ContentTypesDto, ContentTypesPatchDto, FileSyncSettingsDto, FileSyncSettingsPatchDto,
    GeneralSettingsDto, GeneralSettingsPatchDto, KeyboardShortcutsPatchDto, NetworkSettingsDto,
    NetworkSettingsPatchDto, PairingSettingsDto, PairingSettingsPatchDto, QuickPanelSettingsDto,
    QuickPanelSettingsPatchDto, RetentionPolicyDto, RetentionPolicyPatchDto, RetentionRuleDto,
    RuleEvaluationDto, SecuritySettingsDto, SecuritySettingsPatchDto, SettingsDto,
    SettingsPatchDto, SettingsUpdateResultDto, ShortcutKeyDto, SyncFrequencyDto, SyncSettingsDto,
    SyncSettingsPatchDto, ThemeDto, UpdateChannelDto,
};
use uc_daemon_contract::api::dto::auth::{ConnectRequest, SessionTokenResponse};
use uc_daemon_contract::api::dto::clipboard_command::{
    CancelTransferRequest, CancelTransferResponse, DispatchOutcomeResponse, DispatchTextRequest,
    PerTargetOutcomeDto, ResendRequest, ResendResponse, RestoreEntryResponse,
};
use uc_daemon_contract::api::dto::clipboard_delivery::{
    DeliveryFailureReasonDto, EntryDeliveryStatusDto, EntryDeliveryTargetDto, EntryDeliveryViewDto,
    EntrySourceDto,
};
use uc_daemon_contract::api::dto::envelope::{
    AckUpgradeEnvelope, CancelTransferEnvelope, ClearCacheEnvelope, ClearHistoryEnvelope,
    ClipboardStatsEnvelope, DispatchOutcomeEnvelope, EncryptionActionEnvelope,
    EncryptionStateEnvelope, EntryDeliveryViewEnvelope, EntryDetailEnvelope, EntryResourceEnvelope,
    KeychainAccessEnvelope, LifecycleStatusEnvelope, ListEntriesEnvelope, LocalDeviceInfoEnvelope,
    MemberSyncPreferencesEnvelope, MemberSyncResultEnvelope, PeerSnapshotListEnvelope,
    PresenceRefreshEnvelope, ResendEnvelope, RestoreEntryEnvelope, SearchQueryEnvelope,
    SearchRebuildEnvelope, SearchStatusEnvelope, SessionTokenEnvelope, SettingsEnvelope,
    SettingsUpdateResultEnvelope, SetupInitializeEnvelope, SetupIssueInvitationEnvelope,
    SetupMigrationProgressEnvelope, SetupRedeemEnvelope, SetupStateEnvelope,
    SetupSwitchSpaceEnvelope, SpaceMemberListEnvelope, StatusEnvelope, StorageStatsEnvelope,
    ToggleFavoriteEnvelope, UnlockSpaceEnvelope, UpgradeStatusEnvelope,
};
use uc_daemon_contract::api::dto::storage::{
    ClearCacheRequest, ClearCacheResponse, StorageStatsDto,
};
use uc_daemon_contract::api::dto::upgrade::{AckUpgradePayload, UpgradeStatusDto};
use uc_daemon_contract::api::dto::v2::setup::{
    CurrentInvitation, InitializeSpaceRequest, InitializeSpaceResponse, IssueInvitationResponse,
    MigrationPhaseDto, MigrationProgressResponse, RedeemRequest, RedeemResponse,
    SetupStateResponse, SwitchSpaceRequest, SwitchSpaceResponse,
};
use uc_daemon_contract::api::dto::ws::{WsErrorResponse, WsSubscribeRequest};
use uc_daemon_contract::api::types::DaemonWsEvent;
use uc_daemon_contract::api::types::{
    HealthResponse, LifecycleStatusResponse, PeerSnapshotDto, PresenceRefreshResponse,
    SpaceMemberDto, StatusResponse, WorkerStatusDto,
};

/// Applies the contract-owned cross-cutting OpenAPI metadata (info-adjacent
/// security schemes + per-operation session requirement, skipping the
/// `PUBLIC_PATHS` allowlist) to the derived `ApiDoc`.
///
/// Wrapping the contract helper in a webserver-local `Modify` keeps the
/// `paths(...)` handler list in the webserver while sourcing the dual-scheme
/// `SecurityAddon` from the single source of truth.
struct ContractMeta;

impl Modify for ContractMeta {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        uc_daemon_contract::api::openapi_meta::apply_metadata(openapi);
    }
}

#[derive(OpenApi)]
#[openapi(
    info(
        title = "UniClipboard Daemon API",
        version = "1.0.0",
        description = "Local daemon HTTP API for the UniClipboard GUI and native clients. \
            All enveloped responses use the canonical `{ data, ts }` shape; errors use \
            `{ code, message, details? }`. Binary and WebSocket endpoints are exempt from \
            the envelope. L2+ operations require a session token (query `?auth=` or the \
            `Authorization` header)."
    ),
    modifiers(&ContractMeta),
    paths(
        // ── clipboard (history + delivery) ─────────────────────────
        crate::api::clipboard::list_entries,
        crate::api::clipboard::get_entry,
        crate::api::clipboard::delete_entry,
        crate::api::clipboard::toggle_favorite,
        crate::api::clipboard::get_stats,
        crate::api::clipboard::get_entry_resource,
        crate::api::clipboard::get_entry_delivery_view_handler,
        crate::api::clipboard::clear_history,
        crate::api::clipboard::dispatch_text,
        crate::api::clipboard::resend_entry,
        crate::api::clipboard::cancel_transfer,
        crate::api::routes::restore_clipboard_entry_handler,
        // ── clipboard binary (octet-stream, doc-only) ──────────────
        crate::api::blob::get_blob,
        crate::api::blob::get_thumbnail,
        // ── search ─────────────────────────────────────────────────
        crate::api::search::search_query_handler,
        crate::api::search::search_status_handler,
        crate::api::search::search_rebuild_handler,
        // ── storage ────────────────────────────────────────────────
        crate::api::storage::get_storage_stats_handler,
        crate::api::storage::clear_cache_handler,
        // ── device ─────────────────────────────────────────────────
        crate::api::device::get_local_device_info_handler,
        // ── member ─────────────────────────────────────────────────
        crate::api::member::get_member_sync_preferences_handler,
        crate::api::member::update_member_sync_preferences_handler,
        // ── pairing ────────────────────────────────────────────────
        crate::api::pairing::handle_unpair_device,
        // ── encryption ─────────────────────────────────────────────
        crate::api::encryption::get_encryption_state_handler,
        crate::api::encryption::unlock_handler,
        crate::api::encryption::unlock_with_passphrase_handler,
        crate::api::encryption::lock_handler,
        crate::api::encryption::factory_reset_handler,
        crate::api::encryption::verify_keychain_access_handler,
        // ── settings ───────────────────────────────────────────────
        crate::api::settings::get_settings_handler,
        crate::api::settings::update_settings_handler,
        // ── lifecycle ──────────────────────────────────────────────
        crate::api::lifecycle::get_lifecycle_status_handler,
        crate::api::lifecycle::retry_lifecycle_handler,
        crate::api::lifecycle::lifecycle_ready_handler,
        // ── upgrade ────────────────────────────────────────────────
        crate::api::upgrade::get_upgrade_status_handler,
        crate::api::upgrade::ack_upgrade_handler,
        // ── system: diagnostics & topology ─────────────────────────
        crate::api::routes::health,
        crate::api::routes::status,
        crate::api::routes::peers,
        crate::api::routes::paired_devices,
        crate::api::routes::refresh_presence,
        crate::api::ws::router,
        // ── auth (L1/public bootstrap, system tag) ─────────────────
        crate::security::connect::connect_handler,
        // ── setup-v2 ───────────────────────────────────────────────
        crate::api::v2::setup::initialize,
        crate::api::v2::setup::issue_invitation,
        crate::api::v2::setup::redeem,
        crate::api::v2::setup::cancel,
        crate::api::v2::setup::reset,
        crate::api::v2::setup::get_state,
        crate::api::v2::setup::switch_space,
        crate::api::v2::setup::query_migration_progress,
    ),
    components(
        schemas(
            // ── canonical error body ───────────────────────────────
            ApiErrorResponse,
            // ── clipboard: enveloped aliases ───────────────────────
            ListEntriesEnvelope,
            EntryDetailEnvelope,
            EntryResourceEnvelope,
            ClipboardStatsEnvelope,
            ClearHistoryEnvelope,
            ToggleFavoriteEnvelope,
            DispatchOutcomeEnvelope,
            ResendEnvelope,
            CancelTransferEnvelope,
            RestoreEntryEnvelope,
            EntryDeliveryViewEnvelope,
            // ── clipboard: payload + request DTOs ──────────────────
            EntryProjectionResponseDto,
            EntryDetailDto,
            EntryResourceDto,
            ClipboardStatsDto,
            ClearHistoryResultDto,
            ToggleFavoriteRequest,
            ToggleFavoriteResultDto,
            DispatchTextRequest,
            DispatchOutcomeResponse,
            PerTargetOutcomeDto,
            ResendRequest,
            ResendResponse,
            CancelTransferRequest,
            CancelTransferResponse,
            // ── clipboard: delivery view (ADR-008 P3-1) ────────────
            EntryDeliveryViewDto,
            EntrySourceDto,
            EntryDeliveryTargetDto,
            EntryDeliveryStatusDto,
            DeliveryFailureReasonDto,
            RestoreEntryResponse,
            // ── search ─────────────────────────────────────────────
            SearchQueryEnvelope,
            SearchStatusEnvelope,
            SearchRebuildEnvelope,
            SearchQueryResultDto,
            SearchStatusData,
            SearchRebuildAcceptedData,
            SearchResultDto,
            // ── storage ────────────────────────────────────────────
            StorageStatsEnvelope,
            ClearCacheEnvelope,
            StorageStatsDto,
            ClearCacheRequest,
            ClearCacheResponse,
            // ── device ─────────────────────────────────────────────
            LocalDeviceInfoEnvelope,
            LocalDeviceInfoDto,
            // ── member ─────────────────────────────────────────────
            MemberSyncPreferencesEnvelope,
            MemberSyncResultEnvelope,
            MemberSyncPreferencesDto,
            MemberSyncResultDto,
            MemberSyncPreferencesPatchDto,
            ContentTypesDto,
            ContentTypesPatchDto,
            // ── pairing ────────────────────────────────────────────
            UnpairDeviceRequest,
            // ── encryption ─────────────────────────────────────────
            EncryptionStateEnvelope,
            EncryptionActionEnvelope,
            KeychainAccessEnvelope,
            UnlockSpaceEnvelope,
            EncryptionStateResponse,
            EncryptionActionResponse,
            KeychainAccessResponse,
            UnlockSpaceRequest,
            UnlockSpaceResponse,
            // ── settings ───────────────────────────────────────────
            SettingsEnvelope,
            SettingsUpdateResultEnvelope,
            SettingsDto,
            SettingsUpdateResultDto,
            SettingsPatchDto,
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
            QuickPanelSettingsDto,
            ShortcutKeyDto,
            ThemeDto,
            UpdateChannelDto,
            // ── settings: PUT /settings patch DTOs (nested children of
            //    SettingsPatchDto, each $ref'd from the request body) ───────
            GeneralSettingsPatchDto,
            SyncSettingsPatchDto,
            RetentionPolicyPatchDto,
            SecuritySettingsPatchDto,
            PairingSettingsPatchDto,
            FileSyncSettingsPatchDto,
            NetworkSettingsPatchDto,
            QuickPanelSettingsPatchDto,
            KeyboardShortcutsPatchDto,
            // ── lifecycle ──────────────────────────────────────────
            LifecycleStatusEnvelope,
            LifecycleStatusResponse,
            // ── upgrade ────────────────────────────────────────────
            UpgradeStatusEnvelope,
            AckUpgradeEnvelope,
            UpgradeStatusDto,
            AckUpgradePayload,
            // ── system: diagnostics & topology ─────────────────────
            StatusEnvelope,
            PeerSnapshotListEnvelope,
            SpaceMemberListEnvelope,
            PresenceRefreshEnvelope,
            HealthResponse,
            StatusResponse,
            WorkerStatusDto,
            PeerSnapshotDto,
            SpaceMemberDto,
            PresenceRefreshResponse,
            // ── websocket protocol schemas ─────────────────────────
            DaemonWsEvent,
            WsSubscribeRequest,
            WsErrorResponse,
            // ── auth/connect (L1/public) ───────────────────────────
            SessionTokenEnvelope,
            SessionTokenResponse,
            ConnectRequest,
            // ── setup-v2 ───────────────────────────────────────────
            SetupInitializeEnvelope,
            SetupIssueInvitationEnvelope,
            SetupRedeemEnvelope,
            SetupStateEnvelope,
            SetupSwitchSpaceEnvelope,
            SetupMigrationProgressEnvelope,
            InitializeSpaceRequest,
            InitializeSpaceResponse,
            IssueInvitationResponse,
            RedeemRequest,
            RedeemResponse,
            SetupStateResponse,
            SwitchSpaceRequest,
            SwitchSpaceResponse,
            MigrationProgressResponse,
            MigrationPhaseDto,
            CurrentInvitation,
        )
    ),
    tags(
        (name = "clipboard", description = "Clipboard entry CRUD, stats, resources, binary blobs/thumbnails, history actions, and delivery"),
        (name = "search", description = "Query, index status, and index rebuild"),
        (name = "storage", description = "Storage stats and cache maintenance"),
        (name = "device", description = "Local device identity"),
        (name = "member", description = "Per-space-member sync preferences"),
        (name = "pairing", description = "Space-member unpair lifecycle"),
        (name = "encryption", description = "Encryption state and session lock/unlock"),
        (name = "settings", description = "Persisted settings read/update (no OS side effects)"),
        (name = "lifecycle", description = "Daemon lifecycle state, retry, and ready-signal"),
        (name = "upgrade", description = "Version upgrade detection and acknowledgement"),
        (name = "system", description = "Diagnostics and topology: health, status, peer/member snapshots, presence, websocket, connect"),
        (name = "setup-v2", description = "Stateless v2 space-setup and invitation flow"),
    )
)]
pub struct ApiDoc;

#[cfg(test)]
mod assembly_smoke_tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    /// Recursively collects every `"$ref"` string value anywhere in `value`.
    fn collect_refs(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, child) in map {
                    if key == "$ref" {
                        if let Value::String(s) = child {
                            out.push(s.clone());
                        }
                    }
                    collect_refs(child, out);
                }
            }
            Value::Array(items) => {
                for item in items {
                    collect_refs(item, out);
                }
            }
            _ => {}
        }
    }

    /// $ref-integrity guard (permanent). Materializes the doc, walks EVERY
    /// `$ref`, and proves each `#/components/schemas/NAME` resolves to a real
    /// component key. This is the guard that the prior `json.contains(name)`
    /// smoke test was too weak to provide (P2 §$ref-integrity blockers).
    #[test]
    fn api_doc_has_no_dangling_refs() {
        let doc = ApiDoc::openapi();
        let value: Value =
            serde_json::to_value(&doc).expect("ApiDoc must serialize to serde_json::Value");

        // The set of declared component schema names.
        let schema_keys: BTreeSet<String> = value
            .get("components")
            .and_then(|c| c.get("schemas"))
            .and_then(Value::as_object)
            .map(|m| m.keys().cloned().collect())
            .expect("OpenAPI doc must declare components.schemas");

        // Walk every `$ref` and assert each schema ref resolves.
        let mut refs = Vec::new();
        collect_refs(&value, &mut refs);
        assert!(!refs.is_empty(), "expected at least one $ref in the doc");

        const SCHEMA_PREFIX: &str = "#/components/schemas/";
        let mut dangling: BTreeSet<String> = BTreeSet::new();
        for r in &refs {
            if let Some(name) = r.strip_prefix(SCHEMA_PREFIX) {
                if !schema_keys.contains(name) {
                    dangling.insert(name.to_string());
                }
            } else {
                panic!("unexpected non-schema $ref form: `{r}`");
            }
        }
        assert!(
            dangling.is_empty(),
            "dangling $refs (not present in components.schemas): {dangling:?}"
        );

        // The bare generic must never leak in as a component key — only the
        // concrete `#[aliases(...)]` instantiations are registered.
        assert!(
            !schema_keys.contains("ApiEnvelope"),
            "bare generic `ApiEnvelope` must never appear as a component key"
        );

        // Endpoint cardinality is frozen by §D. The `paths(...)` list registers
        // 51 handler operations, but 3 paths carry two HTTP methods each
        // (`/settings` GET+PUT, `/clipboard/entries/{id}` GET+DELETE,
        // `/member/{device_id}/sync-preferences` GET+PATCH), so they collapse to
        // 48 unique path templates / 51 operations. Freeze both numbers so a
        // dropped handler OR a dropped path is caught. (ADR-008 P3-1 D15 added
        // `POST /encryption/unlock-with-passphrase`, `POST /encryption/factory-reset`,
        // and `GET /clipboard/entries/{id}/delivery`: +3 paths, +3 operations.)
        const HTTP_METHODS: [&str; 7] =
            ["get", "put", "post", "delete", "patch", "head", "options"];
        let paths = value
            .get("paths")
            .and_then(Value::as_object)
            .expect("OpenAPI doc must declare paths");
        assert_eq!(
            paths.len(),
            48,
            "expected exactly 48 path templates, found {}: {:?}",
            paths.len(),
            paths.keys().collect::<Vec<_>>()
        );
        let operation_count: usize = paths
            .values()
            .filter_map(Value::as_object)
            .map(|item| {
                item.keys()
                    .filter(|k| HTTP_METHODS.contains(&k.as_str()))
                    .count()
            })
            .sum();
        assert_eq!(
            operation_count, 51,
            "expected exactly 51 operations across all paths, found {operation_count}"
        );

        // A few frozen operationIds (§D) must be present somewhere in the doc.
        let json = serde_json::to_string(&value).expect("re-serialize to string");
        for op in [
            "dispatchClipboardText",
            "restoreClipboardEntry",
            "setupV2SwitchSpace",
            "getHealth",
            "authConnect",
        ] {
            assert!(
                json.contains(&format!("\"{op}\"")),
                "expected operationId `{op}` in doc"
            );
        }
    }
}
