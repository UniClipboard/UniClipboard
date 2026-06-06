//! HTTP handlers for the mobile-sync loopback endpoints (ADR-008 P3-b).
//!
//! Ported from the former GUI-only `mobile_sync` Tauri commands when the GUI
//! moved onto the daemon HTTP API. Each handler is a thin passthrough to the
//! daemon-resident [`MobileSyncFacade`] (the same facade + input structs the
//! Tauri commands called), wrapping the result in the canonical envelope.
//!
//! # Error wire form
//!
//! The semantic [`MobileSyncError`] taxonomy (with its structured fields:
//! `LabelTooLong{max}`, `UsernameTaken{username}`, `UsernameTooShort{min,got}`,
//! …) is preserved verbatim from the Tauri layer, minus `specta`. It converts
//! into [`ApiError`] by serializing to `{ code, ...fields }`, splitting `code`
//! out as the semantic tag and parking the remaining fields on
//! [`ApiError::with_details`] — so the FE translator reconstructs the SAME
//! discriminated union it switched on before, off `DaemonApiError.details`.
//! Statuses follow the established severity split (UserError → 4xx, SystemError
//! → 5xx) and deliberately avoid `401` so `callSdk` does not fire a spurious
//! session refresh + retry on a user-recoverable outcome.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use serde::Serialize;
use serde_json::Value;
use tracing::{info_span, Instrument};
use uc_application::facade::mobile_sync::{
    GetMobileSyncSettingsError, LanInterfaceOption, ListLanInterfacesError, ListMobileDevicesError,
    MobileDeviceSummary, MobileSyncFacade, MobileSyncSettingsView,
    RegisterMobileShortcutDeviceError, RegisterMobileShortcutDeviceInput,
    RegisterMobileShortcutDeviceOutput, RevokeMobileDeviceError, RevokeMobileDeviceInput,
    RotateMobilePasswordError, RotateMobilePasswordInput, RotateMobilePasswordOutput,
    ShortcutInstallMethod, ShortcutInstallMethodOption, UpdateMobileSyncSettingsError,
    UpdateMobileSyncSettingsInput, UpdateMobileSyncSettingsOutput,
};
use uc_core::mobile_sync::{MobileClientType, MobileDeviceId};
use uc_daemon_contract::api::dto::envelope::ApiEnvelope;
use utoipa;

use crate::api::dto::error::{log_facade_failure, ApiError};
use crate::api::dto::mobile_sync::{
    LanInterfaceViewDto, MobileDeviceViewDto, MobileSyncActionResultDto, MobileSyncSettingsViewDto,
    RegisterMobileDeviceRequest, RegisterMobileDeviceResultDto, RotateMobilePasswordRequest,
    RotateMobilePasswordResultDto, ShortcutInstallMethodViewDto, UpdateMobileSyncSettingsRequest,
    UpdateMobileSyncSettingsResultDto,
};
use crate::api::server::DaemonApiState;

// ============================================================================
// Error taxonomy ─ semantic union, identical wire form to the old Tauri error
// ============================================================================

/// label length cap — mirror of the facade constant (`MAX_LABEL_LEN = 64`).
const LABEL_MAX_LEN: usize = 64;

