//! UniClipboard daemon binary entry point.
//!
//! Bootstraps via `build_daemon_app()` for config/paths, creates placeholder
//! workers, and runs `DaemonApp` in a tokio runtime.

use std::sync::Arc;

use uc_bootstrap::builders::build_daemon_app;
use uc_daemon::app::DaemonApp;
use uc_daemon::worker::DaemonWorker;
use uc_daemon::workers::clipboard_watcher::ClipboardWatcherWorker;
use uc_daemon::workers::peer_discovery::PeerDiscoveryWorker;

fn main() -> anyhow::Result<()> {
    // build_daemon_app() calls build_core() which inits tracing + wires deps.
    // Safe to call outside tokio (no internal block_on in daemon path).
    let ctx = build_daemon_app()?;

    // Socket path: {app_data_root}/uniclipboard-daemon.sock
    let socket_path = ctx
        .storage_paths
        .app_data_root
        .join("uniclipboard-daemon.sock");

    // Create workers (Arc-wrapped for tokio::spawn compatibility)
    let workers: Vec<Arc<dyn DaemonWorker>> = vec![
        Arc::new(ClipboardWatcherWorker),
        Arc::new(PeerDiscoveryWorker),
    ];

    // Create and run daemon app
    let daemon = DaemonApp::new(workers, socket_path);

    // Use explicit runtime construction (consistent with uc-bootstrap pattern,
    // avoids potential conflicts with tracing init's internal runtime for Seq)
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    rt.block_on(daemon.run())
}
