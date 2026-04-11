//! Reusable daemon entry point.
//!
//! Contains the full composition root extracted from `main.rs` so that both the
//! standalone `uniclipboard-daemon` binary and the `uniclipboard-cli daemon`
//! subcommand can start an identical daemon process.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uc_app::usecases::LoggingLifecycleEventEmitter;
use uc_app::usecases::SessionReadyEmitter;
use uc_bootstrap::assembly::SetupAssemblyPorts;
use uc_bootstrap::build_non_gui_runtime_with_emitter;
use uc_bootstrap::builders::build_daemon_app;
use uc_bootstrap::BlobProcessingPorts;
use uc_core::ports::SystemClipboardPort;

use crate::api::types::DaemonWsEvent;
use crate::app::{DaemonApp, SetupCompletionEmitter};
use crate::pairing::host::DaemonPairingHost;
use crate::peers::monitor::PeerMonitor;
use crate::service::DaemonService;
use crate::service::ServiceHealth;
use crate::state::{DaemonServiceSnapshot, RuntimeState};
use crate::workers::clipboard_watcher::{ClipboardWatcherWorker, DaemonClipboardChangeHandler};
use crate::workers::file_sync_orchestrator::FileSyncOrchestratorWorker;
use crate::workers::inbound_clipboard_sync::InboundClipboardSyncWorker;
use crate::workers::peer_discovery::PeerDiscoveryWorker;
use uc_platform::clipboard::LocalClipboard;

