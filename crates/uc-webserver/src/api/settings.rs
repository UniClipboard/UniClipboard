//! HTTP route handlers for settings endpoints.
//!
//! Provides read and write access to application settings.
//!
//! NOTE: Unlike the Tauri command (which applies OS-level side effects like
//! autostart registration and global shortcut updates), these handlers only
//! update the settings domain model — no autostart, no keyboard shortcuts.
use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use tracing::{info, instrument};
use utoipa;
use zeroize::Zeroize;

use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use uc_engine::{
    CustomRelayMutation, CustomRelayMutationOutcome, CustomRelayRejection, CustomRelaySummary,
    EngineError, EngineErrorCategory, Operation, OperationResult, RelayCredentialEdit,
    RelayCredentialInput, RelayCredentialStatus, RelayEntrySource, RelayOverview,
    RelayProbeCredential, RelayProbeInput, RelayProbeOutcome, RelayRoutingMode, SaveRelayInput,
    SaveRelayOutcome, SecretString, SettingsPatch, SettingsUpdateOutcome,
};

use crate::api::dto::error::{log_engine_failure, log_facade_failure, ApiError};
use crate::api::dto::settings::{
    CustomRelayDto, CustomRelayMutationDto, CustomRelayMutationResultDto, RelayCredentialEditDto,
    RelayCredentialRequestDto, RelayCredentialStatusDto, RelayEntrySourceDto, RelayOverviewDto,
    RelayOverviewEntryDto, RelayProbeCredentialDto, RelayProbeOutcomeDto, RelayProbeRequestDto,
    RelayRoutingModeDto, RelaySaveRequestDto, RelaySaveResultDto, SettingsDto, SettingsPatchDto,
    SettingsUpdateResultDto,
};
use crate::api::projection::settings::desktop_patch;
use crate::api::projection::{IntoApiDto, IntoDomain};
use crate::api::server::DaemonApiState;
use uc_daemon_contract::api::dto::settings::{
    SettingsPartiallyAppliedDetailsDto, SettingsWriteOwnerDto, SETTINGS_PARTIALLY_APPLIED_CODE,
};

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
        .route("/settings/relay-probe", post(probe_relay_url_handler))
        .route(
            "/settings/relay-credential/status",
            post(get_relay_credential_handler),
        )
        .route("/settings/relay", put(save_relay_handler))
        .route(
            "/settings/custom-relays",
            get(get_custom_relays_handler).post(mutate_custom_relay_handler),
        )
        .route("/settings/relay-overview", get(get_relay_overview_handler))
}

#[utoipa::path(
    get,
    path = "/settings/custom-relays",
    tag = "settings",
    operation_id = "getCustomRelays",
    responses(
        (status = 200, description = "Engine-owned custom relay list", body = CustomRelayListEnvelope),
        (status = 500, description = "Relay query failed", body = ApiErrorResponse),
        (status = 503, description = "Credential storage unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.custom_relays.get", level = "info", skip(state))]
async fn get_custom_relays_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Vec<CustomRelayDto>>>, ApiError> {
    let relays = query_custom_relays(&state).await?;
    info!(relay_count = relays.len(), "custom relay query succeeded");
    Ok(Json(ApiEnvelope::now(relays)))
}

#[utoipa::path(
    get,
    path = "/settings/relay-overview",
    tag = "settings",
    operation_id = "getRelayOverview",
    responses(
        (status = 200, description = "Engine-owned built-in and custom relay overview", body = RelayOverviewEnvelope),
        (status = 500, description = "Relay overview query failed", body = ApiErrorResponse),
        (status = 503, description = "Credential storage unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.relay_overview.get", level = "info", skip(state))]
pub(crate) async fn get_relay_overview_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<RelayOverviewDto>>, ApiError> {
    let result = state
        .execute(Operation::QueryRelayOverview)
        .await
        .map_err(|error| relay_credential_error_to_api("query_relay_overview", error))?;
    let OperationResult::RelayOverview(overview) = result else {
        return Err(relay_credential_unexpected_result_to_api(
            "query_relay_overview",
            "engine returned an unexpected relay overview result",
        ));
    };
    let dto = relay_overview_to_dto(overview);
    info!(
        entry_count = dto.entries.len(),
        change_pending = dto.change_pending,
        "relay overview query succeeded"
    );
    Ok(Json(ApiEnvelope::now(dto)))
}

