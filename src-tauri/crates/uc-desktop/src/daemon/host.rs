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
//! daemon 不再 wire 自己的 deps —— caller 通过 [`ProcessRuntimeHandles`]
//! 把进程级一次性资源 (sqlite pool / repos / settings / blob workers /
//! clipboard_write_coordinator / file_transfer_lifecycle 等) 透传进来,
//! daemon 启停时只重建 daemon-lifecycle 资源 (iroh node / space_setup /
//! HTTP server / LAN listener)。整个进程只有一份 `AppFacade`,daemon 启动
//! swap 5 个子 facade 进去,退出时清空。
//!
//! blob/spool worker 不在这里 spawn —— caller 在进程启动期一次性
//! `spawn_blob_processing_tasks`,挂在进程级 task_registry 上。

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::facade::{AppFacade, AppPaths, FileTransferFacade};
use uc_bootstrap::assembly::WiredDependencies;
use uc_bootstrap::file_transfer_lifecycle::FileTransferLifecycle;

use crate::daemon::app_assembly::{build_daemon_app_instance, DaemonAppAssemblyInput};
use crate::daemon::app_facade_assembly::{
    build_daemon_lifecycle_facades, DaemonLifecycleFacadesInput,
};
use crate::daemon::bootstrap::{build_daemon_bootstrap_assembly, DaemonBootstrapAssembly};
use crate::daemon::handle::DaemonHandle;
use crate::daemon::mobile_lan_lifecycle::{AppFacadeListenerSpawner, MobileLanLifecycleController};
use crate::daemon::run_loop::{run_daemon_main, DaemonRunLoopInput};
use crate::daemon::run_mode::DaemonRunMode;
use crate::daemon::runtime_assembly::{build_daemon_runtime_workers, DaemonRuntimeAssemblyInput};
use crate::daemon::runtime_controls::build_daemon_runtime_controls;
use crate::daemon::search_assembly::build_daemon_search_assembly;
use crate::daemon::service_assembly::build_daemon_service_plan;
use crate::daemon::tokio_runtime::build_daemon_tokio_runtime;

/// 进程级一次性资源句柄,daemon 每次 spawn 都从 caller 拿一份 clone。
///
/// daemon-lifecycle (iroh node / space_setup / HTTP server / LAN listener)
/// 在每次 daemon start/stop 重建,但这些"持久"资源跨 daemon reload 复用 ——
/// sqlite pool 等不会因 daemon 重启而被销毁重建。
///
/// `Clone` 派生:`wired` 内部全是 `Arc<dyn Port>` / `PathBuf`,其它字段
/// 也是 Arc,clone 等价于一组 Arc::clone。
#[derive(Clone)]
pub struct ProcessRuntimeHandles {
    pub wired: WiredDependencies,
    pub storage_paths: AppPaths,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    /// 进程级 file-transfer facade(从 `BackgroundRuntimeDeps` clone 出来)。
    /// daemon-lifecycle 装配时喂给 `MobileSyncFacade`,GUI shell 也用同一份
    /// 装进进程级 `AppFacade.file_transfer`。整个进程只有一份。
    pub file_transfer_facade: Arc<FileTransferFacade>,
}

