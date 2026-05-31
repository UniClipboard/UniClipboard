//! Autostart-related Tauri commands
//! 开机自启动相关的 Tauri 命令
//!
//! Single source of truth for the "launch at login" toggle. The preference
//! (`auto_start`) is persisted through the settings facade, and the OS-level
//! launch registration is applied through [`AutostartPort`] in the same
//! command so the stored value and the actual OS state never diverge.
//!
//! 开机自启动开关的单一真相源。偏好（`auto_start`）经设置 facade 持久化，
//! OS 级别的启动项注册在同一命令内通过 [`AutostartPort`] 应用，确保存储值
//! 与实际 OS 状态不会分裂。

use std::sync::Arc;

use tauri::{AppHandle, State};
use tracing::{info_span, Instrument};
use uc_application::facade::settings::{GeneralSettingsPatch, SettingsPatch};
use uc_platform::ports::observability::TraceMetadata;

use crate::adapters::autostart::{reconcile_autostart, TauriAutostart};
use crate::bootstrap::TauriAppRuntime;
use crate::commands::{record_trace_fields, CommandError};

/// Update the "launch at login" preference and apply it to the OS.
///
/// Persists `auto_start` through the settings facade first (the stored
/// preference is the source of truth), then reconciles the OS-level launch
/// registration via [`AutostartPort`]. If the OS step fails, the persisted
/// setting is rolled back so settings never claim a state the OS rejected.
///
/// 更新开机自启动偏好并应用到操作系统：先经设置 facade 持久化（存储偏好为
/// 真相源），再通过 [`AutostartPort`] 对齐 OS 启动项；OS 步骤失败时回滚已
/// 持久化的设置，避免设置声称一个 OS 未能达成的状态。
#[tauri::command]
#[specta::specta]
pub async fn update_autostart(
    app: AppHandle,
    runtime: State<'_, Arc<TauriAppRuntime>>,
    enabled: bool,
    _trace: Option<TraceMetadata>,
) -> Result<(), CommandError> {
    let span = info_span!(
        "command.autostart.update",
        enabled,
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);

    async move {
        let facade = runtime.app_facade();
        let previous = facade
            .settings
            .get()
            .await
            .map_err(CommandError::internal)?
            .general
            .auto_start;

        // Persist first, mirroring `update_keyboard_shortcuts`: the stored
        // preference is the source of truth and the OS registration is the
        // side effect that must follow it.
        facade
            .settings
            .update(general_auto_start_patch(enabled))
            .await
            .map_err(CommandError::internal)?;

        // Apply the OS-level launch registration through the platform port.
        let port = TauriAutostart::new(app.clone());
        if let Err(os_err) = reconcile_autostart(&port, enabled) {
            // Roll back so the persisted preference never diverges from the
            // OS state we failed to reach.
            if let Err(rollback_err) = facade
                .settings
                .update(general_auto_start_patch(previous))
                .await
            {
                tracing::error!(
                    error = %rollback_err,
                    "Failed to roll back auto_start setting after OS autostart failure"
                );
            }
            return Err(CommandError::internal(format!(
                "Failed to apply OS autostart: {os_err}"
            )));
        }

        Ok(())
    }
    .instrument(span)
    .await
}

/// Build a settings patch that touches only the `auto_start` general field.
fn general_auto_start_patch(enabled: bool) -> SettingsPatch {
    SettingsPatch {
        general: Some(GeneralSettingsPatch {
            auto_start: Some(enabled),
            ..Default::default()
        }),
        ..Default::default()
    }
}