fn relay_routing_mode_to_dto(mode: RelayRoutingMode) -> RelayRoutingModeDto {
    match mode {
        RelayRoutingMode::BuiltIn => RelayRoutingModeDto::BuiltIn,
        RelayRoutingMode::Custom => RelayRoutingModeDto::Custom,
        RelayRoutingMode::Disabled => RelayRoutingModeDto::Disabled,
    }
}

fn relay_overview_to_dto(overview: RelayOverview) -> RelayOverviewDto {
    RelayOverviewDto {
        saved_mode: relay_routing_mode_to_dto(overview.saved_mode),
        applied_mode: overview.applied_mode.map(relay_routing_mode_to_dto),
        change_pending: overview.change_pending,
        entries: overview
            .entries
            .into_iter()
            .map(|entry| RelayOverviewEntryDto {
                source: match entry.source {
                    RelayEntrySource::BuiltIn => RelayEntrySourceDto::BuiltIn,
                    RelayEntrySource::Custom => RelayEntrySourceDto::Custom,
                },
                region_id: entry.region_id,
                url: entry.url,
                credential_configured: entry.credential_configured,
                in_effect: entry.in_effect,
            })
            .collect(),
    }
}

#[utoipa::path(
    post,
    path = "/settings/custom-relays",
    tag = "settings",
    operation_id = "mutateCustomRelay",
    request_body = CustomRelayMutationDto,
    responses(
        (status = 200, description = "Custom relay mutation applied", body = CustomRelayMutationResultEnvelope),
        (status = 400, description = "Invalid relay URL", body = ApiErrorResponse),
        (status = 404, description = "Relay no longer exists", body = ApiErrorResponse),
        (status = 409, description = "Relay already exists", body = ApiErrorResponse),
        (status = 500, description = "Relay mutation failed", body = ApiErrorResponse),
        (status = 503, description = "Credential storage unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.settings.custom_relays.mutate",
    level = "info",
    skip(state, payload)
)]
async fn mutate_custom_relay_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<CustomRelayMutationDto>,
) -> Result<Json<ApiEnvelope<CustomRelayMutationResultDto>>, ApiError> {
    let relays = match payload {
        CustomRelayMutationDto::Add { url, credential } => {
            let access_token = credential_to_optional_secret(credential)?;
            execute_custom_relay_mutation(&state, CustomRelayMutation::Add { url, access_token })
                .await?
        }
        CustomRelayMutationDto::Edit {
            previous_url,
            url,
            credential: RelayCredentialEditDto::Delete,
        } => {
            let credential_url = credential_deletion_url(previous_url, url)?;
            delete_relay_credential(&state, credential_url).await?;
            query_custom_relays(&state).await?
        }
        CustomRelayMutationDto::Edit {
            previous_url,
            url,
            credential,
        } => {
            let access_token = credential_to_optional_secret(credential)?;
            execute_custom_relay_mutation(
                &state,
                CustomRelayMutation::Edit {
                    previous_url,
                    url,
                    access_token,
                },
            )
            .await?
        }
        CustomRelayMutationDto::Delete { url } => {
            execute_custom_relay_mutation(&state, CustomRelayMutation::Delete { url }).await?
        }
    };

    info!(
        relay_count = relays.len(),
        "custom relay mutation succeeded"
    );
    Ok(Json(ApiEnvelope::now(CustomRelayMutationResultDto {
        relays,
        restart_required: true,
    })))
}

fn credential_deletion_url(previous_url: String, url: String) -> Result<String, ApiError> {
    if previous_url != url {
        return Err(ApiError::bad_request(
            "save the new relay address before removing its credential",
        )
        .with_code("custom_relay_credential_delete_requires_separate_step"));
    }

    Ok(previous_url)
}