/// 独立 daemon binary 入口：创建专属 tokio runtime,启动 daemon,阻塞到退出。
///
/// 这条路径 GUI shell 不应再用——in-process 拉起请改走 [`start_in_process`]。
///
/// standalone binary 没有 GUI shell 持有的 `Arc<AppFacade>`,所以入口内部
/// 调 `build_process_runtime` 装一份进程级 deps + facade,顺带 spawn 一次
/// blob/spool worker (跨 daemon reload 不重建),然后跑标准 daemon-lifecycle。
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let rt = build_daemon_tokio_runtime()?;
    rt.block_on(async move {
        // standalone 自己 wire 一次进程级 deps + facade。这份 facade 不被
        // 暴露给任何外部 caller (没有 GUI shell),只活在 daemon 进程生命周期
        // 内 —— daemon main loop 退出 binary 整个 exit。
        let crate::bootstrap::ProcessRuntimeContext {
            wired,
            background,
            storage_paths,
            config: _config,
        } = crate::bootstrap::build_process_runtime()?;

        let event_emitter: Arc<dyn uc_application::facade::HostEventEmitterPort> =
            Arc::new(uc_bootstrap::LoggingHostEventEmitter);
        let clipboard_write_coordinator = background.clipboard_write_coordinator.clone();
        let file_transfer_lifecycle = background.file_transfer_lifecycle.clone();
        let file_transfer_facade = wired.file_transfer_facade.clone();

        let runtime = crate::DesktopRuntime::with_setup(
            wired.deps.clone(),
            storage_paths.clone(),
            event_emitter,
            clipboard_write_coordinator.clone(),
            file_transfer_facade.clone(),
        );
        let app_facade = Arc::clone(runtime.app_facade());

        // 进程级 blob/spool worker —— 一次性 spawn,挂在 runtime task_registry
        // 上,跨 daemon reload 不重建 (本 standalone binary 进程不 reload,这里
        // 与 in-process 路径保持同一编排形态)。
        let blob_ports = uc_bootstrap::BlobProcessingPorts::from_app_deps(&wired.deps);
        let task_registry_for_blob = Arc::clone(runtime.task_registry());
        tokio::spawn(async move {
            uc_bootstrap::spawn_blob_processing_tasks(
                background,
                blob_ports,
                &task_registry_for_blob,
            )
            .await;
        });

        let handles = ProcessRuntimeHandles {
            wired,
            storage_paths,
            clipboard_write_coordinator,
            file_transfer_lifecycle,
            file_transfer_facade,
        };
        let handle = start_in_process(run_mode, app_facade, handles).await?;
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
/// - `app_facade` 进程级单例 `AppFacade`。GUI shell `build_process_runtime`
///   后通过 `DesktopRuntime::with_setup` 装好,daemon 启动 swap 5 个
///   daemon-lifecycle 子 facade(space_setup / member_roster / clipboard_sync /
///   blob_transfer / mobile_sync) 进去,daemon 退出时清空。整个进程只有
///   这一份 `AppFacade`。
///
/// - `handles` 进程级一次性资源 (`WiredDependencies` + storage_paths +
///   clipboard_write_coordinator + file_transfer_lifecycle)。daemon
///   start_in_process 不再 wire 自己的 deps —— sqlite pool 等跨 daemon
///   reload 复用同一份。
pub(crate) async fn start_in_process(
    run_mode: DaemonRunMode,
    app_facade: Arc<AppFacade>,
    handles: ProcessRuntimeHandles,
) -> anyhow::Result<DaemonHandle> {
    let cancel = CancellationToken::new();

    let DaemonBootstrapAssembly {
        clipboard_sync_facade,
        blob_transfer_facade,
        space_setup_assembly,
        mobile_sync_endpoint_info,
    } = build_daemon_bootstrap_assembly(&handles.wired).await?;

    let ProcessRuntimeHandles {
        wired,
        storage_paths,
        clipboard_write_coordinator,
        file_transfer_lifecycle,
        file_transfer_facade,
    } = handles;

    let deps = wired.deps;
    let emitter_cell = wired.emitter_cell;
    let settings_port = deps.settings.clone();
    let runtime_controls = build_daemon_runtime_controls(run_mode);

    let runtime_workers = build_daemon_runtime_workers(DaemonRuntimeAssemblyInput {
        deps: &deps,
        event_tx: runtime_controls.event_tx.clone(),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        clipboard_sync_facade: clipboard_sync_facade.clone(),
        blob_transfer_facade: blob_transfer_facade.clone(),
        file_cache_dir: storage_paths.file_cache_dir.clone(),
        file_transfer_lifecycle,
        clipboard_write_coordinator: clipboard_write_coordinator.clone(),
        host_event_emitter: emitter_cell.clone(),
    })?;

    // blob/spool worker **不在这里 spawn** —— 它们是进程级 long-lived task,
    // 由 caller 在进程启动期一次性 spawn,挂在进程级 task_registry 上,跨
    // daemon reload 不重建。

    let search_assembly = build_daemon_search_assembly(&deps, runtime_controls.event_tx.clone());

    let service_plan = build_daemon_service_plan(
        run_mode,
        runtime_controls.encryption_unlocked,
        &runtime_workers,
        &search_assembly,
    );

    let storage_paths_for_daemon = storage_paths.clone();

    // # mobile_sync LAN listener 生命周期 controller
    //
    // 装配在 lifecycle facade 构造前完成,因为 mobile_sync facade 要把它存
    // 进 `MobileSyncFacadeDeps::lan_lifecycle` —— update_settings 写盘后
    // 通过它即时 start/stop/rebind listener。
    //
    // controller 不直接持 `Arc<MobileSyncFacade>`(否则 facade ↔ controller
    // 循环引用,构造顺序无解);改持 `Arc<AppFacade>`,运行期通过
    // `AppFacade.mobile_sync` OnceLock 取当前 facade。装配此刻 OnceLock 还
    // 未装入,但 controller 不会在此前被调用 —— apply() 只在 daemon
    // run() 启动后 或 update_settings 收到 PATCH 时触发。
    let mobile_lan_lifecycle: Arc<MobileLanLifecycleController> =
        Arc::new(MobileLanLifecycleController::new(
            mobile_sync_endpoint_info.clone(),
            Arc::new(AppFacadeListenerSpawner::new(
                Arc::clone(&app_facade),
                Some(file_transfer_facade.clone()),
            )),
        ));

    // Phase 4 重构:不再装第二份 `AppFacade`,改为构造 5 个 daemon-lifecycle
    // 子 facade 然后 swap 进 GUI shell 已装好的进程级 AppFacade。
    let (lifecycle_facades, local_device_id) =
        build_daemon_lifecycle_facades(DaemonLifecycleFacadesInput {
            deps: &deps,
            storage_paths: &storage_paths_for_daemon,
            space_setup_assembly: &space_setup_assembly,
            clipboard_sync: clipboard_sync_facade.clone(),
            blob_transfer: blob_transfer_facade.clone(),
            file_transfer: file_transfer_facade.clone(),
            mobile_sync_apply_inbound: runtime_workers.apply_inbound.clone(),
            lan_lifecycle: Arc::clone(&mobile_lan_lifecycle)
                as Arc<dyn uc_core::ports::MobileLanLifecyclePort>,
        });

    app_facade.install_daemon_lifecycle(lifecycle_facades);

    // search_coordinator 是 daemon-lifecycle 的(绑 daemon search assembly),
    // 进程级 SearchFacade 内部 coordinator 字段在 GUI 启动期为空, daemon
    // 启动时通过 SearchFacade::set_coordinator 装入一次。方案 C 后 daemon
    // 进程内不再 reload, Arc 跟随进程退出自然回收。
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
        mobile_lan_lifecycle: Arc::clone(&mobile_lan_lifecycle),
    });

    let input = DaemonRunLoopInput {
        run_mode,
        daemon,
        app_facade: app_facade_for_daemon,
        settings: settings_port,
        space_setup_assembly,
        deferred_ready_notify: runtime_controls.deferred_ready_notify,
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate,
    };
    // 方案 C 后 daemon 退出 = 进程退出, daemon-lifecycle 字段无需显式卸下,
    // 跟随 AppFacade Arc drop 自然回收。
    let join = tokio::spawn(run_daemon_main(input));

    Ok(DaemonHandle::new(cancel, join))
}
