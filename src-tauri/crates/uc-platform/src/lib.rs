//! # uc-platform
//!
//! Platform-specific implementations for UniClipboard.
//!
//! This crate contains infrastructure implementations that interact with
//! the operating system, external services, and hardware.

// Tracing support for platform layer instrumentation
pub use tracing;

/// Provides a compile-time default profile for platform builds.
///
/// This function returns a fallback profile used only when the `UC_PROFILE` environment
/// variable is not set; runtime configuration always takes precedence. It is intended
/// to separate development build data (for example, data directories or system keychain
/// namespaces) from production installs to avoid interference.
///
/// # Returns
///
/// `Some("dev")` when the `dev-profile` feature is enabled, `None` otherwise.
///
/// # Examples
///
/// ```
/// // When built with `--features dev-profile`, this yields `Some("dev")`.
/// let profile = uc_platform::default_profile();
/// match profile {
///     Some("dev") => (),
///     Some(_) | None => (),
/// }
/// ```
#[inline]
pub const fn default_profile() -> Option<&'static str> {
    #[cfg(feature = "dev-profile")]
    {
        Some("dev")
    }
    #[cfg(not(feature = "dev-profile"))]
    {
        None
    }
}

/// Resolve the active profile name (single source of truth for `app_dirs` + `system_secure_storage`).
///
/// Runtime `UC_PROFILE` takes precedence over the compile-time `default_profile()` fallback.
/// Returns `None` when neither is set.
pub(crate) fn resolve_profile() -> Option<String> {
    if let Ok(profile) = std::env::var("UC_PROFILE") {
        if !profile.is_empty() {
            return Some(profile);
        }
    }
    default_profile().map(str::to_string)
}

pub mod app_dirs;
pub mod bootstrap;
pub mod capability;
pub mod clipboard;
pub mod file_secure_storage;
pub mod migrating_secure_storage;
pub mod ports;
pub mod secure_storage;
pub mod system_secure_storage;
pub mod usecases;
