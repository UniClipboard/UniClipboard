//! Standalone daemon process runtime — holds process-level infrastructure for
//! the `uniclipd` binary.
//!
//! This is the daemon's counterpart to `uc-desktop::DesktopRuntime`. It holds
//! the `AppFacade` and `TaskRegistry` that the standalone `run()` entry point
//! needs, without pulling in any GUI-shell concerns.
//!
//! Created during ADR-008 P2 (Slice 2b) to break the `run()` function's
//! dependency on `uc-desktop::DesktopRuntime`.

use std::sync::Arc;

use uc_application::deps::AppDeps;
use uc_application::facade::{AppFacade, AppPaths, FileTransferFacade, InMemoryLifecycleStatus};
use uc_bootstrap::{build_app_facade_from_deps, AppFacadeAssemblyOptions, TaskRegistry};

use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_bootstrap::ClipboardRestoreAssembly;

pub struct DaemonProcessRuntime {
    app_facade: Arc<AppFacade>,
    task_registry: Arc<TaskRegistry>,
}

impl DaemonProcessRuntime {
    pub fn new(
        deps: AppDeps,
        storage_paths: AppPaths,
        clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
        file_transfer_facade: Arc<FileTransferFacade>,
    ) -> Self {
        let task_registry = Arc::new(TaskRegistry::new());

        let lifecycle_status = Arc::new(InMemoryLifecycleStatus::new());
        let clipboard_integration_mode = uc_bootstrap::resolve_clipboard_integration_mode();

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
        }
    }

    pub fn app_facade(&self) -> &Arc<AppFacade> {
        &self.app_facade
    }

    pub fn task_registry(&self) -> &Arc<TaskRegistry> {
        &self.task_registry
    }
}
