//! # uc-platform
//!
//! Platform-specific implementations for UniClipboard.
//!
//! This crate contains infrastructure implementations that interact with
//! the operating system, external services, and hardware.

// Tracing support for platform layer instrumentation
pub use tracing;

pub mod adapters;
pub mod app_dirs;
pub mod bootstrap;
pub mod capability;
pub mod clipboard;
pub mod file_secure_storage;
pub mod identity_store;
pub mod ipc;
pub mod key_scope;
pub mod net_utils;
pub mod ports;
pub mod runtime;
pub mod secure_storage;
pub mod system_secure_storage;
pub mod test_support;
