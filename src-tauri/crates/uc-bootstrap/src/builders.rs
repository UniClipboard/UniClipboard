//! # Scene-Specific Builders
//!
//! Entry-point constructors for GUI, CLI, and daemon runtime modes.
//!
//! All three builders share a private `build_core()` helper that:
//! 1. Initializes tracing (idempotent)
//! 2. Resolves application config
//! 3. Wires all dependencies via `wire_dependencies`
//!
//! Each builder returns a context struct containing `AppDeps` (NOT `CoreRuntime`).
//! Callers construct `CoreRuntime` themselves with the appropriate emitter cell,
//! lifecycle status, and task registry.
//!
//! ## Important
//!
//! `build_gui_app()` creates an internal single-threaded Tokio runtime for
//! blocking async calls (pairing config resolution). It MUST NOT be called
//! from inside an existing Tokio runtime (e.g. `#[tokio::test]`).

use std::sync::Arc;

use tokio::sync::mpsc;

use uc_app::app_paths::AppPaths;
use uc_app::usecases::space_access::SpaceAccessOrchestrator;
use uc_app::usecases::{
    DeviceAnnouncer, DeviceNameAnnouncer, LifecycleEventEmitter, LoggingLifecycleEventEmitter,
    PairingOrchestrator, StagedPairedDeviceStore,
};
use uc_app::AppDeps;
use uc_core::config::AppConfig;
use uc_core::network::pairing_state_machine::PairingAction;
use uc_core::ports::host_event_emitter::HostEventEmitterPort;
use uc_core::ports::PeerDirectoryPort;
use uc_infra::fs::key_slot_store::{JsonKeySlotStore, KeySlotStore};
use uc_platform::adapters::PairingRuntimeOwner;

use crate::assembly::{
    get_storage_paths, resolve_pairing_config, resolve_pairing_device_name, wire_dependencies,
    BackgroundRuntimeDeps, SetupAssemblyPorts,
};

/// Context for GUI entry point. Contains everything needed to construct
/// AppRuntime EXCEPT tauri::AppHandle. uc-tauri calls AppRuntime::with_setup()
/// using `deps` from this context -- NOT a prebuilt CoreRuntime.
///
/// [Codex Review R1] Returns AppDeps to preserve compatibility with
/// AppRuntime::with_setup() which builds CoreRuntime internally.
pub struct GuiBootstrapContext {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub setup_ports: SetupAssemblyPorts,
    pub storage_paths: AppPaths,
    pub pairing_orchestrator: Arc<PairingOrchestrator>,
    pub pairing_action_rx: mpsc::Receiver<PairingAction>,
    pub staged_store: Arc<StagedPairedDeviceStore>,
    pub key_slot_store: Arc<dyn KeySlotStore>,
    pub config: AppConfig,
}

/// Context for CLI entry point. AppDeps + config, no background workers.
/// Caller constructs CoreRuntime from deps as needed.
pub struct CliBootstrapContext {
    pub deps: AppDeps,
    pub config: AppConfig,
}

/// Context for daemon entry point. AppDeps + background deps,
/// workers not started. Caller constructs CoreRuntime and starts background workers.
pub struct DaemonBootstrapContext {
    pub deps: AppDeps,
    pub background: BackgroundRuntimeDeps,
    pub emitter_cell: Arc<std::sync::RwLock<Arc<dyn HostEventEmitterPort>>>,
    pub pairing_orchestrator: Arc<PairingOrchestrator>,
    pub pairing_action_rx: mpsc::Receiver<PairingAction>,
    pub staged_store: Arc<StagedPairedDeviceStore>,
    pub space_access_orchestrator: Arc<SpaceAccessOrchestrator>,
    pub key_slot_store: Arc<dyn KeySlotStore>,
    pub storage_paths: AppPaths,
    pub config: AppConfig,
}

