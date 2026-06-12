//! # uc-bootstrap — Sole Composition Root
//!
//! This crate is the single place allowed to depend on
//! uc-core + uc-app + uc-infra + uc-platform simultaneously.
//! All entry points (GUI, CLI, daemon) depend on uc-bootstrap
//! for dependency wiring and initialization.

pub mod analytics;
pub mod assembly;
pub mod background_tasks;
pub mod builders;
pub mod config;
mod correlation;
pub mod file_transfer_lifecycle;
pub mod init;
mod network_policy;
pub mod non_gui_runtime;
pub mod space_setup;
pub mod task_registry;
pub mod tracing;

pub use task_registry::TaskRegistry;

// Slice 6 / Issue #549 — composition-root analytics 装配入口。
// 详见模块 doc。`build_core` 在 `wire_dependencies` 之后调用一次。
// `build_analytics_sink` 在 `wire_dependencies` 内被 AppDeps 构造点调用，
// 装配 GatedAnalyticsSink 包装的 dev/release sink。
pub use analytics::{build_analytics_sink, compose_event_context};

// Re-export primary public items
pub use assembly::{
    build_clipboard_write_coordinator, build_gui_client_context, get_storage_paths,
    resolve_pairing_device_name, wire_dependencies, wire_gui_client_deps, BackgroundRuntimeDeps,
    GuiClientDeps, SystemClipboardWiring, WiredDependencies, WiringError, WiringResult,
};
pub use background_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
pub use builders::{
    build_cli_context, build_cli_context_with_profile, build_cli_wiring_context,
    build_daemon_lifecycle, build_slice1_cli_context, CliBootstrapContext, DaemonLifecycle,
};
pub use config::load_config;
pub use init::{
    ensure_default_device_name, is_setup_complete, reconcile_peer_addresses,
    reconcile_trusted_peers,
};
pub use non_gui_runtime::{
    build_app_facade_from_deps, build_cli_app_facade, build_cli_app_runtime,
    build_mobile_sync_facade, build_non_gui_bundle, resolve_clipboard_integration_mode,
    AppFacadeAssemblyOptions, CliAppRuntime, ClipboardRestoreAssembly, LoggingHostEventEmitter,
    NonGuiBundle,
};
pub use space_setup::{
    build_space_setup_assembly, IrohNodeConfig, SpaceSetupAssembly, SpaceSetupAssemblyError,
};
pub use tracing::init_tracing_subscriber;
