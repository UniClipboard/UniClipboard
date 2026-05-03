//! daemon 宿主入口。
//!
//! 提供两套接口：
//!
//! - [`run`]：同步阻塞入口，独立 daemon binary（`uniclipboard-daemon`）
//!   使用——内部创建专属 tokio runtime，监听 OS 信号到 main loop 自然退出。
//! - [`start_in_process`]：async 入口，GUI shell 在自己的 tokio runtime 里
//!   调用——启动 daemon main loop 作为 task，返回
//!   [`DaemonHandle`]，由 caller 显式 shutdown。
//!
//! 两个入口共用同一套装配 + main loop 实现（[`build_daemon_bootstrap_assembly`] /
//! [`run_daemon_main`]），只在"在哪个 runtime 上跑、谁触发 shutdown"上有差别。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;

use crate::daemon::app_assembly::{build_daemon_app_instance, DaemonAppAssemblyInput};
use crate::daemon::app_facade_assembly::{
    build_daemon_app_facade, DaemonAppFacadeAssembly, DaemonAppFacadeAssemblyInput,
};
use crate::daemon::background_tasks::spawn_daemon_background_tasks;
use crate::daemon::bootstrap::{build_daemon_bootstrap_assembly, DaemonBootstrapAssembly};
use crate::daemon::handle::DaemonHandle;
use crate::daemon::run_loop::{run_daemon_main, DaemonRunLoopInput};
use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::runtime_assembly::{build_daemon_runtime_workers, DaemonRuntimeAssemblyInput};
use crate::daemon::runtime_controls::build_daemon_runtime_controls;
use crate::daemon::search_assembly::build_daemon_search_assembly;
use crate::daemon::service_assembly::build_daemon_service_plan;
use crate::daemon::shutdown::spawn_stdin_eof_canceller;
use crate::daemon::tokio_runtime::build_daemon_tokio_runtime;

/// 独立 daemon binary 入口：创建专属 tokio runtime，启动 daemon，阻塞到退出。
///
/// 这条路径 GUI shell 不应再用——in-process 拉起请改走 [`start_in_process`]。
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let rt = build_daemon_tokio_runtime()?;
    rt.block_on(async move {
        let handle = start_in_process(run_mode).await?;
        // daemon main loop 自己监听 OS 信号（除 GuiInProcess 外），到信号
        // 触发后会自然退出。这里只 await join。
        handle.wait().await
    })
}

/// In-process daemon 启动入口（async）。
///
/// 假设 caller 已经在某个 tokio runtime 上下文中。完成装配后用
/// `tokio::spawn` 把 main loop 跑起来，返回 [`DaemonHandle`] 给 caller
/// 用于显式 shutdown。
///
/// `run_mode` 决定 daemon 内部行为：
/// - [`DaemonRunMode::GuiInProcess`]：daemon 不监听 OS 信号、不监听 stdin EOF——
///   shutdown 必须通过返回的 handle 触发，避免抢占 GUI 自己的信号 handler。
/// - 其他模式：daemon 内部仍监听 SIGTERM/SIGINT；`GuiSidecar` 还会
///   spawn 一个线程把 stdin EOF 转为 cancel 信号（旧 sidecar 模型）。
pub async fn start_in_process(run_mode: DaemonRunMode) -> anyhow::Result<DaemonHandle> {
    let cancel = CancellationToken::new();
    if run_mode.follows_gui_parent() {
        spawn_stdin_eof_canceller(cancel.clone());
    }

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
    } = build_daemon_bootstrap_assembly().await?;

    let uc_bootstrap::NonGuiBundle {
        deps,
        storage_paths,
        emitter_cell: _bundle_emitter_cell,
        lifecycle_status,
        task_registry,
        clipboard_integration_mode,
    } = non_gui_bundle;
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

    spawn_daemon_background_tasks(background, blob_ports, task_registry.clone());

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
        external_shutdown: Some(cancel.clone()),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        local_device_id,
        listens_to_os_signals: run_mode.listens_to_os_signals(),
    });

    let input = DaemonRunLoopInput {
        run_mode,
        daemon,
        app_facade,
        settings: settings_port,
        space_setup_assembly,
        deferred_ready_notify: runtime_controls.deferred_ready_notify,
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate,
    };
    let join = tokio::spawn(run_daemon_main(input));

    Ok(DaemonHandle::new(cancel, join))
}
