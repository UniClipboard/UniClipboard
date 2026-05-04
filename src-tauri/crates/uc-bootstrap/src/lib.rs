//! # uc-bootstrap — Sole Composition Root
//!
//! This crate is the single place allowed to depend on
//! uc-core + uc-app + uc-infra + uc-platform simultaneously.
//! All entry points (GUI, CLI, daemon) depend on uc-bootstrap
//! for dependency wiring and initialization.

pub mod assembly;
pub mod background_tasks;
pub mod builders;
pub mod config;
pub mod file_transfer_lifecycle;
pub mod init;
mod network_policy;
pub mod non_gui_runtime;
pub mod space_setup;
pub mod task_registry;
pub mod tracing;

pub use task_registry::TaskRegistry;

// Re-export primary public items
pub use assembly::{
    build_clipboard_write_coordinator, get_storage_paths, resolve_pairing_device_name,
    wire_dependencies, BackgroundRuntimeDeps, WiredDependencies, WiringError, WiringResult,
};
pub use background_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
pub use builders::{
    build_cli_context, build_cli_context_with_profile, build_daemon_app, build_slice1_cli_context,
    CliBootstrapContext, DaemonBootstrapContext,
};
pub use config::load_config;
pub use init::{
    ensure_default_device_name, is_setup_complete, reconcile_peer_addresses,
    reconcile_trusted_peers,
};
pub use non_gui_runtime::{
    build_app_facade_from_deps, build_cli_app_facade, build_cli_app_runtime, build_non_gui_bundle,
    resolve_clipboard_integration_mode, AppFacadeAssemblyOptions, CliAppRuntime,
    ClipboardRestoreAssembly, LoggingHostEventEmitter, NonGuiBundle,
};
pub use space_setup::{
    build_space_setup_assembly, IrohNodeConfig, SpaceSetupAssembly, SpaceSetupAssemblyError,
};
pub use tracing::init_tracing_subscriber;
