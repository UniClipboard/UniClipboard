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
//!
//! # Phase 4 重构(2026-05-10)
//!
//! `start_in_process` 现在接受调用方已构造好的 `Arc<AppFacade>`(进程内
//! 单例),daemon 启停时 swap 5 个 daemon-lifecycle 子 facade 到这份
//! `AppFacade`,而不再装第二份完整 `AppFacade`。daemon 仍然 wire 第二份
//! deps —— 数据走 sqlite WAL 双 pool 兼容,daemon reload 后通过 swap 让
//! GUI command 看到新的 lifecycle facades。`WireOverrides` 仍然保留:
//! GUI 端与 daemon 端共享 `mobile_sync_endpoint_info` Arc 的机制不变;
//! 后续 PR 会把 daemon wire 也合并进进程级 deps。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uc_application::facade::AppFacade;
use uc_bootstrap::WireOverrides;

use crate::daemon::app_assembly::{build_daemon_app_instance, DaemonAppAssemblyInput};
use crate::daemon::app_facade_assembly::{
    build_daemon_lifecycle_facades, DaemonLifecycleFacadesInput,
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
use crate::daemon::tokio_runtime::build_daemon_tokio_runtime;

/// 独立 daemon binary 入口：创建专属 tokio runtime,启动 daemon,阻塞到退出。
///
/// 这条路径 GUI shell 不应再用——in-process 拉起请改走 [`start_in_process`]。
///
/// standalone binary 没有 GUI shell 持有的 `Arc<AppFacade>`,所以入口内部
/// 调 `build_process_runtime` 装一份进程级 deps + facade,然后跑标准
/// daemon-lifecycle。
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let rt = build_daemon_tokio_runtime()?;
    rt.block_on(async move {
        // standalone 自己 wire 一次进程级 deps + facade。这份 facade 不被
        // 暴露给任何外部 caller (没有 GUI shell),只活在 daemon 进程生命周期
        // 内 —— daemon main loop 退出 binary 整个 exit。
        let ctx = crate::bootstrap::build_process_runtime()?;

        let event_emitter: Arc<dyn uc_application::facade::HostEventEmitterPort> =
            Arc::new(uc_bootstrap::LoggingHostEventEmitter);
        let runtime = crate::DesktopRuntime::with_setup(
            ctx.deps,
            ctx.storage_paths,
            event_emitter,
            ctx.background.clipboard_write_coordinator.clone(),
        );
        let app_facade = Arc::clone(runtime.app_facade());
        // standalone 没有 GUI shell 来 .manage 这份 endpoint_info —— 直接让
        // daemon 端 wire 内部 new 一份 (走 default WireOverrides)。
        let wire_overrides = WireOverrides::default();
        let handle = start_in_process(run_mode, app_facade, wire_overrides).await?;
        // runtime 必须活到 daemon 退出 —— move 进 await 内部维持生命周期。
        // daemon main loop 自己监听 OS 信号(除 GuiInProcess 外),信号触发后
        // 自然退出;handle.wait() 返回意味 daemon 已停。
        let result = handle.wait().await;
        drop(runtime);
        result
    })
}