fn credential_to_optional_secret(
    credential: RelayCredentialEditDto,
) -> Result<Option<SecretString>, ApiError> {
    match credential {
        RelayCredentialEditDto::Keep => Ok(None),
        RelayCredentialEditDto::Set { mut access_token } => {
            let secret = SecretString::new(&access_token);
            access_token.zeroize();
            Ok(Some(secret))
        }
        RelayCredentialEditDto::Delete => Err(custom_relay_rejection_to_api(
            CustomRelayRejection::InvalidUrl,
        )),
    }
}

async fn query_custom_relays(state: &DaemonApiState) -> Result<Vec<CustomRelayDto>, ApiError> {
    let result = state
        .execute(Operation::QueryCustomRelays)
        .await
        .map_err(|error| relay_credential_error_to_api("query_custom_relays", error))?;
    let OperationResult::CustomRelays(relays) = result else {
        return Err(relay_credential_unexpected_result_to_api(
            "query_custom_relays",
            "engine returned an unexpected custom-relay result",
        ));
    };
    Ok(custom_relays_to_dto(relays))
}

async fn execute_custom_relay_mutation(
    state: &DaemonApiState,
    mutation: CustomRelayMutation,
) -> Result<Vec<CustomRelayDto>, ApiError> {
    let result = state
        .execute(Operation::MutateCustomRelay(mutation))
        .await
        .map_err(|error| relay_credential_error_to_api("mutate_custom_relay", error))?;
    match result {
        OperationResult::CustomRelayMutated(CustomRelayMutationOutcome::Saved { relays }) => {
            Ok(custom_relays_to_dto(relays))
        }
        OperationResult::CustomRelayMutated(CustomRelayMutationOutcome::Rejected { reason }) => {
            Err(custom_relay_rejection_to_api(reason))
        }
        _ => Err(relay_credential_unexpected_result_to_api(
            "mutate_custom_relay",
            "engine returned an unexpected custom-relay mutation result",
        )),
    }
}

async fn delete_relay_credential(state: &DaemonApiState, url: String) -> Result<(), ApiError> {
    let result = state
        .execute(Operation::SaveRelay(Box::new(SaveRelayInput {
            settings: SettingsPatch::default(),
            credential: RelayCredentialEdit::Delete { url },
        })))
        .await
        .map_err(|error| relay_credential_error_to_api("delete_relay_credential", error))?;
    match result {
        OperationResult::RelaySaved(SaveRelayOutcome::Saved { .. }) => Ok(()),
        OperationResult::RelaySaved(SaveRelayOutcome::Rejected { .. }) => Err(
            ApiError::bad_request("custom relay credential deletion was rejected"),
        ),
        _ => Err(relay_credential_unexpected_result_to_api(
            "delete_relay_credential",
            "engine returned an unexpected relay credential deletion result",
        )),
    }
}

fn custom_relays_to_dto(relays: Vec<CustomRelaySummary>) -> Vec<CustomRelayDto> {
    relays
        .into_iter()
        .map(|relay| CustomRelayDto {
            url: relay.url,
            credential_configured: relay.credential_configured,
        })
        .collect()
}

fn custom_relay_rejection_to_api(reason: CustomRelayRejection) -> ApiError {
    match reason {
        CustomRelayRejection::InvalidUrl => {
            ApiError::bad_request("invalid custom relay URL").with_code("custom_relay_invalid_url")
        }
        CustomRelayRejection::Duplicate => {
            ApiError::conflict("custom relay already exists").with_code("custom_relay_duplicate")
        }
        CustomRelayRejection::NotFound => ApiError {
            status: StatusCode::NOT_FOUND,
            code: "custom_relay_not_found".to_string(),
            message: "custom relay no longer exists".to_string(),
            details: None,
        },
    }
}

#[cfg(test)]
mod custom_relay_tests {
    use super::*;

