//! 桌面运行时句柄 —— GUI-framework agnostic。
//!
//! `DesktopRuntime` 持有 GUI shell 之间共享的进程级零碎件：
//!
//! - `Arc<AppFacade>` —— 业务调用唯一入口
//! - `task_registry` / `settings_port` / `storage_paths` / `event_emitter_cell` /
//!   `device_id` —— 进程基础设施
//!
//! 它**不持有**任何 GUI 框架特定的句柄（如 Tauri `AppHandle`、AppKit
//! `NSApplication`）。各 shell crate 可以包一层加上自己的句柄，例如
//! `uc-tauri::TauriAppRuntime`。

use std::sync::{Arc, RwLock};

use uc_application::deps::AppDeps;
use uc_application::facade::{
    AppFacade, AppPaths, HostEventEmitterPort, InMemoryLifecycleStatus, LifecycleStatusGateway,
};
use uc_bootstrap::{
    build_app_facade_from_deps, AppFacadeAssemblyOptions, ClipboardRestoreAssembly, TaskRegistry,
};
use uc_core::ports::SettingsPort;

/// 桌面端 app runtime（GUI-framework agnostic）。
///
/// commands / 后台任务通过 `app_facade()` 触达业务；其它字段是进程级
/// 基础设施。
pub struct DesktopRuntime {
    app_facade: Arc<AppFacade>,
    task_registry: Arc<TaskRegistry>,
    settings_port: Arc<dyn SettingsPort>,
    storage_paths: AppPaths,
    /// Shared emitter cell —— bootstrap 期可 swap（例如从
    /// `LoggingHostEventEmitter` 切到 daemon API emitter）。
    event_emitter_cell: Arc<RwLock<Arc<dyn HostEventEmitterPort>>>,
    device_id: String,
}

impl DesktopRuntime {
    /// 默认 emitter (`LoggingHostEventEmitter`)。其它情况调用 `with_setup`。
    pub fn new(
        deps: AppDeps,
        storage_paths: AppPaths,
        clipboard_write_coordinator: Arc<
            uc_application::clipboard_write::ClipboardWriteCoordinator,
        >,
    ) -> Self {
        let event_emitter: Arc<dyn HostEventEmitterPort> =
            Arc::new(uc_bootstrap::LoggingHostEventEmitter);
        Self::with_setup(
            deps,
            storage_paths,
            event_emitter,
            clipboard_write_coordinator,
        )
    }

    /// 装配 `AppFacade` + 收集进程级零碎件，产出 `DesktopRuntime`。
    ///
    /// `clipboard_write_coordinator` 是必填参数 —— `ClipboardRestoreFacade`
    /// 需要它，所以装 facade 时必须传入。
    pub fn with_setup(
        deps: AppDeps,
        storage_paths: AppPaths,
        event_emitter: Arc<dyn HostEventEmitterPort>,
        clipboard_write_coordinator: Arc<
            uc_application::clipboard_write::ClipboardWriteCoordinator,
        >,
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

        let event_emitter_cell = Arc::new(RwLock::new(event_emitter));

        // Compose AppFacade — 与 desktop daemon 入口共享同一装配函数。
        // GUI 端不直接做 space setup / member roster / search coordinator，
        // 这三处传 None；其它 facade 全部从 deps 拼齐。
        let app_facade = build_app_facade_from_deps(
            &deps,
            &storage_paths,
            lifecycle_status,
            AppFacadeAssemblyOptions {
                clipboard_restore: Some(ClipboardRestoreAssembly {
                    write_coordinator: clipboard_write_coordinator,
                    integration_mode: clipboard_integration_mode,
                }),
                ..Default::default()
            },
        );

        Self {
            app_facade,
            task_registry,
            settings_port,
            storage_paths,
            event_emitter_cell,
            device_id,
        }
    }

    /// 业务入口 —— commands / 后台任务通过它访问业务。
    pub fn app_facade(&self) -> &Arc<AppFacade> {
        &self.app_facade
    }

    /// Get the current event emitter (clones the inner Arc).
    pub fn event_emitter(&self) -> Arc<dyn HostEventEmitterPort> {
        match self.event_emitter_cell.read() {
            Ok(guard) => Arc::clone(&*guard),
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DesktopRuntime::event_emitter, recovering from poisoned state"
                );
                Arc::clone(&*poisoned.into_inner())
            }
        }
    }

    /// Swap the event emitter. Called from daemon setup to replace the
    /// initial `LoggingHostEventEmitter` with a daemon API emitter.
    pub fn set_event_emitter(&self, emitter: Arc<dyn HostEventEmitterPort>) {
        match self.event_emitter_cell.write() {
            Ok(mut guard) => {
                *guard = emitter;
            }
            Err(poisoned) => {
                tracing::error!(
                    "RwLock poisoned in DesktopRuntime::set_event_emitter, recovering from poisoned state"
                );
                let mut guard = poisoned.into_inner();
                *guard = emitter;
            }
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

    /// Returns the storage paths bundle (db / vault / cache / logs / app data root).
    pub fn storage_paths(&self) -> &AppPaths {
        &self.storage_paths
    }

    /// Returns a reference to the task registry for lifecycle management.
    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        &self.task_registry
    }
}