/// Shared core wiring used by all three builders.
/// Initializes tracing, resolves config, wires dependencies.
///
/// If `log_profile_override` is `Some`, the `UC_LOG_PROFILE` env var is set
/// before tracing initialization so the subscriber picks up the desired profile.
fn build_core(
    pairing_runtime_owner: PairingRuntimeOwner,
    log_profile_override: Option<uc_observability::LogProfile>,
) -> anyhow::Result<(AppConfig, crate::assembly::WiredDependencies)> {
    // Apply log profile override before tracing init
    if let Some(profile) = log_profile_override {
        std::env::set_var("UC_LOG_PROFILE", profile.to_string());
    }

    // Idempotent -- safe to call multiple times
    crate::tracing::init_tracing_subscriber()?;

    let config = AppConfig::empty();

    let wired = wire_dependencies(&config, pairing_runtime_owner)
        .map_err(|e| anyhow::anyhow!("Dependency wiring failed: {}", e))?;

    Ok((config, wired))
}

fn gui_pairing_runtime_owner() -> PairingRuntimeOwner {
    PairingRuntimeOwner::ExternalDaemon
}

fn cli_pairing_runtime_owner() -> PairingRuntimeOwner {
    PairingRuntimeOwner::ExternalDaemon
}

fn daemon_pairing_runtime_owner() -> PairingRuntimeOwner {
    PairingRuntimeOwner::CurrentProcess
}

/// Build GUI bootstrap context. Returns raw AppDeps (not CoreRuntime) so that
/// AppRuntime::with_setup() in uc-tauri can construct CoreRuntime with the
/// correct emitter cell, lifecycle status, and task registry.
///
/// MUST be called outside any Tokio runtime (panics otherwise due to internal
/// `tokio::runtime::Builder::new_current_thread().block_on()`).
pub fn build_gui_app() -> anyhow::Result<GuiBootstrapContext> {
    let (config, wired) = build_core(gui_pairing_runtime_owner(), None)?;

    let deps = wired.deps;
    let background = wired.background;

    let pairing_device_repo = deps.device.paired_device_repo.clone();
    let pairing_device_identity = deps.device.device_identity.clone();
    let pairing_settings = deps.settings.clone();
    let discovery_network = deps.network_ports.peers.clone();
    let pairing_peer_id = background.libp2p_network.local_peer_id();
    let pairing_identity_pubkey = background.libp2p_network.local_identity_pubkey();

    // Use standalone tokio runtime (not tauri::async_runtime) -- uc-bootstrap has no tauri dep
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (pairing_device_name, pairing_config) = rt.block_on(async {
        let device_name = resolve_pairing_device_name(pairing_settings.clone()).await;
        let config = resolve_pairing_config(pairing_settings).await;
        (device_name, config)
    });

    let pairing_device_id = pairing_device_identity.current_device_id().to_string();
    let staged_store = Arc::new(StagedPairedDeviceStore::new());
    let (pairing_orchestrator, pairing_action_rx) = PairingOrchestrator::new(
        pairing_config,
        pairing_device_repo,
        pairing_device_name,
        pairing_device_id,
        pairing_peer_id,
        pairing_identity_pubkey,
        staged_store.clone(),
    );
    let pairing_orchestrator = Arc::new(pairing_orchestrator);
    let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());

    let storage_paths = get_storage_paths(&config)?;
    let key_slot_store: Arc<dyn KeySlotStore> =
        Arc::new(JsonKeySlotStore::new(storage_paths.vault_dir.clone()));

    // Create device announcer and lifecycle emitter for SetupAssemblyPorts
    let device_announcer: Option<Arc<dyn DeviceAnnouncer>> = Some(Arc::new(
        DeviceNameAnnouncer::new(deps.network_ports.peers.clone(), deps.settings.clone()),
    ));
    let lifecycle_emitter: Arc<dyn LifecycleEventEmitter> = Arc::new(LoggingLifecycleEventEmitter);

    let setup_ports = SetupAssemblyPorts::from_network(
        pairing_orchestrator.clone(),
        space_access_orchestrator.clone(),
        discovery_network,
        device_announcer,
        lifecycle_emitter,
    );

    // [Codex Review R1] Return AppDeps, NOT CoreRuntime.
    // CoreRuntime is constructed by AppRuntime::with_setup() in uc-tauri,
    // which needs to create the shared emitter cell, task registry, etc.
    Ok(GuiBootstrapContext {
        deps,
        background,
        setup_ports,
        storage_paths,
        pairing_orchestrator,
        pairing_action_rx,
        staged_store,
        key_slot_store,
        config,
    })
}