    #[test]
    fn rejection_codes_are_stable_and_distinct() {
        let cases = [
            (
                CustomRelayRejection::InvalidUrl,
                StatusCode::BAD_REQUEST,
                "custom_relay_invalid_url",
            ),
            (
                CustomRelayRejection::Duplicate,
                StatusCode::CONFLICT,
                "custom_relay_duplicate",
            ),
            (
                CustomRelayRejection::NotFound,
                StatusCode::NOT_FOUND,
                "custom_relay_not_found",
            ),
        ];

        for (reason, status, code) in cases {
            let error = custom_relay_rejection_to_api(reason);
            assert_eq!(error.status, status);
            assert_eq!(error.code, code);
        }
    }

    #[test]
    fn projection_exposes_only_public_relay_state() {
        let relays = custom_relays_to_dto(vec![CustomRelaySummary {
            url: "https://relay.example.com/".to_string(),
            credential_configured: true,
        }]);

        assert_eq!(relays.len(), 1);
        assert_eq!(relays[0].url, "https://relay.example.com/");
        assert!(relays[0].credential_configured);
    }

    #[test]
    fn deleting_a_credential_cannot_commit_an_address_edit_first() {
        let rejected = credential_deletion_url(
            "https://old.example.com/".to_string(),
            "https://new.example.com/".to_string(),
        )
        .expect_err("changing the address and deleting its token must be rejected before mutation");
        assert_eq!(rejected.status, StatusCode::BAD_REQUEST);
        assert_eq!(
            rejected.code,
            "custom_relay_credential_delete_requires_separate_step"
        );

        let unchanged = credential_deletion_url(
            "https://old.example.com/".to_string(),
            "https://old.example.com/".to_string(),
        )
        .expect("deleting a credential without changing the address is supported");
        assert_eq!(unchanged, "https://old.example.com/");
    }
}

/// GET /settings
/// Returns the current application settings as a typed Settings struct.
#[utoipa::path(
    get,
    path = "/settings",
    tag = "settings",
    operation_id = "getSettings",
    responses(
        (status = 200, description = "Current application settings", body = SettingsEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.get", level = "info", skip(state))]
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<SettingsDto>>, ApiError> {
    info!("get settings request received");
    let result = state
        .execute(Operation::QuerySettings)
        .await
        .map_err(|error| settings_error_to_api("get_settings", error))?;
    let OperationResult::Settings(settings) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected settings result",
        ));
    };

    let desktop = state.desktop_settings.snapshot().await;

    info!(
        desktop_revision = desktop.revision,
        "get settings succeeded"
    );
    Ok(Json(ApiEnvelope::now(
        (*settings, &desktop.settings).into_api_dto(),
    )))
}

