//! Daemon host entry points.
//!
//! Provides two interfaces:
//!
//! - [`run`]: Synchronous blocking entry for the standalone daemon binary
//!   (`uniclipd`) — creates its own tokio runtime, internally calls
//!   [`start_in_process`] and blocks on the returned handle until the main
//!   loop exits (driven by OS signals).
//! - [`start_in_process`]: Async assembly core — assembles the process runtime,
//!   spawns the daemon main loop as a task and returns a [`DaemonHandle`].
//!   `run` wraps it for the binary; it is the daemon's own assembly body, not a
//!   GUI entry point (ADR-008 P3-3: the GUI is a pure external-daemon client and
//!   never hosts a daemon in-process).
//!
//! Both share the same assembly + main loop implementation
//! ([`build_daemon_bootstrap_assembly`] / [`run_daemon_main`]).
//!
//! Migrated from `uc-desktop/src/daemon/host.rs` (ADR-008 P2, Slice 2b).

use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uc_application::clipboard_write::ClipboardWriteCoordinator;
use uc_application::facade::{AppFacade, AppPaths, FileTransferFacade};
use uc_bootstrap::assembly::WiredDependencies;
use uc_bootstrap::file_transfer_lifecycle::FileTransferLifecycle;

use super::app_assembly::{build_daemon_app_instance, DaemonAppAssemblyInput};
use super::app_facade_assembly::{build_daemon_lifecycle_facades, DaemonLifecycleFacadesInput};
use super::bootstrap::{build_daemon_bootstrap_assembly, DaemonBootstrapAssembly};
use super::handle::DaemonHandle;
use super::mobile_lan_lifecycle::{AppFacadeListenerSpawner, MobileLanLifecycleController};
use super::process_runtime::DaemonProcessRuntime;
use super::run_loop::{run_daemon_main, DaemonRunLoopInput};
use super::run_mode::DaemonRunMode;
use super::runtime_assembly::{build_daemon_runtime_workers, DaemonRuntimeAssemblyInput};
use super::runtime_controls::build_daemon_runtime_controls;
use super::search_assembly::build_daemon_search_assembly;
use super::service_assembly::build_daemon_service_plan;
use super::tokio_runtime::build_daemon_tokio_runtime;

/// Process-level persistent resource handles passed to the daemon on each spawn.
///
/// Daemon-lifecycle resources (iroh node / space_setup / HTTP server / LAN
/// listener) are rebuilt on each daemon start/stop, but these persistent
/// resources survive daemon reloads — the sqlite pool etc. are not destroyed
/// when the daemon restarts.
///
/// `Clone` is derived: `wired` internally holds `Arc<dyn Port>` / `PathBuf`,
/// other fields are also `Arc`, so clone is just a set of `Arc::clone` calls.
#[derive(Clone)]
pub struct ProcessRuntimeHandles {
    pub wired: WiredDependencies,
    pub storage_paths: AppPaths,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    pub file_transfer_facade: Arc<FileTransferFacade>,
}

