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

use crate::assembly::{build_setup_orchestrator, SetupAssemblyPorts};

#[cfg(test)]
use std::sync::{Mutex, OnceLock};

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
            HostEvent::Transfer(_) => {
                tracing::debug!(event_type = "transfer", "host event (non-gui)");
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
// build_non_gui_runtime
// ---------------------------------------------------------------------------

/// Construct a [`CoreRuntime`] for non-GUI entry points (daemon, CLI).
///
/// Uses [`LoggingHostEventEmitter`] as the permanent emitter (no swap needed
/// in non-GUI modes), `InMemoryLifecycleStatus`, and the
/// `UC_CLIPBOARD_MODE` environment override.
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
    let setup_ports = SetupAssemblyPorts::placeholder(&deps);
    build_non_gui_runtime_with_setup(deps, storage_paths, setup_ports)
}

/// Construct a [`CoreRuntime`] for non-GUI entry points with explicit setup ports.
///
/// Daemon startup uses this path so the runtime owns the real pairing/setup adapters
/// rather than the placeholder bundle used by CLI tests and fallback call sites.
pub fn build_non_gui_runtime_with_setup(
    deps: AppDeps,
    storage_paths: AppPaths,
    setup_ports: SetupAssemblyPorts,
) -> anyhow::Result<CoreRuntime> {
    let session_ready_emitter: Arc<dyn SessionReadyEmitter> = Arc::new(LoggingSessionReadyEmitter);
    build_non_gui_runtime_internal(deps, storage_paths, setup_ports, session_ready_emitter)
}

/// Construct a [`CoreRuntime`] for non-GUI entry points with explicit setup ports
/// and a custom session-ready emitter.
///
/// Daemon uses this path to inject a `SetupCompletionEmitter` that signals
/// `DaemonApp` when setup completes (Phase 67: deferred PeerDiscoveryWorker).
pub fn build_non_gui_runtime_with_emitter(
    deps: AppDeps,
    storage_paths: AppPaths,
    setup_ports: SetupAssemblyPorts,
    session_ready_emitter: Arc<dyn SessionReadyEmitter>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
) -> anyhow::Result<CoreRuntime> {
    build_non_gui_runtime_internal_with_emitter(
        deps,
        storage_paths,
        setup_ports,
        session_ready_emitter,
        emitter_cell,
    )
}

fn build_non_gui_runtime_internal(
    deps: AppDeps,
    storage_paths: AppPaths,
    setup_ports: SetupAssemblyPorts,
    session_ready_emitter: Arc<dyn SessionReadyEmitter>,
) -> anyhow::Result<CoreRuntime> {
    let emitter: Arc<dyn HostEventEmitterPort> = Arc::new(LoggingHostEventEmitter);
    let emitter_cell = Arc::new(std::sync::RwLock::new(emitter));
    build_non_gui_runtime_internal_with_emitter(
        deps,
        storage_paths,
        setup_ports,
        session_ready_emitter,
        emitter_cell,
    )
}

fn build_non_gui_runtime_internal_with_emitter(
    deps: AppDeps,
    storage_paths: AppPaths,
    setup_ports: SetupAssemblyPorts,
    session_ready_emitter: Arc<dyn SessionReadyEmitter>,
    emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
) -> anyhow::Result<CoreRuntime> {
    let lifecycle_status = Arc::new(InMemoryLifecycleStatus::new());
    let task_registry = Arc::new(TaskRegistry::new());
    let clipboard_integration_mode = resolve_clipboard_integration_mode();

    let setup_orchestrator = build_setup_orchestrator(
        &deps,
        setup_ports,
        lifecycle_status.clone(),
        emitter_cell.clone(),
        session_ready_emitter,
    );

    Ok(CoreRuntime::new(
        deps,
        emitter_cell,
        lifecycle_status,
        setup_orchestrator,
        clipboard_integration_mode,
        task_registry,
        storage_paths,
    ))
}

// ---------------------------------------------------------------------------
// build_cli_runtime
// ---------------------------------------------------------------------------