/// PUT /settings
/// Updates application settings. Accepts a partial settings object and splits
/// it by owner (ADR-013 section 8.3): Engine fields to the Engine, product
/// fields to the daemon's product store, `telemetryEnabled` to its own file.
///
/// NOTE: this handler does NOT apply OS-level side effects (no autostart
/// registration, no keyboard shortcut updates). It only persists settings.
#[utoipa::path(
    put,
    path = "/settings",
    tag = "settings",
    operation_id = "updateSettings",
    request_body = SettingsPatchDto,
    responses(
        (status = 200, description = "Settings persisted; carries success + restart-required signal", body = SettingsUpdateResultEnvelope),
        (status = 400, description = "Body does not decode, a product value fails its grammar, or the Engine rejected the patch; nothing was written", body = ApiErrorResponse),
        (status = 500, description = "A write failed. Code `settings_partially_applied` with `SettingsPartiallyAppliedDetailsDto` details when an earlier owner was written; `internal_error` when nothing was written", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.update", level = "info", skip(state, body))]
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    body: Result<Json<SettingsPatchDto>, JsonRejection>,
) -> Result<Json<ApiEnvelope<SettingsUpdateResultDto>>, ApiError> {
    // The rejection text can quote the submitted value, so it is not logged or returned.
    let Json(payload) = body.map_err(|_| ApiError::bad_request("invalid settings request body"))?;

    // D-D1: `network` changes and `debugMode` take effect after a restart.
    // The frontend still sends whole sections, so this is coarse until task 5
    // moves it to diff patches.
    let debug_mode_changed = payload
        .general
        .as_ref()
        .and_then(|general| general.debug_mode)
        .is_some();
    let restart_required = payload.network.is_some() || debug_mode_changed;

    let telemetry_update = payload.general.as_ref().and_then(|g| g.telemetry_enabled);
    let analytics_update = payload
        .general
        .as_ref()
        .and_then(|g| g.usage_analytics_enabled);

    // Validate every local value before any store is written.
    let desktop_patch = desktop_patch(&payload);
    state
        .desktop_settings
        .validate(&desktop_patch)
        .await
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let engine_patch: SettingsPatch = payload.into_domain();
    let has_engine = engine_patch != SettingsPatch::default();
    let has_desktop = !desktop_patch.is_empty();
    info!(
        has_engine,
        has_desktop,
        has_telemetry = telemetry_update.is_some(),
        "update settings request received"
    );

    let mut applied = Vec::new();
    if has_engine {
        let result = state
            .execute(Operation::UpdateSettings(Box::new(engine_patch)))
            .await
            .map_err(|error| settings_error_to_api("update_settings", error))?;
        match result {
            OperationResult::SettingsUpdated(SettingsUpdateOutcome::Updated(_)) => {}
            OperationResult::SettingsUpdated(SettingsUpdateOutcome::Rejected { reason }) => {
                return Err(ApiError::bad_request(reason));
            }
            _ => {
                return Err(ApiError::internal(
                    "engine returned an unexpected settings-update result",
                ));
            }
        }
        applied.push(SettingsWriteOwnerDto::Engine);
    }

    if has_desktop {
        if let Err(error) = state.desktop_settings.apply(desktop_patch).await {
            tracing::warn!(error = %error, error_kind = "desktop_settings_save_failed", "Failed to save product settings");
            let skipped = telemetry_update
                .map(|_| vec![SettingsWriteOwnerDto::Telemetry])
                .unwrap_or_default();
            return Err(write_failure(
                applied,
                SettingsWriteOwnerDto::Desktop,
                skipped,
            ));
        }
        applied.push(SettingsWriteOwnerDto::Desktop);
        if let Some(enabled) = analytics_update {
            uc_observability::set_analytics_enabled(enabled);
        }
    }

    if let Some(enabled) = telemetry_update {
        if let Err(error) = uc_observability::telemetry_gate::save_preference(enabled) {
            tracing::warn!(error = %error, error_kind = "telemetry_preference_save_failed", "Failed to save error reporting preference");
            return Err(write_failure(
                applied,
                SettingsWriteOwnerDto::Telemetry,
                Vec::new(),
            ));
        }
    }

    info!(restart_required, "update settings succeeded");
    // ADR-008 §0.1: wire is `ApiEnvelope<SettingsUpdateResultDto>` —
    // `{ data: { success, restartRequired }, ts }`. The previously top-level
    // `success` / `restartRequired` siblings are folded into the payload.
    Ok(Json(ApiEnvelope::now(SettingsUpdateResultDto {
        success: true,
        restart_required,
    })))
}

/// POST /settings/relay-probe
///
/// Probes a candidate relay URL for reachability. An optional one-time access
/// token is used only for this probe and is never persisted. A probe
/// that fails to reach the relay is a NORMAL categorized outcome returned 200
/// (mirrors the Tauri command contract) — only a missing relay-diagnostic
/// adapter (server misconfiguration) becomes a 500 `ApiError`.
#[utoipa::path(
    post,
    path = "/settings/relay-probe",
    tag = "settings",
    operation_id = "probeRelayUrl",
    request_body = RelayProbeRequestDto,
    responses(
        (status = 200, description = "Relay probe outcome (reachable or a categorized failure)", body = RelayProbeOutcomeEnvelope),
        (status = 500, description = "Relay-diagnostic internal error", body = ApiErrorResponse),
        (status = 503, description = "Relay-diagnostic adapter unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.settings.relay_probe",
    level = "info",
    skip(state, payload)
)]
async fn probe_relay_url_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<RelayProbeRequestDto>,
) -> Result<Json<ApiEnvelope<RelayProbeOutcomeDto>>, ApiError> {
    info!("relay probe request received");
    let credential = match payload.credential {
        RelayProbeCredentialDto::Stored => RelayProbeCredential::Stored,
        RelayProbeCredentialDto::None => RelayProbeCredential::None,
        RelayProbeCredentialDto::Override { mut access_token } => {
            let secret = SecretString::new(&access_token);
            access_token.zeroize();
            RelayProbeCredential::Override(secret)
        }
    };
    let result = state
        .execute(Operation::ProbeRelay(RelayProbeInput {
            url: payload.url,
            credential,
        }))
        .await
        .map_err(|error| settings_error_to_api("relay_probe", error))?;
    let OperationResult::RelayProbed(outcome) = result else {
        return Err(ApiError::internal(
            "engine returned an unexpected relay-probe result",
        ));
    };

    info!("relay probe completed");
    Ok(Json(ApiEnvelope::now(probe_outcome_to_dto(outcome))))
}