/// Standalone daemon binary entry: creates its own tokio runtime, starts the
/// daemon via [`start_in_process`], and blocks until exit.
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let rt = build_daemon_tokio_runtime()?;
    rt.block_on(async move {
        let super::process_bootstrap::ProcessRuntimeContext {
            wired,
            background,
            storage_paths,
            config: _config,
        } = super::process_bootstrap::build_process_runtime(run_mode).await?;

        // D22: acquire per-profile instance lock before any port binding.
        //
        // ORDERING (ADR-008 P5-L L8a) — load-bearing for P5-L L8 controlled
        // restart: this guard is held to the END of the `block_on` closure,
        // i.e. until AFTER `handle.wait()` below returns. `handle.wait()`
        // encloses the full iroh `endpoint.close()` teardown (via
        // `run_loop.rs`'s sequential `daemon.run().await` → `…shutdown().await`),
        // so the lock drops strictly AFTER iroh fully unbinds its socket. A new
        // daemon must not acquire the lock until the old daemon's iroh socket is
        // released, otherwise the replacement races `AddrInUse`. Do NOT move
        // this guard's scope earlier (e.g. into a sub-block).
        //
        // Acquisition waits for the lock EVENT-DRIVEN (blocking `flock` on a
        // detached thread; the kernel wakes us the instant the holder
        // releases) on EVERY start — not only controlled-restart promotions —
        // because an exiting predecessor still holds the lock during iroh
        // teardown (its `/health` goes absent before lock-release). Any
        // health-probing spawner (CLI/GUI) can hit that window after a plain
        // stop/start cycle: observed in production (2026-06-12), the spawner
        // saw `/health` absent ~2s after the predecessor's shutdown signal and
        // spawned a replacement that lost the then-single-shot `try_acquire`
        // to a predecessor that needed ~5.4s to release. The deadline
        // (`timing::LOCK_ACQUIRE_DEADLINE`) is pure hang protection, not a
        // teardown estimate — a healthy holder costs the waiter nothing
        // extra, since only a manual double-launch ever waits it out. I/O
        // errors are still terminal on the first attempt.
        let _instance_lock = uc_daemon_local::instance_lock::acquire_with_deadline(
            &storage_paths.app_data_root_dir,
            uc_daemon_local::timing::LOCK_ACQUIRE_DEADLINE,
        )
        .await
        .map_err(|e| {
            // Surface the failure in the JSON log: this error otherwise only
            // reaches stderr via anyhow's Termination in `main`, which is
            // detached/nulled in production — the log would just stop dead
            // after bootstrap with no trace of why the process exited.
            let error_kind = match &e {
                uc_daemon_local::instance_lock::InstanceLockError::AlreadyRunning { .. } => {
                    "instance_lock_already_running"
                }
                uc_daemon_local::instance_lock::InstanceLockError::Io(_) => "instance_lock_io",
            };
            tracing::error!(
                error_kind,
                retryable = false,
                error = %e,
                "daemon instance lock acquisition failed — exiting"
            );
            anyhow::anyhow!("{e}")
        })?;

        // ADR-008 P5-L L7: now that we hold the instance lock, consume any pending
        // cross-process handover by clearing it (claim under lock — R8-F1). A
        // controlled restart (L8) leaves a {target_mode, generation} record in the
        // lock dir; the new daemon clears it here. No-op in production (no writer yet).
        uc_daemon_local::handover::clear(&storage_paths.app_data_root_dir);

        let clipboard_write_coordinator = background.clipboard_write_coordinator.clone();
        let file_transfer_lifecycle = background.file_transfer_lifecycle.clone();
        let file_transfer_facade = wired.file_transfer_facade.clone();

        let runtime = DaemonProcessRuntime::new(
            wired.deps.clone(),
            storage_paths.clone(),
            clipboard_write_coordinator.clone(),
            file_transfer_facade.clone(),
        );
        let app_facade = Arc::clone(runtime.app_facade());

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
        let result = handle.wait().await;
        drop(runtime);
        result
    })
}

pub use uc_daemon_local::spawn_contract::RUN_MODE_ENV;
pub use uc_daemon_local::spawn_contract::RUN_MODE_ONESHOT;
pub use uc_daemon_local::spawn_contract::RUN_MODE_SERVER;

/// Standalone daemon binary entry: parse run mode from environment, then start.
///
/// Reads [`RUN_MODE_ENV`]: `"server"` → [`DaemonRunMode::ServerHeadless`]
/// (headless node, no X11/Wayland); `"oneshot"` → [`DaemonRunMode::Oneshot`]
/// (ADR-008 P5-L L0 inert skeleton, behavior-identical to standalone and not
/// emitted by any spawner yet); otherwise → [`DaemonRunMode::Standalone`].
pub fn run_standalone_from_env() -> anyhow::Result<()> {
    let run_mode = match std::env::var(RUN_MODE_ENV).as_deref() {
        Ok(RUN_MODE_SERVER) => {
            std::env::set_var("UC_DISABLE_SYSTEM_CLIPBOARD", "1");
            DaemonRunMode::ServerHeadless
        }
        // P5-L L0: Oneshot runs the system clipboard like Standalone, so it
        // must NOT set UC_DISABLE_SYSTEM_CLIPBOARD. Unreachable in production
        // (no spawner emits RUN_MODE_ONESHOT); decode only.
        Ok(RUN_MODE_ONESHOT) => DaemonRunMode::Oneshot,
        _ => DaemonRunMode::Standalone,
    };
    run(run_mode)
}

