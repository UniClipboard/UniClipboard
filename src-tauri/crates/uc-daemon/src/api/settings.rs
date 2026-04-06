//! HTTP route handlers for settings endpoints.
//!
//! Provides read and write access to application settings.
//!
//! NOTE: Unlike the Tauri command (which applies OS-level side effects like
//! autostart registration and global shortcut updates), these handlers only
//! update the settings domain model — no autostart, no keyboard shortcuts.
use axum::extract::State;
use axum::routing::{get, put};
use axum::{Json, Router};
use uc_app::usecases::CoreUseCases;
use uc_core::settings::model::Settings;
use utoipa;

use crate::api::dto::error::ApiError;
use crate::api::dto::settings::{
    GetSettingsResponse, KeyboardShortcutsPatchDto, SettingsPatchDto, UpdateSettingsResponse,
};
use crate::api::server::DaemonApiState;

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
}

/// GET /settings
/// Returns the current application settings as a typed Settings struct.
#[utoipa::path(
    get,
    path =  "/settings",
    tag = "settings",
    responses(
        (status=200, body=GetSettingsResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> Result<Json<GetSettingsResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;
    let usecases = CoreUseCases::new(runtime.as_ref());

    let settings = usecases
        .get_settings()
        .execute()
        .await
        .map_err(|e| ApiError::internal(e.to_string()))?;

    Ok(Json(GetSettingsResponse {
        data: settings.into(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

/// PUT /settings
/// Updates application settings. Accepts a partial settings object and merges it
/// with the existing settings.
///
/// NOTE: Unlike the Tauri command, this handler does NOT apply OS-level side
/// effects (no autostart registration, no keyboard shortcut updates). It only
/// persists the settings domain model.
#[utoipa::path(
    put,
    path =  "/settings",
    tag = "settings",
    responses(
        (status=200, body=UpdateSettingsResponse),
        (status = 400, description = "Invalid request", body = crate::api::dto::error::ApiErrorResponse),
        (status = 500, description = "Internal server error", body = crate::api::dto::error::ApiErrorResponse)
    )
)]
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    Json(payload): Json<SettingsPatchDto>,
) -> Result<Json<UpdateSettingsResponse>, ApiError> {
    let runtime = state.runtime_or_error()?;

    let usecases = CoreUseCases::new(runtime.as_ref());

    // Load existing settings first
    let existing = usecases.get_settings().execute().await.map_err(|e| {
        tracing::error!(error = %e, "failed to load existing settings");
        ApiError::internal(e.to_string())
    })?;

    let merged = merge_settings_patch(existing, payload)?;

    usecases
        .update_settings()
        .execute(&merged)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to persist settings");
            ApiError::internal(e.to_string())
        })?;

    Ok(Json(UpdateSettingsResponse {
        success: true,
        data: merged.into(),
        ts: chrono::Utc::now().timestamp_millis(),
    }))
}

fn merge_settings_patch(
    mut existing: Settings,
    patch: SettingsPatchDto,
) -> Result<Settings, ApiError> {
    if let Some(general) = patch.general {
        if let Some(v) = general.auto_start {
            existing.general.auto_start = v;
        }
        if let Some(v) = general.silent_start {
            existing.general.silent_start = v;
        }
        if let Some(v) = general.auto_check_update {
            existing.general.auto_check_update = v;
        }
        if let Some(v) = general.theme {
            existing.general.theme = v.into();
        }
        if let Some(v) = general.theme_color {
            existing.general.theme_color = v;
        }
        if let Some(v) = general.language {
            existing.general.language = v;
        }
        if let Some(v) = general.device_name {
            existing.general.device_name = v;
        }
        if let Some(v) = general.update_channel {
            existing.general.update_channel = v.map(Into::into);
        }
        if let Some(v) = general.telemetry_enabled {
            existing.general.telemetry_enabled = v;
        }
    }

    if let Some(sync) = patch.sync {
        if let Some(v) = sync.auto_sync {
            existing.sync.auto_sync = v;
        }
        if let Some(v) = sync.sync_frequency {
            existing.sync.sync_frequency = v.into();
        }
        if let Some(content_types) = sync.content_types {
            if let Some(v) = content_types.text {
                existing.sync.content_types.text = v;
            }
            if let Some(v) = content_types.image {
                existing.sync.content_types.image = v;
            }
            if let Some(v) = content_types.link {
                existing.sync.content_types.link = v;
            }
            if let Some(v) = content_types.file {
                existing.sync.content_types.file = v;
            }
            if let Some(v) = content_types.code_snippet {
                existing.sync.content_types.code_snippet = v;
            }
            if let Some(v) = content_types.rich_text {
                existing.sync.content_types.rich_text = v;
            }
        }
    }

    // retention_policy merge
    if let Some(rp) = patch.retention_policy {
        if let Some(v) = rp.enabled {
            existing.retention_policy.enabled = v;
        }
        if let Some(v) = rp.skip_pinned {
            existing.retention_policy.skip_pinned = v;
        }
        if let Some(v) = rp.evaluation {
            existing.retention_policy.evaluation = v.into();
        }
        if let Some(rules) = rp.rules {
            existing.retention_policy.rules = rules
                .into_iter()
                .map(|r| match r {
                    super::dto::settings::RetentionRuleDto::ByAge { max_age } => {
                        uc_core::settings::model::RetentionRule::ByAge { max_age }
                    }
                    super::dto::settings::RetentionRuleDto::ByCount { max_items } => {
                        uc_core::settings::model::RetentionRule::ByCount { max_items }
                    }
                    super::dto::settings::RetentionRuleDto::ByContentType {
                        content_type,
                        max_age,
                    } => uc_core::settings::model::RetentionRule::ByContentType {
                        content_type: content_type.into(),
                        max_age,
                    },
                    super::dto::settings::RetentionRuleDto::ByTotalSize { max_bytes } => {
                        uc_core::settings::model::RetentionRule::ByTotalSize { max_bytes }
                    }
                    super::dto::settings::RetentionRuleDto::Sensitive { max_age } => {
                        uc_core::settings::model::RetentionRule::Sensitive { max_age }
                    }
                })
                .collect();
        }
    }

    // security merge
    if let Some(sec) = patch.security {
        if let Some(v) = sec.encryption_enabled {
            existing.security.encryption_enabled = v;
        }
        if let Some(v) = sec.auto_unlock_enabled {
            existing.security.auto_unlock_enabled = v;
        }
        // passphrase is handled separately by the caller (triggers unlock flow)
    }

    // pairing merge
    if let Some(pairing) = patch.pairing {
        if let Some(v) = pairing.step_timeout {
            existing.pairing.step_timeout = v;
        }
        if let Some(v) = pairing.user_verification_timeout {
            existing.pairing.user_verification_timeout = v;
        }
        if let Some(v) = pairing.session_timeout {
            existing.pairing.session_timeout = v;
        }
        if let Some(v) = pairing.max_retries {
            existing.pairing.max_retries = v;
        }
    }

    // keyboard_shortcuts: incremental merge — Some = upsert, None = delete
    if let Some(KeyboardShortcutsPatchDto { shortcuts }) = patch.keyboard_shortcuts {
        for (k, v) in shortcuts {
            match v {
                Some(key) => {
                    existing.keyboard_shortcuts.insert(k, key.into());
                }
                None => {
                    existing.keyboard_shortcuts.remove(&k);
                }
            }
        }
    }

    if let Some(file_sync) = patch.file_sync {
        if let Some(v) = file_sync.file_sync_enabled {
            existing.file_sync.file_sync_enabled = v;
        }
        if let Some(v) = file_sync.small_file_threshold {
            existing.file_sync.small_file_threshold = v;
        }
        if let Some(v) = file_sync.max_file_size {
            existing.file_sync.max_file_size = v;
        }
        if let Some(v) = file_sync.file_cache_quota_per_device {
            existing.file_sync.file_cache_quota_per_device = v;
        }
        if let Some(v) = file_sync.file_retention_hours {
            existing.file_sync.file_retention_hours = v;
        }
        if let Some(v) = file_sync.file_auto_cleanup {
            existing.file_sync.file_auto_cleanup = v;
        }
    }

    Ok(existing)
}
