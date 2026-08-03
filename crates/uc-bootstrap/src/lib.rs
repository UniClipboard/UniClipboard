//! # uc-bootstrap — Desktop Host Preparation
//!
//! Prepares desktop paths, platform adapters, observability, and host
//! capabilities consumed by `uc-engine`.

pub mod entrypoint;
pub mod layer;
pub mod observability;
pub mod wiring;

// The top-level re-exports below ARE the crate's external contract: the symbols
// daemon (apps/daemon) and the CLI dev-tools feature (apps/cli) consume. Keep
// this list in sync with that contract — everything else stays crate-internal
// (`pub(crate)`), reachable only within the composition root.

pub use entrypoint::cli::{build_cli_engine_runtime, CliEngineRuntime};
pub use layer::platform::SystemClipboardWiring;
pub use observability::tracing::{init_tracing_subscriber, install_panic_logging_hook};
pub use wiring::analytics::{initialize_analytics_context, DesktopHostAnalytics};
pub use wiring::desktop_host::{
    prepare_desktop_engine_host, DesktopEngineHost, DesktopHostFileHandles, DesktopHostProcessPaths,
};
pub use wiring::error::{WiringError, WiringResult};
