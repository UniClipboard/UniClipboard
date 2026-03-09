//! UniClipboard Application Orchestration Layer
//!
//! This crate contains business logic use cases and runtime orchestration.

use std::sync::Arc;
use uc_core::ports::{AutostartPort, UiPort};

// Tracing support for use case instrumentation
pub use tracing;

pub mod app_paths;
pub mod deps;
pub mod models;
pub mod usecases;

pub use deps::AppDeps;

/// The application runtime.
pub struct App {
    /// Dependency grouping for direct construction
    pub deps: Option<AppDeps>,

    /// Public fields for backward compatibility
    pub autostart: Arc<dyn AutostartPort>,
    pub ui_port: Arc<dyn UiPort>,
}

impl App {
    /// Create new App instance from dependencies
    ///
    /// All dependencies must be provided - no defaults, no optionals.
    pub fn new(deps: AppDeps) -> Self {
        let (autostart, ui_port) = (deps.autostart.clone(), deps.ui_port.clone());

        Self {
            deps: Some(deps),
            autostart,
            ui_port,
        }
    }
}
