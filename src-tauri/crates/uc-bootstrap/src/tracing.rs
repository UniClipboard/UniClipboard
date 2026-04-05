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
//!   `otlp::init_otlp_pipeline` (optional OTLP telemetry export, Phase 87)
//! - **This module** adds the Sentry layer on top, optionally wires OTLP, and
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
use uc_observability::{otlp::OtlpGuard, LogProfile, WorkerGuard};
use uc_platform::app_dirs::DirsAppDirsAdapter;
use uc_platform::ports::AppDirsPort;

static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
/// Keeps the OTLP TracerProvider alive for the lifetime of the process.
///
/// Stored behind a `ManuallyDrop` inside the `OnceLock` so that the guard is
/// NEVER dropped, even if `set` were to fail (which would otherwise trigger
/// `provider.shutdown()` and poison the shared inner state of every clone held
/// by the registered `tracing_subscriber` layer — producing the infamous
/// "Spans are being emitted even after Shutdown" warning). Static globals are
/// not dropped at program exit, so wrapping in `ManuallyDrop` loses nothing.
static OTLP_GUARD: OnceLock<std::mem::ManuallyDrop<OtlpGuard>> = OnceLock::new();

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
        // No eprintln here -- it pollutes CLI output. The absence of Sentry
        // is a normal condition and will be visible in the JSON log file via
        // the tracing::info! at the end of initialization.
        None
    };

    // Step 4: Build layers from uc-observability
    let console_layer = uc_observability::build_console_layer(&profile);
    let (json_layer, guard) = uc_observability::build_json_layer(&paths.logs_dir, &profile)?;

    // Store WorkerGuard to keep non-blocking writer alive
    if JSON_GUARD.set(guard).is_err() {
        ::tracing::debug!("JSON log guard already initialized — skipping");
    }

    // Step 4b: Optionally initialize OTLP provider (phase 1 of 2).
    //
    // `init_otlp_provider` is fully synchronous — the underlying HTTP client
    // is `reqwest::blocking::Client`, which manages its own internal tokio
    // runtime. No outer tokio runtime is required here, and spans are
    // exported from opentelemetry_sdk's own background std::thread
    // (not a tokio task), so the provider is fully self-contained.
    //
    // Provider initialization is separated from layer creation so that the
    // layer can be built with the correct generic subscriber type `S`
    // (determined by the full `.with()` composition in Step 5, not at
    // provider-init time). `SdkTracerProvider::clone()` uses Arc semantics.
    let otlp_provider_and_guard = if matches!(profile, LogProfile::Prod) {
        None
    } else if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok() {
        match uc_observability::otlp::init_otlp_provider(&profile, device_id.as_deref()) {
            Ok(Some((provider, guard))) => {
                // Wrap the guard in ManuallyDrop before handing it to the
                // OnceLock. If `set` ever fails (it shouldn't — idempotency
                // guard above ensures single-init), ManuallyDrop prevents a
                // stray drop from calling `provider.shutdown()` and poisoning
                // the layer's cloned provider handle.
                if OTLP_GUARD.set(std::mem::ManuallyDrop::new(guard)).is_err() {
                    eprintln!("[uc-bootstrap] OTLP guard already initialized; leaking new guard");
                }
                Some(provider)
            }
            Ok(None) => None,
            Err(e) => {
                // Log to stderr — the global subscriber isn't set yet.
                eprintln!("[uc-bootstrap] failed to initialize OTLP provider ({e}); continuing without it");
                None
            }
        }
    } else {
        None
    };

    let otlp_enabled = otlp_provider_and_guard.is_some();

    // Step 5: Compose all layers and register.
    //
    // Phase 2 of OTLP init: build the typed layer now that the subscriber type `S`
    // is fixed by the `.with()` chain below.
    //
    // `OtlpConcreteLayer<S>` is a concrete type alias for
    // `Filtered<OpenTelemetryLayer<S, SdkTracer>, EnvFilter, S>`.
    // Using a concrete type (not `impl Layer<S>`) allows Rust to infer `S`
    // from the `.with(otlp_layer)` call site rather than requiring it to be
    // fixed at the `let` binding site.
    let otlp_layer: Option<uc_observability::otlp::layer::OtlpConcreteLayer<_>> =
        otlp_provider_and_guard
            .as_ref()
            .map(|provider| uc_observability::otlp::layer::build_otlp_layer(provider, &profile));

    match tracing_subscriber::registry()
        .with(sentry_layer)
        .with(console_layer)
        .with(json_layer)
        .with(otlp_layer)
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
        otlp_enabled = otlp_enabled,
        "Tracing initialized with dual output (console + JSON{})",
        if otlp_enabled { " + OTLP" } else { "" }
    );

    // Legacy env var migration warning (D-14, REQ-87-10).
    // Emitted through the now-initialized subscriber for structured capture.
    if std::env::var("UC_SEQ_URL").is_ok() {
        ::tracing::warn!(
            "UC_SEQ_URL is set but legacy Seq ingestion was removed in Phase 87. \
             Migrate to OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:5341/ingest/otlp"
        );
    }

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