/// In-process daemon start (async).
///
/// Assumes the caller already has an active tokio runtime context. Assembles
/// the daemon, spawns the main loop as a task, and returns a [`DaemonHandle`]
/// for explicit shutdown.
pub async fn start_in_process(
    run_mode: DaemonRunMode,
    app_facade: Arc<AppFacade>,
    handles: ProcessRuntimeHandles,
) -> anyhow::Result<DaemonHandle> {
    let cancel = CancellationToken::new();

    // ADR-008 D9 (P4-2): strict-unattended self-check — the only hard boundary
    // for the unlock contract. A daemon launched with no GUI fallback
    // (autostart / service manager sets UC_DAEMON_UNATTENDED=1, P4-4) cannot
    // honor `auto_unlock_enabled = false`, so fail fast with a clear log + a
    // non-zero exit instead of coming up locked and silently doing nothing.
    // P4-5: also record a machine-readable last-exit report so the next GUI
    // startup surfaces a red banner instead of a silent refusal.
    if uc_daemon_local::spawn_contract::unattended_from_env() {
        let settings = handles.wired.deps.settings.load().await.unwrap_or_default();
        if let Err(violation) = uc_daemon_local::spawn_contract::validate_unattended_unlock(
            true,
            settings.security.auto_unlock_enabled,
        ) {
            tracing::error!(%violation, "daemon refusing to start (ADR-008 D9 unlock contract)");
            let marker = uc_daemon_local::crash_marker::DaemonRunMarker::new(
                handles.storage_paths.app_data_root_dir.clone(),
            );
            if let Err(error) = marker.record_startup_failure(violation.to_string()) {
                tracing::warn!(error = %error, "failed to record daemon startup-failure marker");
            }
            return Err(anyhow::anyhow!("{violation}"));
        }
    }

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
    let host_event_bus = wired.host_event_bus;
    let settings_port = deps.settings.clone();
    let runtime_controls = build_daemon_runtime_controls(run_mode);

    let runtime_workers = build_daemon_runtime_workers(DaemonRuntimeAssemblyInput {
        deps: &deps,
        run_mode,
        event_tx: runtime_controls.event_tx.clone(),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        clipboard_sync_facade: clipboard_sync_facade.clone(),
        blob_transfer_facade: blob_transfer_facade.clone(),
        file_cache_dir: storage_paths.file_cache_dir.clone(),
        file_transfer_lifecycle,
        clipboard_write_coordinator: clipboard_write_coordinator.clone(),
        host_event_bus: host_event_bus.clone(),
        entry_delivery_repo: wired.entry_delivery_repo.clone(),
        clipboard_event_reader_repo: wired.clipboard_event_reader_repo.clone(),
        trusted_peer_repo: wired.trusted_peer_repo.clone(),
    })?;

    let search_assembly = build_daemon_search_assembly(&deps, runtime_controls.event_tx.clone());

    let service_plan = build_daemon_service_plan(
        run_mode,
        runtime_controls.encryption_unlocked,
        &runtime_workers,
        &search_assembly,
    );

    let storage_paths_for_daemon = storage_paths.clone();

    let mobile_lan_lifecycle: Arc<MobileLanLifecycleController> =
        Arc::new(MobileLanLifecycleController::new(
            mobile_sync_endpoint_info.clone(),
            Arc::new(AppFacadeListenerSpawner::new(
                Arc::clone(&app_facade),
                Some(file_transfer_facade.clone()),
            )),
        ));

    let (lifecycle_facades, local_device_id) =
        build_daemon_lifecycle_facades(DaemonLifecycleFacadesInput {
            deps: &deps,
            storage_paths: &storage_paths_for_daemon,
            space_setup_assembly: &space_setup_assembly,
            clipboard_sync: clipboard_sync_facade.clone(),
            blob_transfer: blob_transfer_facade.clone(),
            file_transfer: file_transfer_facade.clone(),
            mobile_sync_apply_inbound: runtime_workers.apply_inbound.clone(),
            clipboard_outbound: runtime_workers.clipboard_outbound.clone(),
            lan_lifecycle: Arc::clone(&mobile_lan_lifecycle)
                as Arc<dyn uc_core::ports::MobileLanLifecyclePort>,
            clipboard_restore: app_facade.clipboard_restore.clone(),
        });

    app_facade.install_daemon_lifecycle(lifecycle_facades);

    app_facade
        .search
        .set_coordinator(Arc::clone(&search_assembly.coordinator));

    let app_facade_for_daemon = Arc::clone(&app_facade);
    let daemon = build_daemon_app_instance(DaemonAppAssemblyInput {
        service_plan,
        app_facade: Arc::clone(&app_facade_for_daemon),
        storage_paths: storage_paths_for_daemon,
        host_event_bus: host_event_bus.clone(),
        event_tx: runtime_controls.event_tx,
        encryption_unlocked: runtime_controls.encryption_unlocked,
        deferred_ready_notify: runtime_controls.deferred_ready_notify.clone(),
        external_shutdown: Some(cancel.clone()),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        local_device_id,
        listens_to_os_signals: run_mode.listens_to_os_signals(),
        process_mode: run_mode.process_mode(),
        residency: run_mode.into(),
        mobile_sync_endpoint_info,
        mobile_lan_lifecycle: Arc::clone(&mobile_lan_lifecycle),
        analytics: Arc::clone(&deps.analytics),
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
    let join = tokio::spawn(run_daemon_main(input));

    Ok(DaemonHandle::new(cancel, join))
}
