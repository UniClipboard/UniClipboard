use std::sync::Arc;

use tokio_util::sync::CancellationToken;
use uc_application::clipboard_write::{ClipboardWriteCoordinator, RestoreBroadcastRequest};
use uc_application::facade::{
    ActiveClipboardReconcileDeps, ActiveClipboardReconcileFacade, AppFacade, AppPaths,
    ClipboardSnapshotDeps, FileTransferFacade,
};
use uc_application::receive_reconciliation::EnsureReceiveReadyPort;
use uc_bootstrap::{FileTransferLifecycle, WiredDependencies};

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
use crate::daemon::startup_recovery::{
    record_upgrade_status_at_startup, spawn_startup_recovery, DaemonReceiveReadinessCoordinator,
    StartupRecoveryInput,
};

#[derive(Clone)]
pub(crate) struct ProcessRuntimeHandles {
    pub wired: WiredDependencies,
    pub storage_paths: AppPaths,
    pub clipboard_write_coordinator: Arc<ClipboardWriteCoordinator>,
    pub file_transfer_lifecycle: Arc<FileTransferLifecycle>,
    pub file_transfer_facade: Arc<FileTransferFacade>,
}

pub(super) async fn start_runtime(
    run_mode: DaemonRunMode,
    app_facade: Arc<AppFacade>,
    handles: ProcessRuntimeHandles,
    restore_broadcast_rx: tokio::sync::mpsc::UnboundedReceiver<RestoreBroadcastRequest>,
) -> anyhow::Result<DaemonHandle> {
    let cancel = CancellationToken::new();

    // A strict unattended process has no GUI fallback for manual unlock.
    // Refuse startup instead of running indefinitely with all work gated.
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

    // Treat the persisted active register as an untrusted baseline. Reconcile
    // it before workers can broadcast or apply the stale activation.
    {
        let clipboard = &handles.wired.deps.clipboard;
        let reconcile = ActiveClipboardReconcileFacade::new(ActiveClipboardReconcileDeps {
            system_clipboard: clipboard.system_clipboard.clone(),
            load_register: clipboard.active_register_load.clone(),
            reset_register: clipboard.active_register_reset.clone(),
            snapshot: ClipboardSnapshotDeps {
                entry_repo: clipboard.entry_ports.get.clone(),
                selection_repo: clipboard.selection_repo.clone(),
                representation_repo: clipboard.representation_ports.get.clone(),
                rep_processing_repo: clipboard
                    .representation_ports
                    .update_processing_result
                    .clone(),
                payload_resolver: clipboard.payload_resolver.clone(),
                blob_store: handles.wired.deps.storage.blob_store.clone(),
            },
        });
        let outcome = reconcile.reconcile().await;
        tracing::debug!(
            ?outcome,
            "active-clipboard register startup reconcile complete"
        );
    }

    let DaemonBootstrapAssembly {
        clipboard_sync_facade,
        blob_transfer_facade,
        mut sync_engine_assembly,
        mobile_sync_endpoint_info,
    } = build_daemon_bootstrap_assembly(&handles.wired).await?;

    let ProcessRuntimeHandles {
        wired,
        storage_paths,
        clipboard_write_coordinator,
        file_transfer_lifecycle,
        file_transfer_facade,
    } = handles;

    // The runtime owns the receiver so restore broadcasts stop with the P2P
    // session and cannot outlive an engine restart.
    sync_engine_assembly.attach_restore_broadcast(restore_broadcast_rx);

    let deps = wired.deps;
    let host_event_bus = wired.shared.host_event_bus;
    let settings_port = deps.settings.clone();
    let runtime_controls = build_daemon_runtime_controls(run_mode);
    let receive_recovery: Arc<dyn EnsureReceiveReadyPort> = file_transfer_lifecycle.clone();
    let receive_readiness: Arc<dyn EnsureReceiveReadyPort> =
        Arc::new(DaemonReceiveReadinessCoordinator::new(
            receive_recovery,
            runtime_controls.clipboard_capture_gate.clone(),
            runtime_controls.deferred_ready_notify.clone(),
        ));

    let runtime_workers = build_daemon_runtime_workers(DaemonRuntimeAssemblyInput {
        deps: &deps,
        run_mode,
        system_clipboard_wiring: wired.system_clipboard_wiring,
        event_tx: runtime_controls.event_tx.clone(),
        clipboard_capture_gate: runtime_controls.clipboard_capture_gate.clone(),
        clipboard_sync_facade: clipboard_sync_facade.clone(),
        blob_transfer_facade: blob_transfer_facade.clone(),
        file_cache_dir: storage_paths.file_cache_dir.clone(),
        file_transfer_lifecycle: file_transfer_lifecycle.clone(),
        receive_readiness: wired.shared.receive_readiness.clone(),
        clipboard_write_coordinator: clipboard_write_coordinator.clone(),
        host_event_bus: host_event_bus.clone(),
        entry_delivery_repo: wired.shared.entry_delivery_repo.clone(),
        clipboard_event_reader_repo: wired.shared.clipboard_event_reader_repo.clone(),
        trusted_peer_repo: wired.shared.trusted_peer_repo.clone(),
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
                wired.shared.active_clipboard_sse_source.clone(),
            )),
        ));

    let (lifecycle_facades, local_device_id) =
        build_daemon_lifecycle_facades(DaemonLifecycleFacadesInput {
            deps: &deps,
            storage_paths: &storage_paths_for_daemon,
            sync_engine_assembly: &sync_engine_assembly,
            clipboard_sync: clipboard_sync_facade.clone(),
            blob_transfer: blob_transfer_facade.clone(),
            file_transfer: file_transfer_facade.clone(),
            mobile_sync_apply_inbound: runtime_workers.apply_inbound.clone(),
            clipboard_outbound: runtime_workers.clipboard_outbound.clone(),
            lan_lifecycle: Arc::clone(&mobile_lan_lifecycle)
                as Arc<dyn uc_core::ports::MobileLanLifecyclePort>,
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
        receive_readiness: Arc::clone(&receive_readiness),
    });

    let input = DaemonRunLoopInput {
        daemon,
        sync_engine_assembly,
    };
    // Startup recovery remains in the same task as the run loop so creating a
    // DaemonHandle stays non-blocking while HTTP readiness ordering is stable.
    let join = tokio::spawn(async move {
        record_upgrade_status_at_startup(&app_facade_for_daemon).await;
        spawn_startup_recovery(StartupRecoveryInput {
            run_mode,
            app_facade: app_facade_for_daemon,
            settings: settings_port,
            space_setup: input.sync_engine_assembly.facade.clone(),
            receive_readiness,
        });
        run_daemon_main(input).await
    });

    Ok(DaemonHandle::new(cancel, join))
}
