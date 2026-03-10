//! UniClipboard Observability Crate
//!
//! Provides dual-output tracing initialization with profile-based filtering:
//! - Pretty console output for developer experience
//! - Structured JSON file output for tooling and analysis
//!
//! # Public API
//!
//! - [`LogProfile`] - Logging profile enum (`Dev`, `Prod`, `DebugClipboard`)
//! - [`init_tracing_subscriber`] - Initialize dual-output tracing subscriber (standalone)
//! - [`build_console_layer`] - Build console layer for composition with other layers
//! - [`build_json_layer`] - Build JSON file layer for composition with other layers
//!
//! # Standalone Usage
//!
//! ```ignore
//! use uc_observability::{init_tracing_subscriber, LogProfile};
//! use std::path::Path;
//!
//! let profile = LogProfile::from_env();
//! init_tracing_subscriber(Path::new("/var/log/myapp"), profile)?;
//! ```
//!
//! # Composition Usage (with Sentry or other layers)
//!
//! ```ignore
//! use uc_observability::{build_console_layer, build_json_layer, LogProfile};
//! use tracing_subscriber::prelude::*;
//!
//! let profile = LogProfile::from_env();
//! let console = build_console_layer(&profile);
//! let (json, guard) = build_json_layer(logs_dir, &profile)?;
//! // Store guard somewhere to keep it alive!
//!
//! tracing_subscriber::registry()
//!     .with(sentry_layer)  // your extra layer
//!     .with(console)
//!     .with(json)
//!     .try_init()?;
//! ```

pub mod format;
mod init;
pub mod profile;

pub use init::{build_console_layer, build_json_layer, init_tracing_subscriber};
pub use profile::LogProfile;
pub use tracing_appender::non_blocking::WorkerGuard;
