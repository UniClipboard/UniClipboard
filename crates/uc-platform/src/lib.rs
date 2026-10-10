//! # uc-platform
//!
//! Platform-specific implementations for UniClipboard.
//!
//! This crate contains infrastructure implementations that interact with
//! the operating system, external services, and hardware.

// Tracing support for platform layer instrumentation
pub use tracing;

/// Resolve the runtime profile through the directory-layout authority.
/// Data directories and keychain services use the same profile suffix.
pub(crate) fn resolve_profile() -> Option<String> {
    uc_app_paths::resolve_profile(None)
}

pub mod app_dirs;
pub mod bootstrap;
pub mod capability;
pub mod clipboard;
pub mod file_secure_storage;
pub mod portable;
pub mod ports;
pub mod secure_storage;
pub mod system_secure_storage;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
pub mod system_wake;
