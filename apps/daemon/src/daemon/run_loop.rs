//! daemon 运行循环。

use uc_bootstrap::SyncEngineAssembly;

use crate::daemon::app::DaemonApp;

/// daemon 运行循环输入。
pub struct DaemonRunLoopInput {
    pub daemon: DaemonApp,
    pub sync_engine_assembly: SyncEngineAssembly,
}

/// Run the daemon main loop, then close the P2P session in order.
///
/// The caller must already be inside a Tokio runtime. The process runtime
/// spawns this future and returns a [`crate::daemon::DaemonHandle`].
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
