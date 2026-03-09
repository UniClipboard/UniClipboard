//! Settings-related Tauri commands
//! 设置相关的 Tauri 命令

use crate::bootstrap::{resolve_pairing_device_name, AppRuntime};
use crate::commands::record_trace_fields;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;
use uc_core::settings::model::Settings;

/// Get application settings
/// 获取应用设置
///
/// Returns the complete application settings as JSON.
///
/// ## Returns / 返回值
/// - JSON representation of current Settings
#[tauri::command]
pub async fn get_settings(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Value, String> {
    let span = info_span!(
        "command.settings.get",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        device_id = %runtime.deps.device_identity.current_device_id(),
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().get_settings();
        let settings = uc.execute().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get settings");
            e.to_string()
        })?;

        // Convert Settings to JSON value
        let json_value = serde_json::to_value(&settings).map_err(|e| {
            tracing::error!(error = %e, "Failed to serialize settings");
            format!("Failed to serialize settings: {}", e)
        })?;

        // DIAGNOSTIC: Log device_name presence and length without exposing raw value (privacy)
        let device_name = json_value
            .get("general")
            .and_then(|g| g.get("device_name"))
            .and_then(|v| v.as_str());
        tracing::info!(
            device_name_present = device_name.is_some(),
            device_name_len = device_name.map(|s| s.len()),
            "Retrieved settings successfully"
        );
        Ok(json_value)
    }
    .instrument(span)
    .await
}

/// Update application settings
/// 更新应用设置
///
/// Updates application settings from JSON.
///
/// ## Parameters / 参数
/// - `settings`: JSON value containing settings to update
#[tauri::command]
pub async fn update_settings(
    runtime: State<'_, Arc<AppRuntime>>,
    settings: Value,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.settings.update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
        device_id = %runtime.deps.device_identity.current_device_id(),
    );
    record_trace_fields(&span, &_trace);
    async {
        // Parse JSON into Settings domain model
        let parsed_settings: Settings = serde_json::from_value(settings.clone()).map_err(|e| {
            tracing::error!(error = %e, "Failed to parse settings JSON");
            format!("Failed to parse settings: {}", e)
        })?;

        let old_settings = runtime
            .usecases()
            .get_settings()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to load existing settings");
                e.to_string()
            })?;
        let device_name_changed =
            old_settings.general.device_name != parsed_settings.general.device_name;

        let uc = runtime.usecases().update_settings();
        uc.execute(parsed_settings).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to update settings");
            e.to_string()
        })?;

        if device_name_changed {
            let device_name = resolve_pairing_device_name(runtime.deps.settings.clone()).await;
            let uc = runtime.usecases().announce_device_name();
            uc.execute(device_name).await.map_err(|e| {
                tracing::error!(error = %e, "Failed to announce device name after settings update");
                e.to_string()
            })?;
        }

        tracing::info!("Settings updated successfully");
        Ok(())
    }
    .instrument(span)
    .await
}
