//! # uc-bootstrap — Sole Composition Root
//!
//! This crate is the single place allowed to depend on
//! uc-core + uc-app + uc-infra + uc-platform simultaneously.
//! All entry points (GUI, CLI, daemon) depend on uc-bootstrap
//! for dependency wiring and initialization.

pub mod assembly;
pub mod builders;
pub mod config;
pub mod config_resolution;
pub mod init;
pub mod non_gui_runtime;
pub mod tracing;

// Re-export primary public items
pub use assembly::{
    build_setup_orchestrator, get_storage_paths, resolve_pairing_config,
    resolve_pairing_device_name, wire_dependencies, wire_dependencies_with_identity_store,
    BackgroundRuntimeDeps, HostEventSetupPort, SetupAssemblyPorts, WiredDependencies, WiringError,
    WiringResult,
};
pub use builders::{
    build_cli_context, build_daemon_app, build_gui_app, CliBootstrapContext,
    DaemonBootstrapContext, GuiBootstrapContext,
};
pub use config::load_config;
pub use config_resolution::{resolve_app_config, resolve_config_path, ConfigResolutionError};
pub use init::ensure_default_device_name;
pub use non_gui_runtime::{build_non_gui_runtime, LoggingHostEventEmitter};
pub use tracing::init_tracing_subscriber;
