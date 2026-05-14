pub mod autostart;
pub mod error;
pub mod mobile_sync;
pub mod quick_panel;
pub mod restart;
pub mod settings;
pub mod space_setup;
pub mod startup;
pub mod storage;
pub mod tray;
pub mod updater;

use tracing::Span;
use uc_platform::ports::observability::TraceMetadata;

/// Get the OS process ID of the Tauri application.
///
/// 获取 Tauri 应用的操作系统进程 ID。
#[tauri::command]
pub fn get_tauri_pid() -> u32 {
    std::process::id()
}

/// Get the stable local device identifier used for telemetry correlation.
#[tauri::command]
pub async fn get_device_id(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<String, CommandError> {
    Ok(runtime.device_id())
}

/// 暴露给 webview 的设备和应用元数据，用于补齐前端 Sentry scope。
///
/// 字段命名与后端 `ScopeContext` 一一对应（`device.id` / `device.role` /
/// `device.platform` / `app.version` / `app.channel`）。webview 侧的
/// `device.role` 默认仍是 `webview`，这里返回的 `device_role` 是 *Rust 主进程*
/// 的角色（`gui-host`），webview 把它打到 `device.host_role` 二级 tag 上,
/// 便于 Sentry 上同时看到两端。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceMeta {
    pub device_id: String,
    pub device_role: String,
    pub platform: String,
    pub app_version: String,
    pub app_channel: String,
}

/// Returns the full device + app meta to the webview.
///
/// 该命令是 PR1 跨设备可观测性改造的一部分:webview 启动后通过它一次性把
/// Sentry initialScope 补齐,让前后端事件在 Sentry 上能用同一组 tag(尤其是
/// `device.id`)互相关联。底层数据来自启动期一次性 resolve 的 [`uc_observability::ScopeContext`]。
#[tauri::command]
pub async fn get_device_meta(
    runtime: tauri::State<'_, std::sync::Arc<crate::bootstrap::TauriAppRuntime>>,
    _trace: Option<TraceMetadata>,
) -> Result<DeviceMeta, CommandError> {
    let scope = uc_observability::global_scope();
    Ok(DeviceMeta {
        device_id: runtime.device_id(),
        device_role: scope
            .map(|s| s.device_role)
            .unwrap_or("gui-host")
            .to_string(),
        platform: scope
            .map(|s| s.platform)
            .unwrap_or(std::env::consts::OS)
            .to_string(),
        app_version: scope
            .map(|s| s.app_version)
            .unwrap_or(env!("CARGO_PKG_VERSION"))
            .to_string(),
        app_channel: scope.map(|s| s.app_channel).unwrap_or("dev").to_string(),
    })
}

// Re-export commonly used types
pub use autostart::*;

pub use restart::*;
pub use startup::*;
pub use storage::*;
pub use updater::*;

pub use error::CommandError;

pub(crate) fn record_trace_fields(span: &Span, trace: &Option<TraceMetadata>) {
    if let Some(metadata) = trace.as_ref() {
        span.record("trace_id", tracing::field::display(&metadata.trace_id));
        span.record("trace_ts", metadata.timestamp);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn device_meta_serializes_with_camel_case_keys() {
        let meta = DeviceMeta {
            device_id: "device-a".to_string(),
            device_role: "gui-host".to_string(),
            platform: "macos".to_string(),
            app_version: "1.2.3".to_string(),
            app_channel: "dev".to_string(),
        };

        let json = serde_json::to_value(meta).expect("DeviceMeta serializes");

        assert_eq!(json["deviceId"], "device-a");
        assert_eq!(json["deviceRole"], "gui-host");
        assert_eq!(json["appVersion"], "1.2.3");
        assert_eq!(json["appChannel"], "dev");
        assert!(json.get("device_id").is_none());
        assert!(json.get("device_role").is_none());
        assert!(json.get("app_version").is_none());
        assert!(json.get("app_channel").is_none());
    }
}
