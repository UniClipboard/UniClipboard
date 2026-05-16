//! 桌面运行时句柄 —— GUI-framework agnostic。
//!
//! `DesktopRuntime` 持有 GUI shell 之间共享的进程级零碎件:
//!
//! - `Arc<AppFacade>` —— 业务调用唯一入口
//! - `task_registry` / `settings_port` / `storage_paths` / `device_id` ——
//!   进程基础设施
//!
//! 它**不持有**任何 GUI 框架特定的句柄(如 Tauri `AppHandle`、AppKit
//! `NSApplication`)。各 shell crate 可以包一层加上自己的句柄,例如
//! `uc-tauri::TauriAppRuntime`。

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::{
    AppFacade, AppPaths, FileTransferFacade, InMemoryLifecycleStatus, LifecycleStatusGateway,
};
use uc_bootstrap::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly, TaskRegistry,
};
use uc_core::ports::SettingsPort;

/// 桌面端 app runtime(GUI-framework agnostic)。
///
/// commands / 后台任务通过 `app_facade()` 触达业务;其它字段是进程级
/// 基础设施。host event 不在此持有 —— 应用层各 use case 直接持有
/// `Arc<HostEventBus>` (通过 `WiredDependencies::host_event_bus` 装入),
/// 桌面运行时不再做"emitter cell 中转"的二级代理。
pub struct DesktopRuntime {
    app_facade: Arc<AppFacade>,
    task_registry: Arc<TaskRegistry>,
    settings_port: Arc<dyn SettingsPort>,
    storage_paths: AppPaths,
    device_id: String,
}

impl DesktopRuntime {
    /// 装配 `AppFacade` + 收集进程级零碎件,产出 `DesktopRuntime`。
    ///
    /// `clipboard_write_coordinator` 是必填参数 —— `ClipboardRestoreFacade`
    /// 需要它,所以装 facade 时必须传入。`file_transfer_facade` 来自进程级
    /// 装配 (`WiredDependencies`),装进 `AppFacade.file_transfer`,GUI command
    /// 与 daemon 都通过同一份访问 file-transfer lifecycle。
    pub fn new(
        deps: AppDeps,
        storage_paths: AppPaths,
        clipboard_write_coordinator: Arc<
            uc_application::clipboard_write::ClipboardWriteCoordinator,
        >,
        file_transfer_facade: Arc<FileTransferFacade>,
    ) -> Self {
        let device_id = deps.device.device_identity.current_device_id().to_string();
        let settings_port = deps.settings.clone();

        let lifecycle_status: Arc<dyn LifecycleStatusGateway> =
            Arc::new(InMemoryLifecycleStatus::new());
        let task_registry = Arc::new(TaskRegistry::new());

        // Clipboard integration mode is resolved from the UC_CLIPBOARD_MODE env var.
        // Defaults to Full (standalone GUI watches clipboard directly).
        // Set UC_CLIPBOARD_MODE=passive when a daemon is running and handling
        // clipboard capture + broadcast via DaemonWsBridge.
        let clipboard_integration_mode = uc_bootstrap::resolve_clipboard_integration_mode();

        // Compose AppFacade —— 与 desktop daemon 入口共享同一装配函数。
        // GUI 端不直接做 space setup / member roster / search coordinator,
        // 这三处传 None;其它 facade 全部从 deps 拼齐。`file_transfer`
        // 进程级 facade 这里装入,daemon 启停不动它。
        let app_facade = build_app_facade_from_deps(
            &deps,
            &storage_paths,
            lifecycle_status,
            AppFacadeAssemblyOptions {
                clipboard_restore: Some(ClipboardRestoreAssembly {
                    write_coordinator: clipboard_write_coordinator,
                    integration_mode: clipboard_integration_mode,
                }),
                file_transfer: Some(file_transfer_facade),
                ..Default::default()
            },
        );

        Self {
            app_facade,
            task_registry,
            settings_port,
            storage_paths,
            device_id,
        }
    }

    /// 业务入口 —— commands / 后台任务通过它访问业务。
    pub fn app_facade(&self) -> &Arc<AppFacade> {
        &self.app_facade
    }

    /// Returns the current device ID for tracing spans and session context.
    pub fn device_id(&self) -> String {
        self.device_id.clone()
    }

    /// Returns a clone of the settings port for resolve_pairing_device_name and startup tasks.
    pub fn settings_port(&self) -> Arc<dyn SettingsPort> {
        self.settings_port.clone()
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
