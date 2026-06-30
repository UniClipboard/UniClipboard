//! # uc-bootstrap — Sole Composition Root
//!
//! This crate is the single place allowed to depend on
//! uc-core + uc-application + uc-infra + uc-platform simultaneously.
//! All entry points (GUI, CLI, daemon) depend on uc-bootstrap
//! for dependency wiring and initialization.

pub mod entrypoint;
pub mod layer;
pub mod observability;
pub mod startup;
pub mod subsystem;
pub mod wiring;

// The top-level re-exports below ARE the crate's external contract: the symbols
// daemon (apps/daemon) and the CLI dev-tools feature (apps/cli) consume. Keep
// this list in sync with that contract — everything else stays crate-internal
// (`pub(crate)`), reachable only within the composition root.

// Slice 6 / Issue #549 — composition-root analytics 装配入口。
// `compose_event_context` 在 `wire_dependencies` 之后由各进程入口调用一次。
pub use subsystem::analytics::compose_event_context;

pub use entrypoint::daemon::{build_daemon_lifecycle, DaemonLifecycle};
pub use entrypoint::non_gui::{
    build_app_facade_from_deps, build_cli_app_runtime, build_mobile_sync_facade,
    resolve_clipboard_integration_mode, AppFacadeAssemblyOptions, CliAppRuntime,
    ClipboardRestoreAssembly,
};
pub use layer::paths::get_storage_paths;
pub use layer::platform::SystemClipboardWiring;
pub use observability::tracing::{init_tracing_subscriber, install_panic_logging_hook};
pub use subsystem::blob_tasks::{spawn_blob_processing_tasks, BlobProcessingPorts};
pub use subsystem::file_transfer::FileTransferLifecycle;
pub use subsystem::sync_engine::SyncEngineAssembly;
pub use wiring::deps::{BackgroundRuntimeDeps, WiredDependencies, WiringError, WiringResult};
pub use wiring::wire::wire_dependencies;
