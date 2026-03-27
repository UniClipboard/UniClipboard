//! UniClipboard daemon binary entry point.
//!
//! Bootstraps via `build_daemon_app()` for config/paths, creates services,
//! and runs `DaemonApp` in a tokio runtime.
//!
//! This is the composition root: typed services are built here, then erased
//! to `Arc<dyn DaemonService>` for uniform lifecycle management by DaemonApp.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};
use tokio_util::sync::CancellationToken;
use uc_app::usecases::LoggingLifecycleEventEmitter;
use uc_app::usecases::SessionReadyEmitter;
use uc_bootstrap::assembly::SetupAssemblyPorts;
use uc_bootstrap::build_non_gui_runtime_with_emitter;
use uc_bootstrap::builders::build_daemon_app;
use uc_core::ports::SystemClipboardPort;
use uc_daemon::api::types::DaemonWsEvent;
use uc_daemon::app::{DaemonApp, SetupCompletionEmitter};
use uc_daemon::pairing::host::DaemonPairingHost;
use uc_daemon::peers::monitor::PeerMonitor;
use uc_daemon::service::DaemonService;
use uc_daemon::service::ServiceHealth;
use uc_daemon::socket::resolve_daemon_socket_path;
use uc_daemon::state::{DaemonServiceSnapshot, RuntimeState};
use uc_daemon::workers::clipboard_watcher::{ClipboardWatcherWorker, DaemonClipboardChangeHandler};
use uc_daemon::workers::file_sync_orchestrator::FileSyncOrchestratorWorker;
use uc_daemon::workers::inbound_clipboard_sync::InboundClipboardSyncWorker;
use uc_daemon::workers::peer_discovery::PeerDiscoveryWorker;
use uc_infra::clipboard::InMemoryClipboardChangeOrigin;
use uc_platform::clipboard::LocalClipboard;