/// Construct a [`CoreRuntime`] for CLI entry points with a single function call.
///
/// This helper combines the common 4-step bootstrap sequence used by CLI commands:
/// 1. Build CLI context via `build_cli_context_with_profile()`
/// 2. Get storage paths via `get_storage_paths()`
/// 3. Build non-GUI runtime via `build_non_gui_runtime()`
///
/// Callers then create `CoreUseCases::new(&runtime)` to access use cases.
///
/// # Arguments
///
/// * `log_profile` — Log profile override (e.g., `Some(LogProfile::Cli)` or `Some(LogProfile::Dev)`)
pub fn build_cli_runtime(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CoreRuntime> {
    let ctx = crate::builders::build_cli_context_with_profile(log_profile)?;
    let storage_paths = crate::assembly::get_storage_paths(&ctx.config)?;
    let runtime = build_non_gui_runtime(ctx.deps, storage_paths)?;
    Ok(runtime)
}

/// Parse a raw string into a [`ClipboardIntegrationMode`].
///
/// Returns `Full` when `raw` is `None`, empty, or an unrecognized value.
/// Returns `Passive` only when the value is `"passive"` (case-insensitive).
pub fn parse_clipboard_integration_mode(raw: Option<&str>) -> ClipboardIntegrationMode {
    let Some(raw_value) = raw else {
        return ClipboardIntegrationMode::Full;
    };

    let normalized = raw_value.trim();
    if normalized.eq_ignore_ascii_case("passive") {
        return ClipboardIntegrationMode::Passive;
    }
    if normalized.eq_ignore_ascii_case("full") {
        return ClipboardIntegrationMode::Full;
    }

    tracing::warn!(
        uc_clipboard_mode = %raw_value,
        "Invalid UC_CLIPBOARD_MODE value; falling back to full integration"
    );
    ClipboardIntegrationMode::Full
}

/// Resolve the clipboard integration mode from the `UC_CLIPBOARD_MODE` env var.
///
/// Defaults to [`ClipboardIntegrationMode::Full`] when the variable is unset.
/// Used by both GUI and non-GUI runtimes to determine clipboard behavior.
pub fn resolve_clipboard_integration_mode() -> ClipboardIntegrationMode {
    let raw = std::env::var("UC_CLIPBOARD_MODE").ok();
    parse_clipboard_integration_mode(raw.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_core::ports::host_event_emitter::*;
    use uc_core::setup::SetupState;

    fn clipboard_mode_env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn test_logging_emitter_returns_ok() {
        let emitter = LoggingHostEventEmitter;

        let events = vec![
            HostEvent::Clipboard(ClipboardHostEvent::NewContent {
                entry_id: "e1".to_string(),
                preview: "hello".to_string(),
                origin: ClipboardOriginKind::Local,
            }),
            HostEvent::Transfer(TransferHostEvent::StatusChanged {
                transfer_id: "t1".to_string(),
                entry_id: "e2".to_string(),
                status: "pending".to_string(),
                reason: None,
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

    #[test]
    fn parse_clipboard_integration_mode_table_driven() {
        let cases = [
            (
                "none defaults to full",
                None,
                ClipboardIntegrationMode::Full,
            ),
            (
                "mixed case passive",
                Some("PaSsIvE"),
                ClipboardIntegrationMode::Passive,
            ),
            (
                "trimmed full",
                Some(" full "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "whitespace only falls back to full",
                Some("   "),
                ClipboardIntegrationMode::Full,
            ),
            (
                "invalid falls back to full",
                Some("definitely-invalid"),
                ClipboardIntegrationMode::Full,
            ),
        ];

        for (name, raw, expected) in cases {
            assert_eq!(parse_clipboard_integration_mode(raw), expected, "{name}");
        }
    }

    #[test]
    fn resolve_clipboard_integration_mode_reads_env_override() {
        let _guard = clipboard_mode_env_lock().lock().expect("env lock");
        let key = "UC_CLIPBOARD_MODE";
        let original = std::env::var(key).ok();

        std::env::set_var(key, "full");
        assert_eq!(
            resolve_clipboard_integration_mode(),
            ClipboardIntegrationMode::Full
        );

        std::env::set_var(key, "passive");
        assert_eq!(
            resolve_clipboard_integration_mode(),
            ClipboardIntegrationMode::Passive
        );

        match original {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}
