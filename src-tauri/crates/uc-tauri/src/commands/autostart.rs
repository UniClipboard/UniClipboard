//! Autostart-related Tauri commands
//! 开机自启动相关的 Tauri 命令
//!
//! These commands are simple wrappers around the tauri_plugin_autostart plugin.
//! 这些命令是 tauri_plugin_autostart 插件的简单包装器。

use crate::commands::record_trace_fields;
use std::fmt::Display;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt as _;
use tracing::{info_span, Instrument};
use uc_core::ports::observability::TraceMetadata;

fn format_autostart_error(prefix: &str, error: impl Display) -> String {
    format!("{}: {}", prefix, error)
}

/// Enable autostart (launch app on system login)
/// 启用开机自启动（系统登录时启动应用）
///
/// ## Architecture / 架构
///
/// This is a simple wrapper command that delegates to the tauri_plugin_autostart plugin.
/// No use case is needed as this is a platform plugin wrapper, not business logic.
/// 这是一个简单的包装命令，委托给 tauri_plugin_autostart 插件。
/// 不需要用例，因为这是平台插件包装器，而非业务逻辑。
#[tauri::command]
pub async fn enable_autostart(
    app_handle: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.autostart.enable",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let autostart_manager = app_handle.autolaunch();
        autostart_manager
            .enable()
            .map_err(|e| format_autostart_error("Failed to enable autostart", e))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Disable autostart
/// 禁用开机自启动
///
/// ## Architecture / 架构
///
/// This is a simple wrapper command that delegates to the tauri_plugin_autostart plugin.
/// No use case is needed as this is a platform plugin wrapper, not business logic.
/// 这是一个简单的包装命令，委托给 tauri_plugin_autostart 插件。
/// 不需要用例，因为这是平台插件包装器，而非业务逻辑。
#[tauri::command]
pub async fn disable_autostart(
    app_handle: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<(), String> {
    let span = info_span!(
        "command.autostart.disable",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let autostart_manager = app_handle.autolaunch();
        autostart_manager
            .disable()
            .map_err(|e| format_autostart_error("Failed to disable autostart", e))?;
        Ok(())
    }
    .instrument(span)
    .await
}

/// Check if autostart is enabled
/// 检查是否已启用开机自启动
///
/// ## Returns / 返回值
/// - `true` if autostart is enabled, `false` otherwise
///
/// ## Architecture / 架构
///
/// This is a simple wrapper command that delegates to the tauri_plugin_autostart plugin.
/// No use case is needed as this is a platform plugin wrapper, not business logic.
/// 这是一个简单的包装命令，委托给 tauri_plugin_autostart 插件。
/// 不需要用例，因为这是平台插件包装器，而非业务逻辑。
#[tauri::command]
pub async fn is_autostart_enabled(
    app_handle: AppHandle,
    _trace: Option<TraceMetadata>,
) -> Result<bool, String> {
    let span = info_span!(
        "command.autostart.is_enabled",
        trace_id = tracing::field::Empty,
        trace_ts = tracing::field::Empty,
    );
    record_trace_fields(&span, &_trace);
    async {
        let autostart_manager = app_handle.autolaunch();
        autostart_manager
            .is_enabled()
            .map_err(|e| format_autostart_error("Failed to check autostart status", e))
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::format_autostart_error;

    #[test]
    fn format_autostart_error_includes_prefix_and_error() {
        let message = format_autostart_error("Failed to enable autostart", "boom");
        assert_eq!(message, "Failed to enable autostart: boom");
    }
}