/// POST /settings/relay-credential/status
/// Returns only whether a credential exists for the relay URL.
#[utoipa::path(
    post,
    path = "/settings/relay-credential/status",
    tag = "settings",
    operation_id = "getRelayCredentialStatus",
    request_body = RelayCredentialRequestDto,
    responses(
        (status = 200, description = "Relay credential configuration state", body = RelayCredentialStatusEnvelope),
        (status = 400, description = "Invalid relay URL", body = ApiErrorResponse),
        (status = 500, description = "Credential storage failed", body = ApiErrorResponse),
        (status = 503, description = "Credential storage unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(
    name = "api.settings.relay_credential.get",
    level = "info",
    skip(state, payload)
)]
async fn get_relay_credential_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<RelayCredentialRequestDto>,
) -> Result<Json<ApiEnvelope<RelayCredentialStatusDto>>, ApiError> {
    info!("relay credential status request received");
    let result = state
        .execute(Operation::QueryRelayCredential(RelayCredentialInput {
            url: payload.url,
        }))
        .await
        .map_err(|error| relay_credential_error_to_api("query", error))?;
    let OperationResult::RelayCredentialStatus(status) = result else {
        return Err(relay_credential_unexpected_result_to_api(
            "query",
            "engine returned an unexpected relay-credential result",
        ));
    };

    info!(
        configured = status.configured,
        "relay credential status returned"
    );
    Ok(Json(ApiEnvelope::now(relay_credential_status_to_dto(
        status,
    ))))
}

