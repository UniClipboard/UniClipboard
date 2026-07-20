//! Daemon host entry points.
//!
//! [`run`] creates the standalone daemon runtime and blocks until the main
//! loop exits. Process assembly and daemon startup stay behind
//! [`DaemonProcessRuntime`]; GUI processes remain pure daemon clients.

use uc_application::clipboard_write::{RestoreBroadcastRequest, RestoreBroadcastTrigger};

use super::process_runtime::{DaemonProcessRuntime, ProcessRuntimeHandles};
use super::run_mode::DaemonRunMode;
use super::tokio_runtime::build_daemon_tokio_runtime;

/// Standalone daemon binary entry: creates its own tokio runtime, starts the
/// daemon process runtime, and blocks until exit.
pub fn run(run_mode: DaemonRunMode) -> anyhow::Result<()> {
    let rt = build_daemon_tokio_runtime()?;
    rt.block_on(async move {
        let super::process_bootstrap::ProcessRuntimeContext {
            wired,
            background,
            storage_paths,
            config: _config,
        } = super::process_bootstrap::build_process_runtime(run_mode).await?;

        // Keep the instance lock until the daemon handle has completed the
        // ordered iroh shutdown. A replacement must not bind while the old
        // endpoint is still releasing its socket.
        let _instance_lock = uc_daemon_local::instance_lock::acquire_with_deadline(
            &storage_paths.app_data_root_dir,
            uc_daemon_local::timing::LOCK_ACQUIRE_DEADLINE,
        )
        .await
        .map_err(|error| {
            let error_kind = match &error {
                uc_daemon_local::instance_lock::InstanceLockError::AlreadyRunning { .. } => {
                    "instance_lock_already_running"
                }
                uc_daemon_local::instance_lock::InstanceLockError::Io(_) => "instance_lock_io",
            };
            tracing::error!(
                error_kind,
                retryable = false,
                error = %error,
                "daemon instance lock acquisition failed - exiting"
            );
            anyhow::anyhow!("{error}")
        })?;

        uc_daemon_local::handover::clear(&storage_paths.app_data_root_dir);

        let clipboard_write_coordinator = background.clipboard_write_coordinator.clone();
        let file_transfer_lifecycle = background.file_transfer_lifecycle.clone();
        let file_transfer_facade = wired.shared.file_transfer_facade.clone();

        let (restore_broadcast_tx, restore_broadcast_rx) =
            tokio::sync::mpsc::unbounded_channel::<RestoreBroadcastRequest>();
        let restore_broadcast_trigger = RestoreBroadcastTrigger::new(restore_broadcast_tx);

        let runtime = DaemonProcessRuntime::new(
            wired.deps.clone(),
            storage_paths.clone(),
            clipboard_write_coordinator.clone(),
            file_transfer_facade.clone(),
            restore_broadcast_trigger,
        );

        let blob_ports = uc_bootstrap::BlobProcessingPorts::from_app_deps(&wired.deps);
        runtime.spawn_blob_processing(background, blob_ports);

        let handles = ProcessRuntimeHandles {
            wired,
            storage_paths,
            clipboard_write_coordinator,
            file_transfer_lifecycle,
            file_transfer_facade,
        };
        let handle = runtime
            .start(run_mode, handles, restore_broadcast_rx)
            .await?;
        let result = handle.wait().await;
        drop(runtime);
        result
    })
}

pub use uc_daemon_local::spawn_contract::RUN_MODE_ENV;
pub use uc_daemon_local::spawn_contract::RUN_MODE_ONESHOT;
pub use uc_daemon_local::spawn_contract::RUN_MODE_SERVER;

/// Standalone daemon binary entry: parse run mode from environment, then start.
pub fn run_standalone_from_env() -> anyhow::Result<()> {
    let run_mode = match std::env::var(RUN_MODE_ENV).as_deref() {
        Ok(RUN_MODE_SERVER) => {
            std::env::set_var("UC_DISABLE_SYSTEM_CLIPBOARD", "1");
            DaemonRunMode::ServerHeadless
        }
        Ok(RUN_MODE_ONESHOT) => DaemonRunMode::Oneshot,
        _ => DaemonRunMode::Standalone,
    };
    run(run_mode)
}