/// Semantic mobile-sync error. Serializes to `{"code": "USERNAME_TAKEN",
/// "username": "..."}` — the exact shape the frontend `MobileSyncError` union
/// switches on. Not registered in the OpenAPI doc (the wire error body is the
/// canonical `ApiErrorResponse`); it exists only to carry `code` + structured
/// fields from the typed facade error into [`ApiError`].
#[derive(Debug, Clone, Serialize, thiserror::Error)]
#[serde(
    tag = "code",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum MobileSyncError {
    #[error("mobile sync facade not available in this runtime")]
    FacadeUnavailable,
    #[error("device label must not be empty")]
    LabelEmpty,
    #[error("device label too long (max {max})")]
    LabelTooLong { max: usize },
    #[error("LAN listener disabled; enable it first")]
    LanListenerDisabled,
    #[error("username already taken: {username}")]
    UsernameTaken { username: String },
    #[error("username too short: must be at least {min} characters (got {got})")]
    UsernameTooShort { min: usize, got: usize },
    #[error("username too long: must be at most {max} characters (got {got})")]
    UsernameTooLong { max: usize, got: usize },
    #[error("username must start with an ASCII letter")]
    UsernameMustStartWithLetter,
    #[error("username contains forbidden characters (only letters, digits, underscore allowed)")]
    UsernameContainsForbiddenChars,
    #[error("password too short (min {min})")]
    PasswordTooShort { min: usize },
    #[error("password too long (max {max})")]
    PasswordTooLong { max: usize },
    #[error("password hashing failed: {message}")]
    PasswordHashFailed { message: String },
    #[error("device not found: {device_id}")]
    DeviceNotFound { device_id: String },
    #[error("invalid LAN parameter: {reason}")]
    InvalidLanParameter { reason: String },
    #[error("settings load failed: {message}")]
    SettingsLoadFailed { message: String },
    #[error("settings save failed: {message}")]
    SettingsSaveFailed { message: String },
    #[error("endpoint info probe failed: {message}")]
    EndpointInfoFailed { message: String },
    #[error("LAN probe failed: {message}")]
    LanProbeFailed { message: String },
    #[error("no usable LAN interface for auto-pick base_url")]
    NoLanInterfaceAvailable,
    #[error("persistence failed: {message}")]
    PersistenceFailed { message: String },
    #[error("QR rendering failed: {message}")]
    QrRenderFailed { message: String },
}

impl MobileSyncError {
    /// `(status, variant_name)`. Status follows the established severity split
    /// (see the old `classify!(MobileSyncError {...})` taxonomy): UserError →
    /// 4xx (no Sentry escalation), SystemError → 5xx (root-cause logged). Never
    /// `401` — `callSdk` would treat it as an expired session and retry.
    fn classify(&self) -> (StatusCode, &'static str) {
        use MobileSyncError as E;
        use StatusCode as S;
        match self {
            // ── SystemError → 5xx ──────────────────────────────────────
            E::FacadeUnavailable => (S::SERVICE_UNAVAILABLE, "facade_unavailable"),
            E::PasswordHashFailed { .. } => (S::INTERNAL_SERVER_ERROR, "password_hash_failed"),
            E::SettingsLoadFailed { .. } => (S::INTERNAL_SERVER_ERROR, "settings_load_failed"),
            E::SettingsSaveFailed { .. } => (S::INTERNAL_SERVER_ERROR, "settings_save_failed"),
            E::EndpointInfoFailed { .. } => (S::INTERNAL_SERVER_ERROR, "endpoint_info_failed"),
            E::LanProbeFailed { .. } => (S::INTERNAL_SERVER_ERROR, "lan_probe_failed"),
            E::NoLanInterfaceAvailable => (S::INTERNAL_SERVER_ERROR, "no_lan_interface_available"),
            E::PersistenceFailed { .. } => (S::INTERNAL_SERVER_ERROR, "persistence_failed"),
            E::QrRenderFailed { .. } => (S::INTERNAL_SERVER_ERROR, "qr_render_failed"),
            // ── UserError → 4xx ────────────────────────────────────────
            E::DeviceNotFound { .. } => (S::NOT_FOUND, "device_not_found"),
            E::UsernameTaken { .. } => (S::CONFLICT, "username_taken"),
            E::LanListenerDisabled => (S::CONFLICT, "lan_listener_disabled"),
            E::LabelEmpty => (S::UNPROCESSABLE_ENTITY, "label_empty"),
            E::LabelTooLong { .. } => (S::UNPROCESSABLE_ENTITY, "label_too_long"),
            E::UsernameTooShort { .. } => (S::UNPROCESSABLE_ENTITY, "username_too_short"),
            E::UsernameTooLong { .. } => (S::UNPROCESSABLE_ENTITY, "username_too_long"),
            E::UsernameMustStartWithLetter => {
                (S::UNPROCESSABLE_ENTITY, "username_must_start_with_letter")
            }
            E::UsernameContainsForbiddenChars => {
                (S::UNPROCESSABLE_ENTITY, "username_contains_forbidden_chars")
            }
            E::PasswordTooShort { .. } => (S::UNPROCESSABLE_ENTITY, "password_too_short"),
            E::PasswordTooLong { .. } => (S::UNPROCESSABLE_ENTITY, "password_too_long"),
            E::InvalidLanParameter { .. } => (S::UNPROCESSABLE_ENTITY, "invalid_lan_parameter"),
        }
    }
}

