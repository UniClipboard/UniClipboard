//! HTTP route handlers for settings endpoints.
//!
//! Provides read and write access to application settings.
//!
//! NOTE: Unlike the Tauri command (which applies OS-level side effects like
//! autostart registration and global shortcut updates), these handlers only
//! update the settings domain model — no autostart, no keyboard shortcuts.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, put};
use axum::{Json, Router};
use serde::Serialize;
use serde_json::json;
use serde_json::Value;
use uc_app::usecases::CoreUseCases;
use uc_core::settings::model::Settings;

use crate::api::routes::internal_error;
use crate::api::server::DaemonApiState;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsGetResponse {
    pub data: Settings,
    pub ts: i64,
}

pub fn router() -> Router<DaemonApiState> {
    Router::new()
        .route("/settings", get(get_settings_handler))
        .route("/settings", put(update_settings_handler))
}

/// GET /settings
/// Returns the current application settings as a typed Settings struct.
async fn get_settings_handler(
    State(state): State<DaemonApiState>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let usecases = CoreUseCases::new(runtime.as_ref());
    match usecases.get_settings().execute().await {
        Ok(settings) => {
            let ts = chrono::Utc::now().timestamp_millis();
            Json(json!({ "data": settings, "ts": ts })).into_response()
        }
        Err(e) => internal_error(anyhow::anyhow!("{}", e)).into_response(),
    }
}

/// PUT /settings
/// Updates application settings. Accepts a partial settings object and merges it
/// with the existing settings.
///
/// NOTE: Unlike the Tauri command, this handler does NOT apply OS-level side
/// effects (no autostart registration, no keyboard shortcut updates). It only
/// persists the settings domain model.
async fn update_settings_handler(
    State(state): State<DaemonApiState>,
    payload: Result<Json<serde_json::Value>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    let Some(runtime) = state.runtime.clone() else {
        return internal_error(anyhow::anyhow!("daemon runtime unavailable")).into_response();
    };

    let Json(payload) = match payload {
        Ok(p) => p,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "bad_request", "message": "malformed request body" } })),
            )
                .into_response();
        }
    };

    let usecases = CoreUseCases::new(runtime.as_ref());

    // Load existing settings first
    let existing = match usecases.get_settings().execute().await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(error = %e, "failed to load existing settings");
            return internal_error(anyhow::anyhow!("{}", e)).into_response();
        }
    };

    // Deep merge: convert existing settings to JSON, merge with incoming payload,
    // then parse back to Settings. This allows partial updates where only the
    // provided fields are changed.
    let existing_value = match serde_json::to_value(&existing) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "failed to serialize existing settings");
            return internal_error(anyhow::anyhow!("{}", e)).into_response();
        }
    };

    let merged_value = json_merge(existing_value, payload);

    let merged: Settings = match serde_json::from_value(merged_value) {
        Ok(m) => m,
        Err(e) => {
            tracing::error!(error = %e, "failed to parse merged settings");
            let msg = format!("invalid settings: {}", e);
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": { "code": "bad_request", "message": msg } })),
            )
                .into_response();
        }
    };

    // Persist the merged settings
    match usecases.update_settings().execute(merged).await {
        Ok(()) => {
            let ts = chrono::Utc::now().timestamp_millis();
            (StatusCode::OK, Json(json!({ "data": { "success": true }, "ts": ts }))).into_response()
        }
        Err(e) => internal_error(anyhow::anyhow!("{}", e)).into_response(),
    }
}

/// Deep-merges `overlay` into `base`, returning a new Value.
/// If both `base` and `overlay` are objects, their fields are merged recursively.
fn json_merge(base: Value, overlay: Value) -> Value {
    match (base, overlay) {
        (Value::Object(mut base_map), Value::Object(overlay_map)) => {
            for (key, overlay_val) in overlay_map {
                let merged = match base_map.remove(&key) {
                    Some(base_val) => json_merge(base_val, overlay_val),
                    None => overlay_val,
                };
                base_map.insert(key, merged);
            }
            Value::Object(base_map)
        }
        // Non-object overlay replaces base
        (_, overlay) => overlay,
    }
}
