//! Tracing configuration for UniClipboard
//!
//! Thin wrapper that composes uc-observability layer builders with the
//! application-specific Sentry layer, then registers a single global
//! tracing subscriber.
//!
//! ## Architecture
//!
//! - **uc-observability** provides `build_console_layer` + `build_json_layer`
//!   (profile-driven, dual-output: pretty console + flat JSON file)
//! - **This module** adds the Sentry layer on top and registers the composed
//!   subscriber via `try_init()`
//!
//! ## Call Site
//!
//! Call `init_tracing_subscriber()` in `main.rs` **before** Tauri Builder setup.

use std::sync::OnceLock;

use tracing_subscriber::prelude::*;
use uc_app::app_paths::AppPaths;
use uc_observability::{LogProfile, WorkerGuard};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialize the tracing subscriber with dual-output and optional Sentry.
///
/// ## Behavior
///
/// 1. Resolves log directory from platform app-dirs
/// 2. Selects [`LogProfile`] from `UC_LOG_PROFILE` env var (or build-type default)
/// 3. Initializes Sentry if `SENTRY_DSN` is set
/// 4. Builds console + JSON layers via `uc_observability`
/// 5. Composes all layers on a `Registry` and registers globally
///
/// ## Errors
///
/// Returns `Err` if:
/// - Platform app-dirs cannot be resolved
/// - The global subscriber is already registered
/// - The logs directory cannot be created
pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    // Step 1: Resolve logs directory
    let app_dirs = DirsAppDirsAdapter::new().get_app_dirs()?;
    let paths = AppPaths::from_app_dirs(&app_dirs);
    std::fs::create_dir_all(&paths.logs_dir)?;

    // Step 2: Select log profile
    let profile = LogProfile::from_env();

    // Step 3: Initialize Sentry (if SENTRY_DSN is set)
    let sentry_layer = if let Ok(dsn) = std::env::var("SENTRY_DSN") {
        let guard = sentry::init((
            dsn,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                traces_sample_rate: 1.0,
                ..Default::default()
            },
        ));

        if SENTRY_GUARD.set(guard).is_err() {
            eprintln!("Sentry guard already initialized");
        }

        Some(sentry_tracing::layer())
    } else {
        eprintln!("Sentry DSN not set, disabling Sentry");
        None
    };

    // Step 4: Build layers from uc-observability
    let console_layer = uc_observability::build_console_layer(&profile);
    let (json_layer, guard) = uc_observability::build_json_layer(&paths.logs_dir, &profile)?;

    // Store WorkerGuard to keep non-blocking writer alive
    if JSON_GUARD.set(guard).is_err() {
        anyhow::bail!("JSON log guard already initialized");
    }

    // Step 5: Compose all layers and register
    tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .try_init()?;

    tracing::info!(
        profile = %profile,
        logs_dir = %paths.logs_dir.display(),
        "Tracing initialized with dual output (console + JSON)"
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_init_compiles() {
        // Verify the function compiles with the expected no-arg signature
        let _: fn() -> anyhow::Result<()> = init_tracing_subscriber;
    }

    #[test]
    fn test_log_profile_from_env_works() {
        // Verify we can resolve a profile without panicking
        let _profile = LogProfile::from_env();
    }
}
