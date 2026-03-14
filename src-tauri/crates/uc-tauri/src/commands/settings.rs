//! Settings-related Tauri commands
//! 设置相关的 Tauri 命令

use crate::bootstrap::{resolve_pairing_device_name, AppRuntime};
use crate::commands::error::CommandError;
use crate::commands::record_trace_fields;
use crate::events::{forward_setting_changed_event, SettingChangedEvent};
use serde_json::Value;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;
use tracing::{info_span, Instrument};
use uc_core::settings::model::Settings;
use uc_platform::ports::observability::TraceMetadata;

/// Get application settings
/// 获取应用设置
///
/// Returns the complete application settings as a typed Settings struct.
///
/// ## Returns / 返回值
/// - Typed Settings struct (serialized to JSON by Tauri)
#[tauri::command]
pub async fn get_settings(
    runtime: State<'_, Arc<AppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<Settings, CommandError> {
    let span = info_span!(
        "command.settings.get",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let uc = runtime.usecases().get_settings();
        let settings = uc.execute().await.map_err(|e| {
            tracing::error!(error = %e, "Failed to get settings");
            CommandError::InternalError(e.to_string())
        })?;

        tracing::info!(
            device_name_present = settings.general.device_name.is_some(),
            device_name_len = settings.general.device_name.as_deref().map(|s| s.len()),
            "Retrieved settings successfully"
        );
        Ok(settings)
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
    app_handle: tauri::AppHandle,
    runtime: State<'_, Arc<AppRuntime>>,
    settings: Value,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.settings.update",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        // Parse JSON into Settings domain model
        let parsed_settings: Settings = serde_json::from_value(settings.clone()).map_err(|e| {
            tracing::error!(error = %e, "Failed to parse settings JSON");
            CommandError::ValidationError(format!("Failed to parse settings: {}", e))
        })?;

        let old_settings = runtime
            .usecases()
            .get_settings()
            .execute()
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "Failed to load existing settings");
                CommandError::InternalError(e.to_string())
            })?;
        let device_name_changed =
            old_settings.general.device_name != parsed_settings.general.device_name;
        let auto_start_changed =
            old_settings.general.auto_start != parsed_settings.general.auto_start;
        let quick_panel_shortcut_changed = {
            let old_val = old_settings
                .keyboard_shortcuts
                .get(crate::quick_panel::SHORTCUT_SETTINGS_KEY);
            let new_val = parsed_settings
                .keyboard_shortcuts
                .get(crate::quick_panel::SHORTCUT_SETTINGS_KEY);
            old_val != new_val
        };

        let uc = runtime.usecases().update_settings();
        uc.execute(parsed_settings.clone()).await.map_err(|e| {
            tracing::error!(error = %e, "Failed to update settings");
            CommandError::InternalError(e.to_string())
        })?;

        // Apply OS-level autostart when auto_start setting changes
        if auto_start_changed {
            match runtime.usecases().apply_autostart() {
                Some(uc) => {
                    if let Err(e) = uc.execute(parsed_settings.general.auto_start) {
                        tracing::error!(error = %e, "Failed to apply OS autostart setting");
                        // Rollback: restore old settings so backend stays consistent with OS state
                        let rollback_uc = runtime.usecases().update_settings();
                        if let Err(rb_err) = rollback_uc.execute(old_settings).await {
                            tracing::error!(error = %rb_err, "Failed to rollback settings after autostart failure");
                        }
                        return Err(CommandError::InternalError(format!(
                            "Failed to apply autostart: {}",
                            e
                        )));
                    }
                }
                None => {
                    tracing::warn!("AppHandle not available, cannot apply autostart setting");
                    // Rollback: restore old settings so backend stays consistent with OS state
                    let rollback_uc = runtime.usecases().update_settings();
                    if let Err(rb_err) = rollback_uc.execute(old_settings).await {
                        tracing::error!(error = %rb_err, "Failed to rollback settings after autostart failure");
                    }
                    return Err(CommandError::InternalError(
                        "AppHandle not available, cannot apply autostart setting".to_string(),
                    ));
                }
            }
        }

        if device_name_changed {
            let device_name = resolve_pairing_device_name(runtime.settings_port()).await;
            let uc = runtime.usecases().announce_device_name();
            uc.execute(device_name).await.map_err(|e| {
                tracing::error!(error = %e, "Failed to announce device name after settings update");
                CommandError::InternalError(e.to_string())
            })?;
        }

        // Re-register global shortcut when quick panel shortcut changes
        if quick_panel_shortcut_changed {
            let old_shortcuts = crate::quick_panel::resolve_shortcut_from_settings(&old_settings);
            let new_shortcuts =
                crate::quick_panel::resolve_shortcut_from_settings(&parsed_settings);
            tracing::info!(
                old = ?old_shortcuts,
                new = ?new_shortcuts,
                "Quick panel shortcut changed, re-registering"
            );
            if let Err(e) =
                crate::quick_panel::update_global_shortcut(&app_handle, &old_shortcuts, &new_shortcuts)
            {
                tracing::error!(error = %e, "Failed to update global shortcut");
                // Rollback: restore old settings so persisted state matches actual registered shortcut
                let rollback_uc = runtime.usecases().update_settings();
                if let Err(rb_err) = rollback_uc.execute(old_settings).await {
                    tracing::error!(error = %rb_err, "Failed to rollback settings after shortcut update failure");
                }
                return Err(CommandError::InternalError(format!(
                    "Failed to update shortcut: {}",
                    e
                )));
            }
        }

        // Broadcast setting-changed event to all windows (quick panel, preview panel, etc.)
        let setting_json = serde_json::to_string(&parsed_settings).unwrap_or_default();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if let Err(e) = forward_setting_changed_event(
            &app_handle,
            SettingChangedEvent {
                setting_json,
                timestamp,
            },
        ) {
            tracing::warn!(error = %e, "Failed to broadcast setting-changed event");
        }

        tracing::info!("Settings updated successfully");
        Ok(())
    }
    .instrument(span)
    .await
}