/// Build CLI bootstrap context. Returns AppDeps for the caller to construct
/// CoreRuntime as needed. No background workers are started.
pub fn build_cli_context() -> anyhow::Result<CliBootstrapContext> {
    build_cli_context_with_profile(Some(uc_observability::LogProfile::Cli))
}

/// Build CLI bootstrap context with an explicit log profile override.
///
/// When `verbose` mode is active, callers pass `Some(LogProfile::Dev)` to
/// get full console tracing. The default `build_cli_context()` uses `Cli`
/// profile which suppresses console output.
pub fn build_cli_context_with_profile(
    log_profile: Option<uc_observability::LogProfile>,
) -> anyhow::Result<CliBootstrapContext> {
    let (config, wired) = build_core(cli_pairing_runtime_owner(), log_profile)?;

    // [Codex Review R1] Return AppDeps, not CoreRuntime.
    // CLI entry point constructs CoreRuntime itself with appropriate emitter.
    Ok(CliBootstrapContext {
        deps: wired.deps,
        config,
    })
}

/// Build daemon bootstrap context. Returns AppDeps + background deps.
/// Caller constructs CoreRuntime and starts background workers.
pub fn build_daemon_app() -> anyhow::Result<DaemonBootstrapContext> {
    let (config, wired) = build_core(daemon_pairing_runtime_owner(), None)?;
    let storage_paths = get_storage_paths(&config)?;
    let deps = wired.deps;
    let background = wired.background;
    let emitter_cell = wired.emitter_cell;

    let pairing_device_repo = deps.device.paired_device_repo.clone();
    let pairing_device_identity = deps.device.device_identity.clone();
    let pairing_settings = deps.settings.clone();
    let pairing_peer_id = background.libp2p_network.local_peer_id();
    let pairing_identity_pubkey = background.libp2p_network.local_identity_pubkey();

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let (pairing_device_name, pairing_config) = rt.block_on(async {
        let device_name = resolve_pairing_device_name(pairing_settings.clone()).await;
        let config = resolve_pairing_config(pairing_settings).await;
        (device_name, config)
    });

    let pairing_device_id = pairing_device_identity.current_device_id().to_string();
    let staged_store = Arc::new(StagedPairedDeviceStore::new());
    let (pairing_orchestrator, pairing_action_rx) = PairingOrchestrator::new(
        pairing_config,
        pairing_device_repo,
        pairing_device_name,
        pairing_device_id,
        pairing_peer_id,
        pairing_identity_pubkey,
        staged_store.clone(),
    );
    let pairing_orchestrator = Arc::new(pairing_orchestrator);
    let space_access_orchestrator = Arc::new(SpaceAccessOrchestrator::new());
    let key_slot_store: Arc<dyn KeySlotStore> =
        Arc::new(JsonKeySlotStore::new(storage_paths.vault_dir.clone()));

    Ok(DaemonBootstrapContext {
        deps,
        background,
        emitter_cell,
        pairing_orchestrator,
        pairing_action_rx,
        staged_store,
        space_access_orchestrator,
        key_slot_store,
        storage_paths,
        config,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use uc_platform::adapters::PairingRuntimeOwner;

    #[test]
    fn gui_builder_uses_external_daemon_pairing_owner() {
        assert_eq!(
            gui_pairing_runtime_owner(),
            PairingRuntimeOwner::ExternalDaemon
        );
        assert_eq!(
            cli_pairing_runtime_owner(),
            PairingRuntimeOwner::ExternalDaemon
        );
    }

    #[test]
    fn daemon_builder_uses_current_process_pairing_owner() {
        assert_eq!(
            daemon_pairing_runtime_owner(),
            PairingRuntimeOwner::CurrentProcess
        );
    }
}