/// PUT /settings/relay
/// Saves the relay URL list and its credential as one recoverable operation.
#[utoipa::path(
    put,
    path = "/settings/relay",
    tag = "settings",
    operation_id = "saveRelay",
    request_body = RelaySaveRequestDto,
    responses(
        (status = 200, description = "Relay settings and credential saved", body = RelaySaveResultEnvelope),
        (status = 400, description = "Invalid relay URL, duplicate URL, or access token", body = ApiErrorResponse),
        (status = 500, description = "Relay settings save failed", body = ApiErrorResponse),
        (status = 503, description = "Credential storage unavailable", body = ApiErrorResponse)
    )
)]
#[instrument(name = "api.settings.relay.save", level = "info", skip(state, payload))]
async fn save_relay_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<RelaySaveRequestDto>,
) -> Result<Json<ApiEnvelope<RelaySaveResultDto>>, ApiError> {
    info!("relay settings save request received");
    let RelaySaveRequestDto {
        settings,
        url,
        credential,
    } = payload;
    // The relay save is an Engine operation; product and telemetry fields
    // belong to other owners and must never reach the Engine (ADR-013 section 8.3).
    let carries_other_owner = !desktop_patch(&settings).is_empty()
        || settings
            .general
            .as_ref()
            .is_some_and(|general| general.telemetry_enabled.is_some());
    if carries_other_owner {
        return Err(ApiError::bad_request(
            "relay save accepts Engine settings only",
        ));
    }
    let credential = match credential {
        RelayCredentialEditDto::Keep => RelayCredentialEdit::Keep { url },
        RelayCredentialEditDto::Set { mut access_token } => {
            let secret = SecretString::new(&access_token);
            access_token.zeroize();
            RelayCredentialEdit::Set {
                url,
                access_token: secret,
            }
        }
        RelayCredentialEditDto::Delete => RelayCredentialEdit::Delete { url },
    };
    let result = state
        .execute(Operation::SaveRelay(Box::new(SaveRelayInput {
            settings: settings.into_domain(),
            credential,
        })))
        .await
        .map_err(|error| relay_credential_error_to_api("save", error))?;
    let (settings, status) = match result {
        OperationResult::RelaySaved(SaveRelayOutcome::Saved {
            settings,
            credential_status,
        }) => {
            let desktop = state.desktop_settings.snapshot().await;
            (
                (*settings, &desktop.settings).into_api_dto(),
                credential_status,
            )
        }
        OperationResult::RelaySaved(SaveRelayOutcome::Rejected { reason }) => {
            return Err(ApiError::bad_request(reason));
        }
        _ => {
            return Err(relay_credential_unexpected_result_to_api(
                "save",
                "engine returned an unexpected relay-save result",
            ));
        }
    };

    info!(configured = status.configured, "relay settings saved");
    Ok(Json(ApiEnvelope::now(RelaySaveResultDto {
        success: true,
        restart_required: true,
        credential_status: relay_credential_status_to_dto(status),
        settings,
    })))
}

/// The error for a failed write after validation. Partial when an earlier
/// owner's part stays written (ADR-013 section 8.3).
fn write_failure(
    applied: Vec<SettingsWriteOwnerDto>,
    failed: SettingsWriteOwnerDto,
    skipped: Vec<SettingsWriteOwnerDto>,
) -> ApiError {
    if applied.is_empty() {
        return ApiError::internal(match failed {
            SettingsWriteOwnerDto::Desktop => "failed to save desktop settings",
            SettingsWriteOwnerDto::Telemetry => "failed to save error reporting preference",
            SettingsWriteOwnerDto::Engine => "failed to save engine settings",
        });
    }
    let details = SettingsPartiallyAppliedDetailsDto {
        applied,
        failed: vec![failed],
        skipped,
    };
    tracing::warn!(
        applied = ?details.applied,
        failed = ?details.failed,
        skipped = ?details.skipped,
        error_kind = "settings_partially_applied",
        "Settings update partially applied"
    );
    ApiError::internal("settings were partially applied")
        .with_code(SETTINGS_PARTIALLY_APPLIED_CODE)
        .with_details(serde_json::to_value(&details).unwrap_or_default())
}

fn relay_credential_status_to_dto(status: RelayCredentialStatus) -> RelayCredentialStatusDto {
    RelayCredentialStatusDto {
        configured: status.configured,
    }
}

fn relay_credential_unexpected_result_to_api(op: &'static str, message: &'static str) -> ApiError {
    let api = ApiError::internal(message);
    log_facade_failure(
        "settings",
        op,
        "unexpected_result",
        api.status,
        &api.message,
    );
    api
}

/// Translate a `probe_relay_url` facade result into the wire outcome.
///
/// The probe-failure variants are expected user-facing outcomes (returned 200
/// so the FE can pick copy without catching an exception — parity with the
/// Tauri command). A missing relay-diagnostic adapter (`RelayProbeUnavailable`)
/// or any non-probe variant leaking through is a genuine server-side fault and
/// is propagated as `Err` for the caller to map to a 500.
fn probe_outcome_to_dto(outcome: RelayProbeOutcome) -> RelayProbeOutcomeDto {
    match outcome {
        RelayProbeOutcome::Success { latency_ms } => RelayProbeOutcomeDto::Success { latency_ms },
        RelayProbeOutcome::InvalidUrl { message } => RelayProbeOutcomeDto::InvalidUrl { message },
        RelayProbeOutcome::Dns { message } => RelayProbeOutcomeDto::Dns { message },
        RelayProbeOutcome::Tls { message } => RelayProbeOutcomeDto::Tls { message },
        RelayProbeOutcome::Handshake { message } => RelayProbeOutcomeDto::Handshake { message },
        RelayProbeOutcome::Timeout => RelayProbeOutcomeDto::Timeout,
        RelayProbeOutcome::Other { message } => RelayProbeOutcomeDto::Other { message },
    }
}