fn main() -> anyhow::Result<()> {
    let gui_managed = std::env::args().any(|arg| arg == "--gui-managed");

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
    // Extract file_cache_dir and file_transfer_orchestrator before ctx is consumed
    // by build_non_gui_runtime_with_emitter (which moves ctx.deps).
    let file_cache_dir = ctx.storage_paths.file_cache_dir.clone();
    let file_transfer_orchestrator = ctx.background.file_transfer_orchestrator.clone();

    // Phase 67: Create oneshot channel for deferred PeerDiscoveryWorker start.
    // SetupCompletionEmitter fires when AppLifecycleCoordinator::ensure_ready() calls
    // emit_ready() (i.e., when the setup flow completes on an uninitialized device).
    let (setup_complete_tx, setup_complete_rx) = tokio::sync::oneshot::channel::<()>();
    let setup_completion_emitter: Arc<dyn SessionReadyEmitter> =
        Arc::new(SetupCompletionEmitter::new(setup_complete_tx));

    let runtime = Arc::new(build_non_gui_runtime_with_emitter(
        ctx.deps,
        ctx.storage_paths.clone(),
        setup_ports,
        setup_completion_emitter,
    )?);

    let socket_path = resolve_daemon_socket_path();

    // 1. Create the shared broadcast channel for WebSocket events.
    //    All services that emit WS events write to this same sender.
    let (event_tx, _) = broadcast::channel::<DaemonWsEvent>(64);

    // 3. Build typed PairingHost (per D-07: construction before DaemonApp).
    //    Typed Arc is kept for DaemonApiState so HTTP routes retain typed access (PH56-04).
    // NOTE: state is built AFTER recover_encryption_session (see below), so we need
    // a temporary state here just for PairingHost construction.
    // Actually, PairingHost receives state by Arc and DaemonPairingHost::new() stores it.
    // We create state before PairingHost and update initial_statuses later — but state is
    // an Arc<RwLock<RuntimeState>> so the actual snapshot is set at construction.
    // Solution: build PairingHost before state; it takes state by Arc so we build state
    // first with placeholder, then rebuild after encryption check. HOWEVER, the simpler
    // approach: build the tokio runtime, run encryption check, THEN build state and services.

    // Build typed workers that don't depend on encryption state first.
    // 5a. Build LocalClipboard and ClipboardWatcherWorker (per D-02, D-07).
    let local_clipboard: Arc<dyn SystemClipboardPort> = Arc::new(
        LocalClipboard::new()
            .map_err(|e| anyhow::anyhow!("failed to create LocalClipboard: {}", e))?,
    );

    // Create shared clipboard change origin for write-back loop prevention.
    // This same Arc will be passed to InboundClipboardSyncWorker when inbound sync is added
    // (per D-09), so both sides share the same state for origin detection.
    let clipboard_change_origin: Arc<dyn uc_core::ports::ClipboardChangeOriginPort> =
        Arc::new(InMemoryClipboardChangeOrigin::new());

    let clipboard_change_handler = Arc::new(DaemonClipboardChangeHandler::new(
        runtime.clone(),
        event_tx.clone(),
        clipboard_change_origin.clone(),
    ));
    let clipboard_watcher = Arc::new(ClipboardWatcherWorker::new(
        local_clipboard.clone(), // clone — FileSyncOrchestratorWorker also needs this for clipboard restore
        clipboard_change_handler,
    ));

    // InboundClipboardSyncWorker receives clipboard messages from peers and applies them
    // via SyncInboundClipboardUseCase (Full mode). Emits clipboard.new_content WS events
    // only for Applied { entry_id: Some(_) } outcomes.
    // Shares clipboard_change_origin with DaemonClipboardChangeHandler to prevent write-back loops.
    let inbound_clipboard_sync = Arc::new(InboundClipboardSyncWorker::new(
        runtime.clone(),
        event_tx.clone(),
        clipboard_change_origin.clone(), // SAME Arc as DaemonClipboardChangeHandler
        Some(file_cache_dir.clone()),
        Some(file_transfer_orchestrator.clone()),
    ));

    // FileSyncOrchestratorWorker subscribes to network events for file transfer lifecycle.
    // Handles TransferProgress, FileTransferCompleted, FileTransferFailed events,
    // runs startup reconciliation, periodic timeout sweeps, and restores completed
    // files to the OS clipboard.
    let file_sync_orchestrator_worker = Arc::new(FileSyncOrchestratorWorker::new(
        file_transfer_orchestrator,
        daemon_network_events.clone(), // clone — PeerDiscoveryWorker also needs this
        local_clipboard.clone(),
        clipboard_change_origin.clone(),
        file_cache_dir,
        daemon_settings.clone(), // clone — PeerDiscoveryWorker also needs this
    ));

    // Build PeerDiscoveryWorker unconditionally (cheap to construct).
    // Whether it's included in initial services depends on encryption state (Phase 67 D-01/D-06).
    let peer_discovery_worker: Arc<dyn DaemonService> = Arc::new(PeerDiscoveryWorker::new(
        daemon_network_control,
        daemon_network_events,
        daemon_peer_directory,
        daemon_settings,
    ));

    // Use explicit runtime construction (consistent with uc-bootstrap pattern,
    // avoids potential conflicts with tracing init's internal runtime for Seq)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // Phase 67: Recover encryption session BEFORE building services.
    // The result determines whether PeerDiscoveryWorker starts immediately or is deferred (per D-01, D-02, D-06, D-07).
    let encryption_unlocked = rt.block_on(async {
        use tracing::{info_span, Instrument};
        uc_daemon::app::recover_encryption_session(&runtime)
            .instrument(info_span!("daemon.startup.recover_encryption_session"))
            .await
    })?;

    // Phase 67: Build initial_statuses AFTER encryption check so peer-discovery
    // health reflects actual state (Stopped when uninitialized, Healthy when initialized).
    let initial_statuses: Vec<DaemonServiceSnapshot> = vec![
        DaemonServiceSnapshot {
            name: "clipboard-watcher".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "inbound-clipboard-sync".to_string(),
            health: ServiceHealth::Healthy,
        },
        DaemonServiceSnapshot {
            name: "file-sync-orchestrator".to_string(),
            health: ServiceHealth::Healthy,
        },
        // Phase 67 D-01/D-06: peer-discovery is Stopped when encryption is uninitialized;
        // it will be started dynamically after setup completes.
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

    // 2. Build typed PairingHost (per D-07: construction before DaemonApp).
    //    Typed Arc is kept for DaemonApiState so HTTP routes retain typed access (PH56-04).
    let pairing_host = Arc::new(DaemonPairingHost::new(
        runtime.clone(),
        ctx.pairing_orchestrator.clone(),
        ctx.pairing_action_rx,
        state.clone(),
        ctx.space_access_orchestrator.clone(),
        ctx.key_slot_store.clone(),
        event_tx.clone(),
    ));

    // 4. Build PeerMonitor (extracted from PairingHost in Plan 02).
    let peer_monitor = Arc::new(PeerMonitor::new(runtime.clone(), event_tx.clone()));

    // Phase 67: Conditional PeerDiscoveryWorker registration (per D-01, D-06, D-11).
    // D-02/D-07: Encryption initialized and unlocked — start PeerDiscoveryWorker immediately.
    // D-01/D-06: Encryption uninitialized — defer PeerDiscoveryWorker until setup completes.
    let (services, deferred_peer_discovery, setup_complete_rx_opt) = if encryption_unlocked {
        let services: Vec<Arc<dyn DaemonService>> = vec![
            Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>,
            Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>,
            Arc::clone(&file_sync_orchestrator_worker) as Arc<dyn DaemonService>,
            Arc::clone(&peer_discovery_worker) as Arc<dyn DaemonService>,
            Arc::clone(&pairing_host) as Arc<dyn DaemonService>,
            Arc::clone(&peer_monitor) as Arc<dyn DaemonService>,
        ];
        (services, None, None)
    } else {
        let services: Vec<Arc<dyn DaemonService>> = vec![
            Arc::clone(&clipboard_watcher) as Arc<dyn DaemonService>,
            Arc::clone(&inbound_clipboard_sync) as Arc<dyn DaemonService>,
            Arc::clone(&file_sync_orchestrator_worker) as Arc<dyn DaemonService>,
            // PeerDiscoveryWorker NOT included — will start after setup completes
            Arc::clone(&pairing_host) as Arc<dyn DaemonService>,
            Arc::clone(&peer_monitor) as Arc<dyn DaemonService>,
        ];
        (
            services,
            Some(peer_discovery_worker),
            Some(setup_complete_rx),
        )
    };

    // 5. Assemble and run daemon app.
    //    api_pairing_host retains typed access for HTTP routes.
    //    space_access_orchestrator is passed for API state wiring.
    let daemon = DaemonApp::new_with_deferred(
        services,
        runtime,
        state,
        event_tx,
        Some(pairing_host),
        Some(ctx.space_access_orchestrator),
        socket_path,
        encryption_unlocked,
        deferred_peer_discovery,
        setup_complete_rx_opt,
        external_shutdown,
    );

    rt.block_on(daemon.run())
}
