//! UniClipboard Application Orchestration Layer
//!
//! This crate contains business logic use cases and runtime orchestration.

// Tracing support for use case instrumentation
pub use tracing;

pub mod app_paths;
pub mod deps;
pub mod models;
pub mod runtime;
pub mod task_registry;
pub mod testing;
pub mod usecases;

pub use deps::{
    AppDeps, ClipboardPorts, DevicePorts, NetworkPorts, SecurityPorts, StatsPorts, StoragePorts,
    SystemPorts,
};
pub use runtime::CoreRuntime;
pub use usecases::CoreUseCases;

/// The application runtime.
pub struct App {
    /// Dependency grouping for direct construction
    pub deps: Option<AppDeps>,
}

impl App {
    /// Create new App instance from dependencies
    ///
    /// All dependencies must be provided - no defaults, no optionals.
    pub fn new(deps: AppDeps) -> Self {
        Self { deps: Some(deps) }
    }
}