/// Run the daemon process. This is the composition root.
///
/// `gui_managed` mirrors the `--gui-managed` flag: when true, the daemon
/// monitors stdin for EOF (parent GUI exit) and defers clipboard capture
/// until the GUI signals readiness.
pub fn run(gui_managed: bool) -> anyhow::Result<()> {
    // When launched with --gui-managed, the parent GUI process keeps our stdin pipe open.
    // If the parent exits (normally, crash, or SIGKILL), the pipe closes and we detect EOF.
    // This token fires on EOF, triggering graceful daemon shutdown via DaemonApp's select loop.
    let external_shutdown = if gui_managed {
        let token = CancellationToken::new();
        let token_clone = token.clone();
        std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 1];
            // Blocks until stdin is closed (parent process gone)
            let _ = std::io::stdin().read(&mut buf);
            token_clone.cancel();
        });
        Some(token)
    } else {
        None
    };

    // build_daemon_app() calls build_core() which inits tracing + wires deps.
    // Safe to call outside tokio (no internal block_on in daemon path).
    let ctx = build_daemon_app()?;
    let daemon_network_control = ctx.deps.network_control.clone();
    let daemon_network_events = ctx.deps.network_ports.events.clone();
    let daemon_peer_directory = ctx.deps.network_ports.peers.clone();
    let daemon_settings = ctx.deps.settings.clone();
    let setup_ports = SetupAssemblyPorts::from_network(
        ctx.pairing_orchestrator.clone(),
        ctx.space_access_orchestrator.clone(),
        ctx.deps.network_ports.peers.clone(),
        None,
        Arc::new(LoggingLifecycleEventEmitter),
    );
    // Extract file_cache_dir, file_transfer_orchestrator, clipboard_write_coordinator,
    // and emitter_cell before ctx is consumed by runtime construction.
    let file_cache_dir = ctx.storage_paths.file_cache_dir.clone();
    let file_transfer_orchestrator = ctx.background.file_transfer_orchestrator.clone();
    let clipboard_write_coordinator = ctx.background.clipboard_write_coordinator.clone();
    let emitter_cell = ctx.emitter_cell.clone();

    // Extract blob processing ports before ctx.deps is moved.
    let blob_ports = BlobProcessingPorts::from_app_deps(&ctx.deps);
    let background = ctx.background;

    // Create Notify for deferred service startup.
    // This is triggered by either:
    // - SetupCompletionEmitter (setup flow completes on uninitialized device)
    // - /lifecycle/ready API endpoint (GUI signals unlock)
    let deferred_ready_notify = Arc::new(tokio::sync::Notify::new());
    let setup_completion_emitter: Arc<dyn SessionReadyEmitter> =
        Arc::new(SetupCompletionEmitter::new(deferred_ready_notify.clone()));

    let runtime = Arc::new(
        build_non_gui_runtime_with_emitter(
            ctx.deps,
            ctx.storage_paths.clone(),
            setup_ports,
            setup_completion_emitter,
            emitter_cell,
        )?
        .with_clipboard_write_coordinator(clipboard_write_coordinator.clone()),
    );

    // 1. Create the shared broadcast channel for WebSocket events.
    //    All services that emit WS events write to this same sender.
    let (event_tx, _) = broadcast::channel::<DaemonWsEvent>(64);

    // Build typed workers that don't depend on encryption state first.
    let local_clipboard: Arc<dyn SystemClipboardPort> = Arc::new(
        LocalClipboard::new()
            .map_err(|e| anyhow::anyhow!("failed to create LocalClipboard: {}", e))?,
    );

    // Reuse the runtime's clipboard change origin so restore commands, watcher,
    // inbound sync, and file restore all observe the same guard state.
    let clipboard_change_origin = runtime
        .wiring_deps()
        .clipboard
        .clipboard_change_origin
        .clone();

    // Clipboard capture gate: controls whether clipboard changes are processed.
    // In standalone CLI mode, capture is always enabled.
    // In GUI-managed mode, capture is disabled until the GUI signals readiness.
    let clipboard_capture_gate = Arc::new(std::sync::atomic::AtomicBool::new(!gui_managed));

    let clipboard_change_handler = Arc::new(DaemonClipboardChangeHandler::new(
        runtime.clone(),
        event_tx.clone(),
        clipboard_change_origin.clone(),
        file_transfer_orchestrator.clone(),
        clipboard_capture_gate.clone(),
    ));
    let clipboard_watcher = Arc::new(ClipboardWatcherWorker::new(
        local_clipboard.clone(),
        clipboard_change_handler,
    ));

    let inbound_clipboard_sync = Arc::new(InboundClipboardSyncWorker::new(
        runtime.clone(),
        event_tx.clone(),
        clipboard_write_coordinator.clone(),
        Some(file_cache_dir.clone()),
        Some(file_transfer_orchestrator.clone()),
    ));

    let file_sync_orchestrator_worker = Arc::new(FileSyncOrchestratorWorker::new(
        file_transfer_orchestrator,
        daemon_network_events.clone(),
        clipboard_write_coordinator,
        file_cache_dir,
        daemon_settings.clone(),
    ));

    let peer_discovery_worker: Arc<dyn DaemonService> = Arc::new(PeerDiscoveryWorker::new(
        daemon_network_control,
        daemon_network_events,
        daemon_peer_directory,
        daemon_settings,
    ));

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Phase 67: Recover encryption session BEFORE building services.
    let encryption_unlocked = rt.block_on(async {
        use tracing::{info_span, Instrument};

        // Determine whether auto-unlock should be attempted.
        // - CLI mode (gui_managed=false): always attempt auto-unlock
        // - GUI mode (gui_managed=true): respect the auto_unlock_enabled setting
        let auto_unlock_enabled = if gui_managed {
            let settings = runtime
                .wiring_deps()
                .settings
                .load()
                .await
                .unwrap_or_default();
            settings.security.auto_unlock_enabled
        } else {
            // CLI mode: always auto-unlock
            true
        };

        crate::app::recover_encryption_session(&runtime, auto_unlock_enabled)
            .instrument(info_span!("daemon.startup.recover_encryption_session"))
            .await
    })?;

    // Start background clipboard processing tasks.
    let task_registry = runtime.task_registry().clone();
    rt.spawn(async move {
        uc_bootstrap::spawn_blob_processing_tasks(background, blob_ports, &task_registry).await;
    });

    let should_defer_clipboard = gui_managed || !encryption_unlocked;

    let initial_statuses: Vec<DaemonServiceSnapshot> = vec![
        DaemonServiceSnapshot {
            name: "clipboard-watcher".to_string(),
            health: if should_defer_clipboard {
                ServiceHealth::Stopped
            } else {
                ServiceHealth::Healthy
            },
        },
        DaemonServiceSnapshot {
            name: "inbound-clipboard-sync".to_string(),
            health: if should_defer_clipboard {
                ServiceHealth::Stopped
            } else {
                ServiceHealth::Healthy
            },
        },
        DaemonServiceSnapshot {
            name: "file-sync-orchestrator".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "peer-discovery".to_string(),
            health: if encryption_unlocked {
                ServiceHealth::Healthy
            } else {
                ServiceHealth::Stopped
            },
        },
        DaemonServiceSnapshot {
            name: "pairing-host".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "peer-monitor".to_string(),
            health: ServiceHealth::Healthy,
        },
    ];
    let state = Arc::new(RwLock::new(RuntimeState::new(initial_statuses)));

    let pairing_host = Arc::new(DaemonPairingHost::new(
        runtime.clone(),
        ctx.pairing_orchestrator.clone(),
        ctx.pairing_action_rx,
        state.clone(),
        ctx.space_access_orchestrator.clone(),
        ctx.key_slot_store.clone(),
        event_tx.clone(),
    ));

    let peer_monitor = Arc::new(PeerMonitor::new(runtime.clone(), event_tx.clone()));

    let (services, deferred_services) = {
        let mut initial: Vec<Arc<dyn DaemonService>> = vec![
            Arc::clone(&file_sync_orchestrator_worker) as Arc<dyn DaemonService>,
            Arc::clone(&pairing_host) as Arc<dyn DaemonService>,
            Arc::clone(&peer_monitor) as Arc<dyn DaemonService>,
        ];
        let mut deferred: Vec<Arc<dyn DaemonService>> = Vec::new();

        if should_defer_clipboard {
            deferred.push(Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>);
            deferred.push(Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>);
        } else {
            initial.push(Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>);
            initial.push(Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>);
        }

        if encryption_unlocked {
            initial.push(Arc::clone(&peer_discovery_worker) as Arc<dyn DaemonService>);
        } else {
            deferred.push(peer_discovery_worker);
        }

        (initial, deferred)
    };
    let deferred_notify_opt = if deferred_services.is_empty() {
        None
    } else {
        Some(deferred_ready_notify.clone())
    };

    let daemon = DaemonApp::new_with_deferred(
        services,
        runtime,
        state,
        event_tx,
        Some(pairing_host),
        Some(ctx.space_access_orchestrator),
        encryption_unlocked,
        deferred_services,
        deferred_notify_opt,
        external_shutdown,
        Some(clipboard_capture_gate),
    );

    rt.block_on(daemon.run())
}