/// In-process daemon 启动入口（async）。
///
/// 假设 caller 已经在某个 tokio runtime 上下文中。完成装配后用
/// `tokio::spawn` 把 main loop 跑起来,返回 [`DaemonHandle`] 给 caller
/// 用于显式 shutdown。
///
/// # 参数
///
/// - `run_mode` 决定 daemon 内部行为:
///   - [`DaemonRunMode::GuiInProcess`]:daemon 不监听 OS 信号——shutdown 必须
///     通过返回的 handle 触发,避免抢占 GUI 自己的信号 handler。
///   - [`DaemonRunMode::Standalone`]:daemon 内部监听 SIGTERM/SIGINT,靠 OS
///     信号自然退出。
///
/// - `app_facade` 进程级单例 `AppFacade`。GUI shell `build_process_runtime` 时已装好,
///   daemon 启动 swap 5 个 daemon-lifecycle 子 facade(space_setup /
///   member_roster / clipboard_sync / blob_transfer / mobile_sync) 进去,
///   daemon 退出时清空。整个进程只有这一份 `AppFacade`。
///
/// - `wire_overrides` 让 caller 在 wire 之前注入预先建好的共享 Arc(典型:
///   GUI shell 端的 `mobile_sync_endpoint_info` Arc)。
pub async fn start_in_process(
    run_mode: DaemonRunMode,
    app_facade: Arc<AppFacade>,
    wire_overrides: WireOverrides,
) -> anyhow::Result<DaemonHandle> {
    let cancel = CancellationToken::new();

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
        mobile_sync_endpoint_info,
    } = build_daemon_bootstrap_assembly(wire_overrides).await?;

    let uc_bootstrap::NonGuiBundle {
        deps,
        storage_paths,
        emitter_cell: _bundle_emitter_cell,
        lifecycle_status: _lifecycle_status,
        task_registry,
        clipboard_integration_mode: _clipboard_integration_mode,
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

    // Phase 4 重构:不再装第二份 `AppFacade`,改为构造 5 个 daemon-lifecycle
    // 子 facade 然后 swap 进 GUI shell 已装好的进程级 AppFacade。
    let (lifecycle_facades, local_device_id) =
        build_daemon_lifecycle_facades(DaemonLifecycleFacadesInput {
            deps: &deps,
            storage_paths: &storage_paths_for_daemon,
            space_setup_assembly: &space_setup_assembly,
            clipboard_sync: clipboard_sync_facade.clone(),
            blob_transfer: blob_transfer_facade.clone(),
            mobile_sync_apply_inbound: runtime_workers.apply_inbound.clone(),
        });

    app_facade.swap_daemon_lifecycle(lifecycle_facades);

    // search_coordinator 是 daemon-lifecycle 的(绑 daemon search assembly),
    // 进程级 SearchFacade 内部 coordinator 字段在 GUI 启动期为 None。daemon
    // 启动时通过 SearchFacade::set_coordinator 装入,daemon 退出时 SearchFacade
    // 持有的 Arc 仍然存在但 daemon 资源已清,后续 swap 也可。
    app_facade
        .search
        .set_coordinator(Arc::clone(&search_assembly.coordinator));

    let app_facade_for_daemon = Arc::clone(&app_facade);
    let daemon = build_daemon_app_instance(DaemonAppAssemblyInput {
        service_plan,
        app_facade: Arc::clone(&app_facade_for_daemon),
        storage_paths: storage_paths_for_daemon,
        emitter_cell: emitter_cell.clone(),
        event_tx: runtime_controls.event_tx,
        encryption_unlocked: runtime_controls.encryption_unlocked,
        deferred_ready_notify: runtime_controls.deferred_ready_notify.clone(),
        external_shutdown: Some(cancel.clone()),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        local_device_id,
        listens_to_os_signals: run_mode.listens_to_os_signals(),
        process_mode: run_mode.process_mode(),
        mobile_sync_endpoint_info,
    });

    let app_facade_for_cleanup = Arc::clone(&app_facade);
    let input = DaemonRunLoopInput {
        run_mode,
        daemon,
        app_facade: app_facade_for_daemon,
        settings: settings_port,
        space_setup_assembly,
        deferred_ready_notify: runtime_controls.deferred_ready_notify,
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate,
    };
    // daemon main loop 退出后清空 AppFacade 上 5 个 lifecycle 字段,
    // 让残留 GUI command / task 看到 None → 报"daemon 未就绪"。
    let join = tokio::spawn(async move {
        let result = run_daemon_main(input).await;
        app_facade_for_cleanup.clear_daemon_lifecycle();
        result
    });

    Ok(DaemonHandle::new(cancel, join))
}
