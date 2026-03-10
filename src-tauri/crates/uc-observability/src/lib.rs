//! UniClipboard Observability Crate
//!
//! Provides dual-output tracing initialization with profile-based filtering:
//! - Pretty console output for developer experience
//! - Structured JSON file output for tooling and analysis
//!
//! # Public API
//!
//! - [`LogProfile`] - Logging profile enum (`Dev`, `Prod`, `DebugClipboard`)
//! - [`init_tracing_subscriber`] - Initialize dual-output tracing subscriber
//!
//! # Usage
//!
//! ```ignore
//! use uc_observability::{init_tracing_subscriber, LogProfile};
//! use std::path::Path;
//!
//! let profile = LogProfile::from_env();
//! init_tracing_subscriber(Path::new("/var/log/myapp"), profile)?;
//! ```

pub mod format;
mod init;
pub mod profile;

pub use init::init_tracing_subscriber;
pub use profile::LogProfile;
