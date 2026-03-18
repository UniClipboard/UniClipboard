//! # Non-GUI Runtime Helpers
//!
//! Provides [`LoggingHostEventEmitter`] and [`build_non_gui_runtime()`] for
//! constructing a [`CoreRuntime`] in non-GUI entry points (daemon, CLI).
//!
//! [`LoggingHostEventEmitter`] logs event type names via `tracing::debug!`
//! without printing inner payloads (which may contain sensitive data like
//! clipboard content, pairing codes, or file paths).

use std::sync::Arc;

use uc_app::app_paths::AppPaths;
use uc_app::runtime::CoreRuntime;
use uc_app::task_registry::TaskRegistry;
use uc_app::usecases::{InMemoryLifecycleStatus, LoggingSessionReadyEmitter, SessionReadyEmitter};
use uc_app::AppDeps;
use uc_core::clipboard::ClipboardIntegrationMode;
use uc_core::ports::host_event_emitter::{EmitError, HostEvent, HostEventEmitterPort};
use uc_platform::ports::WatcherControlPort;

use crate::assembly::{build_setup_orchestrator, SetupAssemblyPorts};

// ---------------------------------------------------------------------------
// LoggingHostEventEmitter
// ---------------------------------------------------------------------------

/// Event emitter that logs event type names via `tracing::debug!`.
///
/// Always returns `Ok(())` — infallible by design. Inner event payloads are
/// NOT logged because they may contain sensitive data (clipboard content,
/// pairing codes/fingerprints, transfer file paths).
pub struct LoggingHostEventEmitter;

impl HostEventEmitterPort for LoggingHostEventEmitter {
    fn emit(&self, event: HostEvent) -> Result<(), EmitError> {
        match event {
            HostEvent::Clipboard(_) => {
                tracing::debug!(event_type = "clipboard", "host event (non-gui)");
            }
            HostEvent::PeerDiscovery(_) => {
                tracing::debug!(event_type = "peer_discovery", "host event (non-gui)");
            }
            HostEvent::PeerConnection(_) => {
                tracing::debug!(event_type = "peer_connection", "host event (non-gui)");
            }
            HostEvent::Transfer(_) => {
                tracing::debug!(event_type = "transfer", "host event (non-gui)");
            }
            HostEvent::Pairing(_) => {
                tracing::debug!(event_type = "pairing", "host event (non-gui)");
            }
            HostEvent::Setup(_) => {
                tracing::debug!(event_type = "setup", "host event (non-gui)");
            }
            HostEvent::SpaceAccess(_) => {
                tracing::debug!(event_type = "space_access", "host event (non-gui)");
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// NoopWatcherControl (for non-GUI modes)
// ---------------------------------------------------------------------------

/// No-op watcher control for non-GUI runtimes.
///
/// Daemon/CLI placeholder workers manage clipboard watching independently
/// through the DaemonWorker trait, so the setup orchestrator's watcher control
/// is unused.
struct NoopWatcherControl;

#[async_trait::async_trait]
impl WatcherControlPort for NoopWatcherControl {
    async fn start_watcher(&self) -> Result<(), uc_platform::ports::WatcherControlError> {
        Ok(())
    }
    async fn stop_watcher(&self) -> Result<(), uc_platform::ports::WatcherControlError> {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// build_non_gui_runtime
// ---------------------------------------------------------------------------

/// Construct a [`CoreRuntime`] for non-GUI entry points (daemon, CLI).
///
/// Uses [`LoggingHostEventEmitter`] as the permanent emitter (no swap needed
/// in non-GUI modes), `InMemoryLifecycleStatus`, and
/// `ClipboardIntegrationMode::Passive`.
///
/// # Arguments
///
/// * `deps` — Pre-wired application dependencies from `wire_dependencies()`.
/// * `storage_paths` — Resolved storage paths (caller resolves via
///   `get_storage_paths(&config)` before calling this function).
pub fn build_non_gui_runtime(
    deps: AppDeps,
    storage_paths: AppPaths,
) -> anyhow::Result<CoreRuntime> {
    let emitter: Arc<dyn HostEventEmitterPort> = Arc::new(LoggingHostEventEmitter);
    let emitter_cell = Arc::new(std::sync::RwLock::new(emitter));

    let lifecycle_status = Arc::new(InMemoryLifecycleStatus::new());
    let task_registry = Arc::new(TaskRegistry::new());

    let setup_ports = SetupAssemblyPorts::placeholder(&deps);
    let session_ready_emitter: Arc<dyn SessionReadyEmitter> = Arc::new(LoggingSessionReadyEmitter);
    let watcher_control: Arc<dyn WatcherControlPort> = Arc::new(NoopWatcherControl);

    let setup_orchestrator = build_setup_orchestrator(
        &deps,
        setup_ports,
        lifecycle_status.clone(),
        emitter_cell.clone(),
        ClipboardIntegrationMode::Passive,
        session_ready_emitter,
        watcher_control,
    );

    Ok(CoreRuntime::new(
        deps,
        emitter_cell,
        lifecycle_status,
        setup_orchestrator,
        ClipboardIntegrationMode::Passive,
        task_registry,
        storage_paths,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::host_event_emitter::*;
    use uc_core::ports::transfer_progress::{TransferDirection, TransferProgress};
    use uc_core::setup::SetupState;

    #[test]
    fn test_logging_emitter_returns_ok() {
        let emitter = LoggingHostEventEmitter;

        let events = vec![
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "e1".to_string(),
                preview: "hello".to_string(),
                origin: ClipboardOriginKind::Local,
            }),
            HostEvent::PeerDiscovery(PeerDiscoveryHostEvent::Discovered {
                peer_id: "p1".to_string(),
                device_name: None,
                addresses: vec![],
            }),
            HostEvent::PeerConnection(PeerConnectionHostEvent::Connected {
                peer_id: "p2".to_string(),
                device_name: Some("Desk".to_string()),
            }),
            HostEvent::Transfer(TransferHostEvent::Progress(TransferProgress {
                transfer_id: "t1".to_string(),
                peer_id: "p3".to_string(),
                direction: TransferDirection::Sending,
                chunks_completed: 0,
                total_chunks: 1,
                bytes_transferred: 0,
                total_bytes: Some(100),
            })),
            HostEvent::Pairing(PairingHostEvent::Verification {
                session_id: "s1".to_string(),
                kind: PairingVerificationKind::Request,
                peer_id: None,
                device_name: None,
                code: None,
                local_fingerprint: None,
                peer_fingerprint: None,
                error: None,
            }),
            HostEvent::Setup(SetupHostEvent::StateChanged {
                state: SetupState::Welcome,
                session_id: None,
            }),
            HostEvent::SpaceAccess(SpaceAccessHostEvent::Completed {
                session_id: "sa1".to_string(),
                peer_id: "p4".to_string(),
                success: true,
                reason: None,
                ts: 0,
            }),
        ];

        for event in events {
            assert!(
                emitter.emit(event).is_ok(),
                "LoggingHostEventEmitter should always return Ok"
            );
        }
    }
}
