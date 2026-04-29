//! daemon 宿主入口。

use std::sync::Arc;

use crate::daemon::app_assembly::{build_daemon_app_instance, DaemonAppAssemblyInput};
use crate::daemon::app_facade_assembly::{
    build_daemon_app_facade, DaemonAppFacadeAssembly, DaemonAppFacadeAssemblyInput,
};
use crate::daemon::background_tasks::spawn_daemon_background_tasks;
use crate::daemon::bootstrap::{build_daemon_bootstrap_assembly, DaemonBootstrapAssembly};
use crate::daemon::run_loop::{run_daemon_until_shutdown, DaemonRunLoopInput};
use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::runtime_assembly::{build_daemon_runtime_workers, DaemonRuntimeAssemblyInput};
use crate::daemon::runtime_controls::build_daemon_runtime_controls;
use crate::daemon::search_assembly::build_daemon_search_assembly;
use crate::daemon::service_assembly::build_daemon_service_plan;
use crate::daemon::shutdown::build_external_shutdown_token;
use crate::daemon::tokio_runtime::build_daemon_tokio_runtime;

/// 运行 daemon 进程。
///
/// 这里是桌面 daemon 模式的组装入口。
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let external_shutdown = build_external_shutdown_token(run_mode);

    let rt = build_daemon_tokio_runtime()?;

    let DaemonBootstrapAssembly {
        non_gui_bundle,
        background,
        blob_ports,
        file_cache_dir,
        file_transfer_lifecycle,
        clipboard_write_coordinator,
        emitter_cell,
        clipboard_sync_facade,
        blob_transfer_facade,
        space_setup_assembly,
    } = build_daemon_bootstrap_assembly(&rt)?;

    let uc_bootstrap::NonGuiBundle {
        deps,
        storage_paths,
        emitter_cell: _bundle_emitter_cell,
        lifecycle_status,
        task_registry,
        clipboard_integration_mode,
    } = non_gui_bundle;
    // 后续后台任务需要持有 settings，先 clone 出来，避免跨 await 借用 deps。
    let settings_port = deps.settings.clone();
    let runtime_controls = build_daemon_runtime_controls(run_mode);

    let runtime_workers = build_daemon_runtime_workers(DaemonRuntimeAssemblyInput {
        deps: &deps,
        event_tx: runtime_controls.event_tx.clone(),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        clipboard_sync_facade: clipboard_sync_facade.clone(),
        blob_transfer_facade: blob_transfer_facade.clone(),
        file_cache_dir: file_cache_dir.clone(),
        file_transfer_lifecycle,
        clipboard_write_coordinator: clipboard_write_coordinator.clone(),
        host_event_emitter: emitter_cell.clone(),
    })?;

    spawn_daemon_background_tasks(&rt, background, blob_ports, task_registry.clone());

    let search_assembly = build_daemon_search_assembly(&deps, runtime_controls.event_tx.clone());

    let service_plan = build_daemon_service_plan(
        run_mode,
        runtime_controls.encryption_unlocked,
        &runtime_workers,
        &search_assembly,
    );

    let storage_paths_for_daemon = storage_paths.clone();
    let DaemonAppFacadeAssembly {
        app_facade,
        local_device_id,
    } = build_daemon_app_facade(DaemonAppFacadeAssemblyInput {
        deps: &deps,
        storage_paths: &storage_paths_for_daemon,
        lifecycle_status: lifecycle_status.clone(),
        space_setup_assembly: &space_setup_assembly,
        clipboard_sync: clipboard_sync_facade.clone(),
        blob_transfer: blob_transfer_facade.clone(),
        clipboard_write_coordinator: clipboard_write_coordinator.clone(),
        clipboard_integration_mode,
        search_coordinator: Arc::clone(&search_assembly.coordinator),
    });
    let daemon = build_daemon_app_instance(DaemonAppAssemblyInput {
        service_plan,
        app_facade: Arc::clone(&app_facade),
        storage_paths: storage_paths_for_daemon,
        emitter_cell: emitter_cell.clone(),
        event_tx: runtime_controls.event_tx,
        encryption_unlocked: runtime_controls.encryption_unlocked,
        deferred_ready_notify: runtime_controls.deferred_ready_notify.clone(),
        external_shutdown,
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        local_device_id,
    });

    run_daemon_until_shutdown(
        &rt,
        DaemonRunLoopInput {
            run_mode,
            daemon,
            app_facade,
            settings: settings_port,
            space_setup_assembly,
            deferred_ready_notify: runtime_controls.deferred_ready_notify,
            clipboard_capture_gate: runtime_controls.clipboard_capture_gate,
        },
    )
}