impl From<MobileSyncError> for ApiError {
    fn from(err: MobileSyncError) -> Self {
        let (status, variant) = err.classify();
        let message = err.to_string();
        // Serialize to `{ code, ...fields }`, then split `code` out as the
        // semantic tag and keep the remaining structured fields as `details`.
        let mut value = serde_json::to_value(&err).unwrap_or(Value::Null);
        let code = value
            .get("code")
            .and_then(Value::as_str)
            .unwrap_or("INTERNAL")
            .to_string();
        let details = match value.as_object_mut() {
            Some(map) => {
                map.remove("code");
                if map.is_empty() {
                    None
                } else {
                    Some(Value::Object(map.clone()))
                }
            }
            None => None,
        };
        log_facade_failure("mobile_sync", variant, variant, status, &message);
        let api = ApiError {
            status,
            code,
            message,
            details: None,
        };
        match details {
            Some(d) => api.with_details(d),
            None => api,
        }
    }
}

/// `?`-friendly: collapse a typed facade error → `MobileSyncError` → `ApiError`
/// in one `.map_err(to_api)`. (`?` cannot chain two `From` conversions.)
fn to_api<E: Into<MobileSyncError>>(err: E) -> ApiError {
    ApiError::from(err.into())
}

// ── facade error → MobileSyncError (verbatim from the former Tauri layer) ───

impl From<RegisterMobileShortcutDeviceError> for MobileSyncError {
    fn from(err: RegisterMobileShortcutDeviceError) -> Self {
        use RegisterMobileShortcutDeviceError as E;
        match err {
            E::LabelEmpty => Self::LabelEmpty,
            E::LabelTooLong => Self::LabelTooLong { max: LABEL_MAX_LEN },
            E::LanListenerDisabled => Self::LanListenerDisabled,
            E::UsernameTaken(username) => Self::UsernameTaken { username },
            E::UsernameTooShort { min, got } => Self::UsernameTooShort { min, got },
            E::UsernameTooLong { max, got } => Self::UsernameTooLong { max, got },
            E::UsernameMustStartWithLetter => Self::UsernameMustStartWithLetter,
            E::UsernameContainsForbiddenChars => Self::UsernameContainsForbiddenChars,
            E::PasswordTooShort { min } => Self::PasswordTooShort { min },
            E::PasswordTooLong { max } => Self::PasswordTooLong { max },
            E::PasswordHashFailed(message) => Self::PasswordHashFailed { message },
            E::PersistenceFailed(message) => Self::PersistenceFailed { message },
            E::QrRenderFailed(message) => Self::QrRenderFailed { message },
            E::SettingsLoadFailed(message) => Self::SettingsLoadFailed { message },
            E::NoLanInterfaceAvailable => Self::NoLanInterfaceAvailable,
            E::LanInterfaceProbeFailed(message) => Self::LanProbeFailed { message },
        }
    }
}

