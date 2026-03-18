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
//! ## Idempotency
//!
//! `init_tracing_subscriber()` can be called multiple times safely.
//! Only the first call initializes the subscriber; subsequent calls return `Ok(())`.
//!
//! ## Call Site
//!
//! Call `init_tracing_subscriber()` in `main.rs` **before** Tauri Builder setup.

use std::path::Path;
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

/// Guard that ensures tracing is initialized exactly once across all entry points.
static TRACING_INITIALIZED: OnceLock<()> = OnceLock::new();

/// Resolve device_id from config directory for logging correlation.
///
/// Reads device identifier from `{config_dir}/device_id.txt` if it exists.
/// Returns `None` if the file doesn't exist (first launch graceful degradation).
fn resolve_device_id_for_logging(config_dir: &Path) -> Option<String> {
    let device_id_path = config_dir.join("device_id.txt");
    std::fs::read_to_string(&device_id_path)
        .ok()?
        .trim()
        .to_string()
        .into()
}

/// Initialize the tracing subscriber with dual-output and optional Sentry.
///
/// ## Idempotency
///
/// This function is idempotent. If called more than once, subsequent calls
/// return `Ok(())` without modifying the global subscriber.
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
/// - The global subscriber is already registered (and this is the first call)
/// - The logs directory cannot be created
pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    // Idempotency guard: skip if already initialized
    if TRACING_INITIALIZED.get().is_some() {
        ::tracing::debug!("Tracing already initialized, skipping");
        return Ok(());
    }

    // Step 1: Resolve logs directory
    let app_dirs = DirsAppDirsAdapter::new().get_app_dirs()?;
    let paths = AppPaths::from_app_dirs(&app_dirs);
    std::fs::create_dir_all(&paths.logs_dir)?;

    // Step 1b: Resolve device_id for process-wide logging correlation
    let device_id = resolve_device_id_for_logging(&app_dirs.app_data_root);
    if let Some(device_id) = device_id.as_ref() {
        let _ = uc_observability::set_global_device_id(device_id.clone());
    }

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
        ::tracing::debug!("JSON log guard already initialized — skipping");
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

        let layer_result = rt
            .block_on(async { uc_observability::build_seq_layer(&profile, device_id.as_deref()) });

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
    match tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .with(seq_layer)
        .try_init()
    {
        Ok(()) => {}
        Err(e) => {
            // [Codex Review R1+R2] Only swallow on genuine re-entry (TRACING_INITIALIZED already set).
            // If this is the first call and try_init() fails, propagate the error.
            if TRACING_INITIALIZED.get().is_some() {
                ::tracing::warn!("Tracing subscriber already set ({}), skipping re-init", e);
                return Ok(());
            } else {
                return Err(anyhow::anyhow!(
                    "Failed to initialize tracing subscriber: {}",
                    e
                ));
            }
        }
    }

    let _ = TRACING_INITIALIZED.set(());

    ::tracing::info!(
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
