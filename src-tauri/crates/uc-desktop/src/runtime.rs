//! 桌面运行时句柄 —— GUI-framework agnostic。
//!
//! `DesktopRuntime` 持有 GUI shell 之间共享的进程级零碎件:
//!
//! - `task_registry` / `settings_port` / `setup_status` / `analytics` /
//!   `storage_paths` / `device_id` —— 进程基础设施
//!
//! ADR-008 P3-3 (B2'-3): GUI 已是外部 `uniclipd` 的纯客户端,不再持有
//! 进程内 `AppFacade`。所有业务调用走 daemon HTTP/WS (`uc-daemon-client`);
//! 此 runtime 只保留 file-backed / in-memory 的进程基础设施 (settings /
//! setup-status / analytics / device-id),全部由 [`uc_bootstrap::GuiClientDeps`]
//! 装配,**不打开 sqlite**。
//!
//! 它**不持有**任何 GUI 框架特定的句柄(如 Tauri `AppHandle`、AppKit
//! `NSApplication`)。各 shell crate 可以包一层加上自己的句柄,例如
//! `uc-tauri::TauriAppRuntime`。

use std::sync::Arc;

use uc_application::facade::AppPaths;
use uc_bootstrap::{GuiClientDeps, TaskRegistry};
use uc_core::ports::{SettingsPort, SetupStatusPort};
use uc_observability::analytics::AnalyticsPort;

/// 桌面端 app runtime(GUI-framework agnostic,纯客户端)。
///
/// 进程基础设施的收口点。业务不再经此 —— commands / 后台任务通过
/// `uc-daemon-client` 直连外部 daemon。host event 由 daemon WS 推送
/// (`DaemonWsBridge`),不在此持有 emitter / bus。
pub struct DesktopRuntime {
    task_registry: Arc<TaskRegistry>,
    settings_port: Arc<dyn SettingsPort>,
    /// 产品 telemetry sink（已包 `GatedAnalyticsSink` 一层），shell 与后台
    /// 任务直接 `capture(Event::X)`，不必自己查 gate。
    analytics: Arc<dyn AnalyticsPort>,
    /// `SetupStatus` 读写端口（file-backed）。后台任务（如 update scheduler）
    /// 需要在启动循环前等 `has_completed == true`。
    setup_status: Arc<dyn SetupStatusPort>,
    storage_paths: AppPaths,
    device_id: String,
}

impl DesktopRuntime {
    /// 从 [`GuiClientDeps`] 收集进程级零碎件,产出纯客户端 `DesktopRuntime`。
    ///
    /// 不装配 `AppFacade`、不打开 sqlite —— 全部 sqlite-backed 状态归外部
    /// daemon。`GuiClientDeps` 只携带 file-backed / in-memory 端口。
    pub fn new(client: GuiClientDeps) -> Self {
        Self {
            task_registry: Arc::new(TaskRegistry::new()),
            settings_port: client.settings,
            analytics: client.analytics,
            setup_status: client.setup_status,
            storage_paths: client.storage_paths,
            device_id: client.device_id,
        }
    }

    /// Returns the current device ID for tracing spans and session context.
    pub fn device_id(&self) -> String {
        self.device_id.clone()
    }

    /// Returns a clone of the settings port for resolve_pairing_device_name and startup tasks.
    pub fn settings_port(&self) -> Arc<dyn SettingsPort> {
        self.settings_port.clone()
    }

    /// 产品 telemetry sink。shell crate / 后台任务直接 `capture(Event::X)`，
    /// gate 由 `GatedAnalyticsSink` 在内部守护，不必上层判断
    /// `usage_analytics_enabled`。
    pub fn analytics(&self) -> Arc<dyn AnalyticsPort> {
        self.analytics.clone()
    }

    /// `SetupStatus` 读写端口。后台 scheduler 在启动循环前 poll
    /// `get_status().has_completed`，setup 期间不打扰用户。
    pub fn setup_status_port(&self) -> Arc<dyn SetupStatusPort> {
        self.setup_status.clone()
    }

    /// Returns the storage paths bundle (db / vault / cache / logs / app data root).
    pub fn storage_paths(&self) -> &AppPaths {
        &self.storage_paths
    }

    /// Returns a reference to the task registry for lifecycle management.
    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        &self.task_registry
    }
}
