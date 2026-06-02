//! Process-level runtime assembly (shared by GUI shell and standalone daemon).
//!
//! Produces [`ProcessRuntimeContext`] — the one-shot process-level context that
//! callers feed into their own runtime (`TauriAppRuntime` / `DaemonProcessRuntime`).
//! Assembly itself is done by the [`uc_bootstrap`] composition root (tracing init,
//! panic hook, `wire_dependencies`, `get_storage_paths`).
//!
//! Covers **process-level one-shot resources**: sqlite pool / repos / settings /
//! secure storage / blob store / clipboard write coordinator / spool & blob
//! worker receivers. They are built once at process start and survive daemon
//! reloads (daemon-lifecycle assembly lives in [`super::bootstrap`]).
//!
//! Migrated from `uc-desktop/src/bootstrap.rs` (ADR-008 P2, Slice 2a).

use uc_application::facade::AppPaths;
use uc_bootstrap::assembly::{get_storage_paths, wire_dependencies, WiredDependencies};
use uc_bootstrap::tracing::install_panic_logging_hook;
use uc_bootstrap::{compose_event_context, init_tracing_subscriber, BackgroundRuntimeDeps};
use uc_core::config::AppConfig;

/// Process-level runtime assembly output. Callers:
///
/// - clone `wired.deps` to assemble their own runtime and pass the same wired
///   bundle to the in-process daemon spawn so daemon-lifecycle assembly reuses
///   the same sqlite pool / repos / adapters.
/// - use `background` + `BlobProcessingPorts::from_app_deps(&wired.deps)` to
///   spawn one-shot spool/blob workers (on the runtime's task_registry).
/// - read `storage_paths` / `config` for startup-time paths & config.
pub struct ProcessRuntimeContext {
    pub wired: WiredDependencies,
    pub background: BackgroundRuntimeDeps,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
}

/// Build the process-level runtime context. Used by both GUI shell and
/// standalone daemon binary.
///
/// Steps:
/// 1. Idempotent tracing subscriber init
/// 2. Panic logging hook install (idempotent)
/// 3. Parse `AppConfig`
/// 4. Wire all deps via [`wire_dependencies`]
/// 5. Resolve `AppPaths`
/// 6. Compose & register process-level product analytics `EventContext`
///
/// Daemon-lifecycle resources (iroh node / space_setup / HTTP server / LAN
/// listener / PID file) are NOT assembled here — those go through
/// [`super::bootstrap::build_daemon_bootstrap_assembly`] and are rebuilt on
/// each daemon start/stop cycle.
pub async fn build_process_runtime() -> anyhow::Result<ProcessRuntimeContext> {
    init_tracing_subscriber()?;
    install_panic_logging_hook();

    let config = AppConfig::empty();

    let (wired, background) = wire_dependencies(&config)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;
    let storage_paths = get_storage_paths(&config)?;

    if let Err(err) = compose_event_context(&wired.deps, &storage_paths).await {
        tracing::warn!(
            error = %err,
            "analytics: compose_event_context failed at process startup; \
             event sink will lack EventContext snapshot for this process"
        );
    }

    Ok(ProcessRuntimeContext {
        wired,
        background,
        storage_paths,
        config,
    })
}
