//! Dual-output tracing subscriber initialization.
//!
//! Composes a console (pretty) layer and a JSON file layer on a single
//! `tracing_subscriber::Registry`, with per-layer filtering from `LogProfile`.
//!
//! # Sentry Integration
//!
//! This function does NOT initialize Sentry. Sentry layer construction remains
//! the responsibility of the caller (e.g., `main.rs` or `uc-tauri`) since it
//! depends on app-level configuration (SENTRY_DSN). If Sentry integration is
//! needed, initialize it separately and add the sentry-tracing layer to the
//! subscriber before calling `try_init()`, or use sentry's own global init.

use crate::profile::LogProfile;
use std::path::Path;

/// Initialize the dual-output tracing subscriber.
///
/// Creates a registry with:
/// 1. A console layer using pretty format with ANSI colors
/// 2. A JSON file layer using `FlatJsonFormat` with daily rolling files
///
/// Both layers get independent `EnvFilter`s from the given profile.
/// If `RUST_LOG` is set, it overrides the profile filters for both layers.
///
/// The `WorkerGuard` for the JSON file writer is stored in a static `OnceLock`
/// to prevent early drop.
///
/// # Arguments
///
/// * `logs_dir` - Directory for JSON log files (e.g., `uniclipboard.json.YYYY-MM-DD`)
/// * `profile` - The `LogProfile` controlling filter verbosity
///
/// # Errors
///
/// Returns `Err` if:
/// - The global subscriber is already registered
/// - The logs directory cannot be created
pub fn init_tracing_subscriber(_logs_dir: &Path, _profile: LogProfile) -> anyhow::Result<()> {
    // Placeholder - will be implemented in Task 2
    Ok(())
}