impl From<RevokeMobileDeviceError> for MobileSyncError {
    fn from(err: RevokeMobileDeviceError) -> Self {
        match err {
            RevokeMobileDeviceError::NotFound(device_id) => Self::DeviceNotFound { device_id },
            RevokeMobileDeviceError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<ListMobileDevicesError> for MobileSyncError {
    fn from(err: ListMobileDevicesError) -> Self {
        match err {
            ListMobileDevicesError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<RotateMobilePasswordError> for MobileSyncError {
    fn from(err: RotateMobilePasswordError) -> Self {
        match err {
            RotateMobilePasswordError::NotFound(id) => Self::DeviceNotFound {
                device_id: id.into_string(),
            },
            RotateMobilePasswordError::PasswordTooShort { min } => Self::PasswordTooShort { min },
            RotateMobilePasswordError::PasswordTooLong { max } => Self::PasswordTooLong { max },
            RotateMobilePasswordError::PasswordHashFailed(message) => {
                Self::PasswordHashFailed { message }
            }
            RotateMobilePasswordError::PersistenceFailed(message) => {
                Self::PersistenceFailed { message }
            }
        }
    }
}

impl From<GetMobileSyncSettingsError> for MobileSyncError {
    fn from(err: GetMobileSyncSettingsError) -> Self {
        match err {
            GetMobileSyncSettingsError::SettingsLoadFailed(message) => {
                Self::SettingsLoadFailed { message }
            }
            GetMobileSyncSettingsError::EndpointInfoFailed(message) => {
                Self::EndpointInfoFailed { message }
            }
        }
    }
}

impl From<UpdateMobileSyncSettingsError> for MobileSyncError {
    fn from(err: UpdateMobileSyncSettingsError) -> Self {
        match err {
            UpdateMobileSyncSettingsError::SettingsLoadFailed(message) => {
                Self::SettingsLoadFailed { message }
            }
            UpdateMobileSyncSettingsError::SettingsSaveFailed(message) => {
                Self::SettingsSaveFailed { message }
            }
            UpdateMobileSyncSettingsError::InvalidLanParameter(reason) => {
                Self::InvalidLanParameter { reason }
            }
        }
    }
}

impl From<ListLanInterfacesError> for MobileSyncError {
    fn from(err: ListLanInterfacesError) -> Self {
        match err {
            ListLanInterfacesError::ProbeFailed(message) => Self::LanProbeFailed { message },
        }
    }
}

// ============================================================================
// Domain → contract DTO conversions (free fns: orphan rule forbids `From`
// between two crate-foreign types)
// ============================================================================

fn client_type_wire(t: &MobileClientType) -> String {
    t.as_wire_str().to_string()
}

/// Encode the two QR PNGs to base64 daemon-side so the JSON DTO ships strings
/// the frontend `<img src="data:image/png;base64,...">` renders directly
/// (ported from the former Tauri boundary conversion).
fn to_register_dto(out: RegisterMobileShortcutDeviceOutput) -> RegisterMobileDeviceResultDto {
    RegisterMobileDeviceResultDto {
        device_id: out.device.device_id.into_string(),
        label: out.device.label,
        client_type: client_type_wire(&out.device.client_type),
        created_at_ms: out.device.created_at_ms,
        base_url: out.base_url,
        username: out.username,
        password: out.password,
        install_url: out.install_url,
        install_qr_code_png_base64: BASE64.encode(out.install_qr_code_png_bytes),
        connect_uri: out.connect_uri,
        qr_code_png_base64: BASE64.encode(out.qr_code_png_bytes),
    }
}

fn to_rotate_dto(out: RotateMobilePasswordOutput) -> RotateMobilePasswordResultDto {
    RotateMobilePasswordResultDto {
        device_id: out.device_id.into_string(),
        username: out.username,
        password: out.password,
    }
}

fn to_device_view(s: MobileDeviceSummary) -> MobileDeviceViewDto {
    MobileDeviceViewDto {
        device_id: s.device_id.into_string(),
        label: s.label,
        client_type: client_type_wire(&s.client_type),
        username: s.username,
        created_at_ms: s.created_at_ms,
        last_seen_at_ms: s.last_seen_at_ms,
        last_seen_ip: s.last_seen_ip,
        reported_name: s.reported_name,
        reported_os: s.reported_os,
    }
}

fn to_install_method_view(o: ShortcutInstallMethodOption) -> ShortcutInstallMethodViewDto {
    let method = match o.method {
        ShortcutInstallMethod::TokenInjected => "tokenInjected",
        ShortcutInstallMethod::IcloudGeneric => "icloudGeneric",
    };
    ShortcutInstallMethodViewDto {
        method: method.to_string(),
        available: o.available,
        disabled_reason: o.disabled_reason,
    }
}

fn to_settings_view(v: MobileSyncSettingsView) -> MobileSyncSettingsViewDto {
    MobileSyncSettingsViewDto {
        enabled: v.enabled,
        lan_listen_enabled: v.lan_listen_enabled,
        lan_advertise_ip: v.lan_advertise_ip,
        lan_port: v.lan_port,
        lan_listener_error: v.lan_listener_error,
        shortcut_install_methods: v
            .shortcut_install_methods
            .into_iter()
            .map(to_install_method_view)
            .collect(),
    }
}

fn to_update_result(o: UpdateMobileSyncSettingsOutput) -> UpdateMobileSyncSettingsResultDto {
    UpdateMobileSyncSettingsResultDto {
        enabled: o.enabled,
        lan_listen_enabled: o.lan_listen_enabled,
        lan_advertise_ip: o.lan_advertise_ip,
        lan_port: o.lan_port,
        restart_required: o.restart_required,
        lan_listener_bind_error: o.lan_listener_bind_error,
    }
}

fn to_lan_interface_view(o: LanInterfaceOption) -> LanInterfaceViewDto {
    LanInterfaceViewDto {
        name: o.name,
        ipv4: o.ipv4,
    }
}

// ============================================================================
// Router + handlers
// ============================================================================

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route(
            "/mobile-sync/devices",
            get(list_mobile_devices_handler).post(register_mobile_device_handler),
        )
        .route(
            "/mobile-sync/devices/:device_id",
            delete(revoke_mobile_device_handler),
        )
        .route(
            "/mobile-sync/devices/:device_id/rotate-password",
            post(rotate_mobile_password_handler),
        )
        .route(
            "/mobile-sync/settings",
            get(get_mobile_sync_settings_handler).patch(update_mobile_sync_settings_handler),
        )
        .route(
            "/mobile-sync/lan-interfaces",
            get(list_mobile_lan_interfaces_handler),
        )
}

/// Resolve the daemon-resident mobile-sync facade, or 503 `FACADE_UNAVAILABLE`.
fn mobile_sync_facade(state: &DaemonApiState) -> Result<Arc<MobileSyncFacade>, ApiError> {
    let app = state.app_facade_or_error()?;
    app.mobile_sync
        .get()
        .cloned()
        .ok_or_else(|| ApiError::from(MobileSyncError::FacadeUnavailable))
}

/// POST /mobile-sync/devices
#[utoipa::path(
    post,
    path = "/mobile-sync/devices",
    operation_id = "registerMobileDevice",
    tag = "mobile-sync",
    request_body = RegisterMobileDeviceRequest,
    responses(
        (status = 200, description = "Device registered (one-time password echo)", body = RegisterMobileDeviceEnvelope),
        (status = 409, description = "Username taken / LAN listener disabled", body = ApiErrorResponse),
        (status = 422, description = "Invalid label / username / password", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn register_mobile_device_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<RegisterMobileDeviceRequest>,
) -> Result<Json<ApiEnvelope<RegisterMobileDeviceResultDto>>, ApiError> {
    let span = info_span!("api.mobile_sync.register_device");
    async move {
        let facade = mobile_sync_facade(&state)?;
        let out = facade
            .register_device(RegisterMobileShortcutDeviceInput {
                label: req.label,
                username: req.username,
                password: req.password,
            })
            .await
            .map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(to_register_dto(out))))
    }
    .instrument(span)
    .await
}

/// GET /mobile-sync/devices
#[utoipa::path(
    get,
    path = "/mobile-sync/devices",
    operation_id = "listMobileDevices",
    tag = "mobile-sync",
    responses(
        (status = 200, description = "Registered devices", body = MobileDeviceListEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn list_mobile_devices_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Vec<MobileDeviceViewDto>>>, ApiError> {
    let span = info_span!("api.mobile_sync.list_devices");
    async move {
        let facade = mobile_sync_facade(&state)?;
        let devices = facade.list_devices().await.map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(
            devices.into_iter().map(to_device_view).collect(),
        )))
    }
    .instrument(span)
    .await
}

/// DELETE /mobile-sync/devices/{device_id}
#[utoipa::path(
    delete,
    path = "/mobile-sync/devices/{device_id}",
    operation_id = "revokeMobileDevice",
    tag = "mobile-sync",
    params(("device_id" = String, Path, description = "Mobile device id")),
    responses(
        (status = 200, description = "Device revoked", body = MobileSyncActionEnvelope),
        (status = 404, description = "Device not found", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn revoke_mobile_device_handler(
    State(state): State<DaemonApiState>,
    Path(device_id): Path<String>,
) -> Result<Json<ApiEnvelope<MobileSyncActionResultDto>>, ApiError> {
    let span = info_span!("api.mobile_sync.revoke_device", device_id = %device_id);
    async move {
        let facade = mobile_sync_facade(&state)?;
        facade
            .revoke_device(RevokeMobileDeviceInput {
                device_id: MobileDeviceId::new(device_id),
            })
            .await
            .map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(MobileSyncActionResultDto {
            success: true,
        })))
    }
    .instrument(span)
    .await
}

/// POST /mobile-sync/devices/{device_id}/rotate-password
#[utoipa::path(
    post,
    path = "/mobile-sync/devices/{device_id}/rotate-password",
    operation_id = "rotateMobilePassword",
    tag = "mobile-sync",
    params(("device_id" = String, Path, description = "Mobile device id")),
    request_body = RotateMobilePasswordRequest,
    responses(
        (status = 200, description = "Password rotated (one-time echo)", body = RotateMobilePasswordEnvelope),
        (status = 404, description = "Device not found", body = ApiErrorResponse),
        (status = 422, description = "Invalid password", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn rotate_mobile_password_handler(
    State(state): State<DaemonApiState>,
    Path(device_id): Path<String>,
    Json(req): Json<RotateMobilePasswordRequest>,
) -> Result<Json<ApiEnvelope<RotateMobilePasswordResultDto>>, ApiError> {
    let span = info_span!(
        "api.mobile_sync.rotate_password",
        device_id = %device_id,
        custom_password = req.password.is_some(),
    );
    async move {
        let facade = mobile_sync_facade(&state)?;
        let out = facade
            .rotate_password(RotateMobilePasswordInput {
                device_id: MobileDeviceId::new(device_id),
                password: req.password,
            })
            .await
            .map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(to_rotate_dto(out))))
    }
    .instrument(span)
    .await
}

/// GET /mobile-sync/settings
#[utoipa::path(
    get,
    path = "/mobile-sync/settings",
    operation_id = "getMobileSyncSettings",
    tag = "mobile-sync",
    responses(
        (status = 200, description = "Mobile sync settings view", body = MobileSyncSettingsEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn get_mobile_sync_settings_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<MobileSyncSettingsViewDto>>, ApiError> {
    let span = info_span!("api.mobile_sync.get_settings");
    async move {
        let facade = mobile_sync_facade(&state)?;
        let view = facade.get_settings().await.map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(to_settings_view(view))))
    }
    .instrument(span)
    .await
}

/// PATCH /mobile-sync/settings
#[utoipa::path(
    patch,
    path = "/mobile-sync/settings",
    operation_id = "updateMobileSyncSettings",
    tag = "mobile-sync",
    request_body = UpdateMobileSyncSettingsRequest,
    responses(
        (status = 200, description = "Settings updated", body = UpdateMobileSyncSettingsEnvelope),
        (status = 422, description = "Invalid LAN parameter", body = ApiErrorResponse),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn update_mobile_sync_settings_handler(
    State(state): State<DaemonApiState>,
    Json(req): Json<UpdateMobileSyncSettingsRequest>,
) -> Result<Json<ApiEnvelope<UpdateMobileSyncSettingsResultDto>>, ApiError> {
    let span = info_span!("api.mobile_sync.update_settings");
    async move {
        let facade = mobile_sync_facade(&state)?;
        let out = facade
            .update_settings(UpdateMobileSyncSettingsInput {
                enabled: req.enabled,
                lan_listen_enabled: req.lan_listen_enabled,
                lan_advertise_ip: req.lan_advertise_ip,
                // Full base-URL override stays a CLI-only provisioning option
                // (`mobile-sync network set --url`); the GUI never touches it,
                // so `None` leaves a CLI-set override untouched.
                lan_advertise_base_url: None,
                lan_port: req.lan_port,
            })
            .await
            .map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(to_update_result(out))))
    }
    .instrument(span)
    .await
}

/// GET /mobile-sync/lan-interfaces
#[utoipa::path(
    get,
    path = "/mobile-sync/lan-interfaces",
    operation_id = "listMobileLanInterfaces",
    tag = "mobile-sync",
    responses(
        (status = 200, description = "Usable IPv4 LAN interfaces", body = LanInterfaceListEnvelope),
        (status = 500, description = "Internal server error", body = ApiErrorResponse),
        (status = 503, description = "Mobile sync facade unavailable", body = ApiErrorResponse),
    )
)]
async fn list_mobile_lan_interfaces_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<ApiEnvelope<Vec<LanInterfaceViewDto>>>, ApiError> {
    let span = info_span!("api.mobile_sync.list_lan_interfaces");
    async move {
        let facade = mobile_sync_facade(&state)?;
        let interfaces = facade.list_lan_interfaces().await.map_err(to_api)?;
        Ok(Json(ApiEnvelope::now(
            interfaces.into_iter().map(to_lan_interface_view).collect(),
        )))
    }
    .instrument(span)
    .await
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Every variant's `code` must be byte-identical to the FE-keyed token, the
    /// structured fields must land in `details`, the status must follow the
    /// severity split, and NO variant may map to 401 (callSdk retry trap).
    #[test]
    fn mobile_sync_error_maps_code_details_status() {
        let cases: Vec<(MobileSyncError, StatusCode, &str, Value)> = vec![
            (
                MobileSyncError::FacadeUnavailable,
                StatusCode::SERVICE_UNAVAILABLE,
                "FACADE_UNAVAILABLE",
                Value::Null,
            ),
            (
                MobileSyncError::LabelTooLong { max: 64 },
                StatusCode::UNPROCESSABLE_ENTITY,
                "LABEL_TOO_LONG",
                json!({ "max": 64 }),
            ),
            (
                MobileSyncError::UsernameTaken {
                    username: "alice".into(),
                },
                StatusCode::CONFLICT,
                "USERNAME_TAKEN",
                json!({ "username": "alice" }),
            ),
            (
                MobileSyncError::UsernameTooShort { min: 3, got: 1 },
                StatusCode::UNPROCESSABLE_ENTITY,
                "USERNAME_TOO_SHORT",
                json!({ "min": 3, "got": 1 }),
            ),
            (
                MobileSyncError::DeviceNotFound {
                    device_id: "did_x".into(),
                },
                StatusCode::NOT_FOUND,
                "DEVICE_NOT_FOUND",
                json!({ "deviceId": "did_x" }),
            ),
            (
                MobileSyncError::PersistenceFailed {
                    message: "disk full".into(),
                },
                StatusCode::INTERNAL_SERVER_ERROR,
                "PERSISTENCE_FAILED",
                json!({ "message": "disk full" }),
            ),
            (
                MobileSyncError::NoLanInterfaceAvailable,
                StatusCode::INTERNAL_SERVER_ERROR,
                "NO_LAN_INTERFACE_AVAILABLE",
                Value::Null,
            ),
        ];
        for (err, status, code, details) in cases {
            let api = ApiError::from(err);
            assert_ne!(
                api.status,
                StatusCode::UNAUTHORIZED,
                "{code} must not be 401"
            );
            assert_eq!(api.status, status, "status for {code}");
            assert_eq!(api.code, code);
            if details.is_null() {
                assert!(api.details.is_none(), "{code} should carry no details");
            } else {
                assert_eq!(
                    api.details.as_ref().unwrap(),
                    &details,
                    "details for {code}"
                );
            }
        }
    }

    /// `device_id` field must serialize camelCase (`deviceId`) on the wire so
    /// the FE union reads it back unchanged.
    #[test]
    fn device_not_found_details_is_camel_case() {
        let api = ApiError::from(MobileSyncError::DeviceNotFound {
            device_id: "did_abc".into(),
        });
        assert_eq!(api.details.unwrap()["deviceId"], "did_abc");
    }

    /// Facade error translation preserves the LABEL_MAX_LEN constant.
    #[test]
    fn label_too_long_translation_uses_constant_max() {
        let api = ApiError::from(MobileSyncError::from(
            RegisterMobileShortcutDeviceError::LabelTooLong,
        ));
        assert_eq!(api.details.unwrap()["max"], LABEL_MAX_LEN);
    }
}
