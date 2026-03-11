//! Tracing configuration for UniClipboard
//!
//! Thin wrapper that composes uc-observability layer builders with the
//! application-specific Sentry layer, then registers a single global
//! tracing subscriber.
//!
//! ## Architecture
//!
//! - **uc-observability** provides `build_console_layer` + `build_json_layer`
//!   (profile-driven, dual-output: pretty console + flat JSON file) and
//!   `build_seq_layer` (optional CLEF ingestion to a local Seq instance)
//! - **This module** adds the Sentry layer on top, optionally wires Seq, and
//!   registers the composed subscriber via `try_init()`
//!
//! ## Call Site
//!
//! Call `init_tracing_subscriber()` in `main.rs` **before** Tauri Builder setup.

use std::sync::OnceLock;

use tracing_subscriber::prelude::*;
use uc_app::app_paths::AppPaths;
use uc_observability::{LogProfile, SeqGuard, WorkerGuard};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static SEQ_GUARD: OnceLock<SeqGuard> = OnceLock::new();
/// Dedicated tokio runtime for the Seq background sender task.
/// Needed because `init_tracing_subscriber` runs before Tauri's async runtime
/// is available. The runtime is kept alive as long as this static exists.
static SEQ_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

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

    // Step 4b: Build Seq layer (if UC_SEQ_URL is set)
    // build_seq_layer uses tokio::spawn internally, so we need a runtime.
    // Since this runs before Tauri's async runtime, we create a dedicated one.
    let seq_enabled;
    let seq_layer = if std::env::var("UC_SEQ_URL").is_ok() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        let layer_result = rt.block_on(async { uc_observability::build_seq_layer(&profile) });

        match layer_result {
            Some((layer, guard)) => {
                seq_enabled = true;
                if SEQ_GUARD.set(guard).is_err() {
                    eprintln!("Seq guard already initialized");
                }
                // Keep the runtime alive so the background sender task continues
                if SEQ_RUNTIME.set(rt).is_err() {
                    eprintln!("Seq runtime already initialized");
                }
                Some(layer)
            }
            None => {
                seq_enabled = false;
                None
            }
        }
    } else {
        seq_enabled = false;
        None
    };

    // Step 5: Compose all layers and register
    tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .with(seq_layer)
        .try_init()?;

    tracing::info!(
        profile = %profile,
        logs_dir = %paths.logs_dir.display(),
        seq_enabled = seq_enabled,
        "Tracing initialized with dual output (console + JSON{})",
        if seq_enabled { " + Seq" } else { "" }
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