fn settings_error_to_api(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.category() {
        EngineErrorCategory::InvalidInput => {
            ("invalid_input", ApiError::bad_request("invalid settings"))
        }
        EngineErrorCategory::Unavailable | EngineErrorCategory::DeadlineExceeded => (
            "unavailable",
            ApiError::service_unavailable("settings service is unavailable"),
        ),
        EngineErrorCategory::InvalidState
        | EngineErrorCategory::Unauthorized
        | EngineErrorCategory::NotFound
        | EngineErrorCategory::Conflict
        | EngineErrorCategory::Internal => (
            "operation_failed",
            ApiError::internal("settings operation failed"),
        ),
    };
    log_engine_failure("settings", op, variant, api, &error)
}

fn relay_credential_error_to_api(op: &'static str, error: EngineError) -> ApiError {
    let (variant, api): (&'static str, ApiError) = match error.category() {
        EngineErrorCategory::InvalidInput => (
            "invalid_input",
            ApiError::bad_request("invalid relay credential"),
        ),
        EngineErrorCategory::Unavailable | EngineErrorCategory::DeadlineExceeded => (
            "unavailable",
            ApiError::service_unavailable("relay credential storage is unavailable"),
        ),
        EngineErrorCategory::InvalidState
        | EngineErrorCategory::Unauthorized
        | EngineErrorCategory::NotFound
        | EngineErrorCategory::Conflict
        | EngineErrorCategory::Internal => (
            "operation_failed",
            ApiError::internal("relay credential operation failed"),
        ),
    };
    log_engine_failure("settings", op, variant, api, &error)
}

// All view↔DTO field mappings live in `crate::api::projection::settings`
// (single source of truth per architecture-rules §Cross-Crate Type Conversion).

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_outcome_maps_success_with_latency() {
        let out = probe_outcome_to_dto(RelayProbeOutcome::Success { latency_ms: 42 });
        assert_eq!(out, RelayProbeOutcomeDto::Success { latency_ms: 42 });
    }

    #[test]
    fn probe_outcome_maps_each_probe_failure_to_200_variant() {
        let cases = [
            (
                RelayProbeOutcome::InvalidUrl {
                    message: "bad".into(),
                },
                RelayProbeOutcomeDto::InvalidUrl {
                    message: "bad".into(),
                },
            ),
            (
                RelayProbeOutcome::Dns {
                    message: "nxdomain".into(),
                },
                RelayProbeOutcomeDto::Dns {
                    message: "nxdomain".into(),
                },
            ),
            (
                RelayProbeOutcome::Tls {
                    message: "cert".into(),
                },
                RelayProbeOutcomeDto::Tls {
                    message: "cert".into(),
                },
            ),
            (
                RelayProbeOutcome::Handshake {
                    message: "nope".into(),
                },
                RelayProbeOutcomeDto::Handshake {
                    message: "nope".into(),
                },
            ),
            (RelayProbeOutcome::Timeout, RelayProbeOutcomeDto::Timeout),
            (
                RelayProbeOutcome::Other {
                    message: "boom".into(),
                },
                RelayProbeOutcomeDto::Other {
                    message: "boom".into(),
                },
            ),
        ];
        for (outcome, expected) in cases {
            assert_eq!(probe_outcome_to_dto(outcome), expected);
        }
    }

    #[test]
    fn relay_credential_status_projection_never_contains_a_secret() {
        let dto = relay_credential_status_to_dto(RelayCredentialStatus { configured: true });
        assert_eq!(
            serde_json::to_value(dto).expect("serialize relay credential status"),
            serde_json::json!({ "configured": true })
        );
    }
}
