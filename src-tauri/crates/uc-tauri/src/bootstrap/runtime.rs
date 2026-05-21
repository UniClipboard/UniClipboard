//! # Tauri 端运行时句柄
//!
//! `TauriAppRuntime` 是 [`uc_desktop::DesktopRuntime`] 在 Tauri shell
//! 上的包装：
//!
//! - 内部持有 `Arc<DesktopRuntime>`，所有 GUI-framework agnostic 字段
//!   （facade、task_registry、settings、storage、event_emitter、device_id）
//!   一律通过它访问；
//! - 额外持有 `Arc<RwLock<Option<tauri::AppHandle>>>` —— Tauri setup 完成
//!   后注入，用于事件发射 / 窗口控制等 Tauri 特有 API。
//!
//! ## 设计意图
//!
//! 把 GUI-framework agnostic 部分留在 `uc-desktop`，保证未来其它 shell
//! （如 `uc-macos-native`）能复用同一个 `DesktopRuntime`，只在最外层
//! 加上各自的窗口/句柄包装。`uc-tauri` 不该暴露任何 `tauri` 类型给上层
//! 业务代码——commands 通过 `runtime.desktop()` 拿到 `DesktopRuntime`，
//! 通过 `runtime.app_handle()` 才需要看到 Tauri 句柄。
//!
//! ## 用法示例
//!
//! ```rust,ignore
//! use uc_tauri::bootstrap::TauriAppRuntime;
//! use tauri::State;
//!
//! #[tauri::command]
//! async fn list_entries(
//!     runtime: State<'_, std::sync::Arc<TauriAppRuntime>>,
//! ) -> Result<(), String> {
//!     let facade = runtime.app_facade();
//!     let entries = facade
//!         .clipboard_history
//!         .list_entry_projections(/* input */)
//!         .await
//!         .map_err(|e| e.to_string())?;
//!     Ok(())
//! }
//! ```

use std::sync::{Arc, RwLock};

use uc_application::deps::AppDeps;
use uc_application::facade::{AppFacade, AppPaths, FileTransferFacade};
use uc_bootstrap::TaskRegistry;
use uc_core::ports::{SettingsPort, SetupStatusPort};
use uc_desktop::DesktopRuntime;
use uc_observability::analytics::AnalyticsPort;

/// Tauri 端的应用运行时句柄。
///
/// 包装 `Arc<DesktopRuntime>` + `Option<tauri::AppHandle>`。所有 GUI-framework
/// agnostic 的访问通过 `desktop()` 进入；Tauri 特有的事件发射/窗口控制
/// 通过 `app_handle()`。
pub struct TauriAppRuntime {
    desktop: Arc<DesktopRuntime>,
    /// Tauri AppHandle for event emission (set after Tauri setup).
    app_handle: Arc<RwLock<Option<tauri::AppHandle>>>,
}

impl TauriAppRuntime {
    /// 装配 `DesktopRuntime` + 在外层加一个空 `AppHandle`,产出
    /// `TauriAppRuntime`。host event 总线由 application 层(`WiredDependencies::
    /// host_event_bus`)直接持有,不再经过 runtime 中转。
    pub fn new(
        deps: AppDeps,
        storage_paths: AppPaths,
        clipboard_write_coordinator: Arc<
            uc_application::clipboard_write::ClipboardWriteCoordinator,
        >,
        file_transfer_facade: Arc<FileTransferFacade>,
    ) -> Self {
        Self::from_desktop(Arc::new(DesktopRuntime::new(
            deps,
            storage_paths,
            clipboard_write_coordinator,
            file_transfer_facade,
        )))
    }

    /// 已经有 `DesktopRuntime` 时直接包一层。
    pub fn from_desktop(desktop: Arc<DesktopRuntime>) -> Self {
        Self {
            desktop,
            app_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// 取底层 `DesktopRuntime`（GUI-framework agnostic）。新代码尽量用这个。
    pub fn desktop(&self) -> &Arc<DesktopRuntime> {
        &self.desktop
    }

    /// Set the Tauri AppHandle for event emission.
    /// This must be called after Tauri setup completes.
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        match self.app_handle.write() {
            Ok(mut guard) => {
                *guard = Some(handle);
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in set_app_handle, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                *guard = Some(handle);
            }
        }
    }

    /// Get a reference to the AppHandle, if available.
    pub fn app_handle(&self) -> std::sync::RwLockReadGuard<'_, Option<tauri::AppHandle>> {
        self.app_handle.read().unwrap_or_else(|poisoned| {
            tracing::error!("RwLock poisoned in app_handle, recovering from poisoned state");
            poisoned.into_inner()
        })
    }

    /// Returns a clone of the shared app_handle cell.
    pub fn app_handle_cell(&self) -> Arc<RwLock<Option<tauri::AppHandle>>> {
        self.app_handle.clone()
    }

    // ---------------------------------------------------------------------
    // 透传 DesktopRuntime 字段，保持历史 API 不破坏。
    // ---------------------------------------------------------------------

    /// 业务入口 —— commands / 后台任务通过它访问业务。
    pub fn app_facade(&self) -> &Arc<AppFacade> {
        self.desktop.app_facade()
    }

    pub fn device_id(&self) -> String {
        self.desktop.device_id()
    }

    pub fn settings_port(&self) -> Arc<dyn SettingsPort> {
        self.desktop.settings_port()
    }

    /// 产品 telemetry sink。Tauri command body / 后台任务直接
    /// `capture(Event::X)`，gate 由 `GatedAnalyticsSink` 守护。
    pub fn analytics(&self) -> Arc<dyn AnalyticsPort> {
        self.desktop.analytics()
    }

    /// `SetupStatus` 读写端口。`update_scheduler` 启动循环前 poll
    /// `has_completed`。
    pub fn setup_status_port(&self) -> Arc<dyn SetupStatusPort> {
        self.desktop.setup_status_port()
    }

    pub fn storage_paths(&self) -> &AppPaths {
        self.desktop.storage_paths()
    }

    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        self.desktop.task_registry()
    }
}
