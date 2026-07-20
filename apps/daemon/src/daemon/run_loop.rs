//! daemon 运行循环。

use uc_bootstrap::SyncEngineAssembly;

use crate::daemon::app::DaemonApp;

/// daemon 运行循环输入。
pub struct DaemonRunLoopInput {
    pub daemon: DaemonApp,
    pub sync_engine_assembly: SyncEngineAssembly,
}

/// 运行 daemon main loop，退出后关闭 space setup 资源。
///
/// async 形态：caller 必须已经在 tokio runtime 上下文中。daemon binary 入口
/// `run` 在自己的 `Runtime::block_on` 里经 async assembly 入口
/// （[`crate::daemon::start_in_process`]）调用——后者通过 `tokio::spawn` 把它
/// 跑成 task，由 [`crate::daemon::DaemonHandle`] 持有 join handle。
pub async fn run_daemon_main(input: DaemonRunLoopInput) -> anyhow::Result<()> {
    let DaemonRunLoopInput {
        daemon,
        sync_engine_assembly,
    } = input;

    // ORDERING (ADR-008 P5-L L8a) — these two awaits are SEQUENTIAL: the iroh
    // teardown (`sync_engine_assembly.shutdown()`, which drives
    // `endpoint.close()`) runs strictly AFTER the run loop returns and strictly
    // BEFORE this task (`run_daemon_main`, spawned in `host.rs` and awaited via
    // `handle.wait()`) completes. That upholds the lock-after-iroh ordering: the
    // instance lock in `host.rs` is only dropped after `handle.wait()` completes,
    // hence after this iroh unbind. Keep these awaits ordered run-then-shutdown.
    let result = daemon.run().await;
    sync_engine_assembly.shutdown().await;
    result
}
