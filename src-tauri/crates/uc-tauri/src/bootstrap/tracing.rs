//! Tracing configuration for UniClipboard
//!
//! This module provides the tracing-subscriber initialization for structured
//! logging with spans, supporting the gradual migration from `log` crate to
//! `tracing` crate.
//!
//! ## Architecture / 架构
//!
//! - **Dual-track system**: Both `log` and `tracing` work during transition
//! - **Format compatibility**: Output format matches existing `log` format
//! - **Environment-aware**: Development uses Webview, Production uses file+stdout
//!
//! ## Migration Path / 迁移路径
//!
//! Phase 0: Infrastructure setup (this module)
//! Phase 1: Command layer creates root spans
//! Phase 2: UseCase layer creates child spans
//! Phase 3: Infra/Platform layers add debug spans
//! Phase 4: Remove `log` dependency (optional)

use std::{fs, io, sync::OnceLock};

use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};
use tracing_subscriber::{fmt, fmt::writer::BoxMakeWriter, prelude::*, registry};
use uc_app::app_paths::AppPaths;
use uc_core::ports::AppDirsPort;
use uc_platform::app_dirs::DirsAppDirsAdapter;

use sentry;
use sentry_tracing;

static LOG_GUARD: OnceLock<WorkerGuard> = OnceLock::new();
static SENTRY_GUARD: OnceLock<sentry::ClientInitGuard> = OnceLock::new();

/// Check if running in development environment
fn is_development() -> bool {
    cfg!(debug_assertions)
}

/// Build the default filter directives for tracing
///
/// ## Behavior / 行为
/// - **Development**: debug level for app, info for libp2p_mdns
/// - **Production**: info level for app, info for libp2p_mdns
/// - **mDNS**: Always set to info to see discovery events, but keep iface=off
fn build_filter_directives(is_dev: bool) -> Vec<String> {
    vec![
        if is_dev { "debug" } else { "info" }.to_string(),
        "libp2p_mdns=info".to_string(),
        "libp2p_mdns::behaviour::iface=off".to_string(),
        "tauri=warn".to_string(), // Filter noisy setup spans (app::setup)
        "wry=off".to_string(),    // Filter Tauri internal spans (custom_protocol)
        "ipc::request=off".to_string(), // Filter Tauri IPC handler spans
        if is_dev {
            "uc_platform=debug"
        } else {
            "uc_platform=info"
        }
        .to_string(),
        if is_dev {
            "uc_infra=debug"
        } else {
            "uc_infra=info"
        }
        .to_string(),
    ]
}

/// Initialize the tracing subscriber with appropriate configuration
///
/// ## Behavior / 行为
///
/// - **Development**: Debug level, outputs to stdout
/// - **Production**: Info level, outputs to stdout
/// - **Environment filter**: Respects RUST_LOG, with sensible defaults
/// - **No log bridge**: Does NOT capture `log::info!()` calls directly
///
/// ## English
///
/// This function:
/// 1. Creates an env-filter for level control
/// 2. Sets up fmt layer with log-compatible formatting
/// 3. Registers the global subscriber
///
/// ## Migration Strategy
///
/// All code should use `tracing::` macros (tracing::info!, etc.) instead of `log::` macros.
/// The `log::` macros are re-exported for compatibility but will NOT use the tracing system.
///
/// ## Call this / 调用位置
///
/// Call in `main.rs` **before** Tauri Builder setup:
///
/// ```ignore
/// fn main() {
///     // ... load config ...
///     uc_tauri::bootstrap::tracing::init_tracing_subscriber()
///         .expect("Failed to initialize tracing");
///
///     run_app(config);
/// }
/// ```
///
/// ## Errors / 错误
///
/// Returns `Err` if:
/// - Subscriber is already registered (should only call once)
/// - Invalid filter directives in RUST_LOG
pub fn init_tracing_subscriber() -> anyhow::Result<()> {
    let is_dev = is_development();

    // Step 1: Build environment filter
    // - Defaults to debug in dev, info in prod
    // - Filters libp2p_mdns warnings (noisy proxy software errors)
    // - Can be overridden with RUST_LOG environment variable
    let filter_directives = build_filter_directives(is_dev);
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(filter_directives.join(",")));

    // Step 2: Initialize Sentry
    // - Only if SENTRY_DSN is set
    // - Guard must be kept alive
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

    // Step 3: Create writers
    let stdout_writer: BoxMakeWriter = BoxMakeWriter::new(io::stdout);
    let file_writer = match build_file_writer() {
        Ok(writer) => Some(writer),
        Err(err) => {
            eprintln!("Failed to initialize file logging, falling back to stdout: {err}");
            None
        }
    };

    // Step 3: Create fmt layers (formatting)
    // Format matches existing log format for compatibility:
    // "2025-01-15 10:30:45.123 INFO [file.rs:42] [target] message"
    let stdout_layer = fmt::layer()
        .with_timer(fmt::time::ChronoUtc::new(
            "%Y-%m-%d %H:%M:%S%.3f".to_string(),
        ))
        .with_level(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_ansi(cfg!(not(test))) // Disable colors in tests
        .with_writer(stdout_writer);

    let file_layer = file_writer.map(|writer| {
        fmt::layer()
            .with_timer(fmt::time::ChronoUtc::new(
                "%Y-%m-%d %H:%M:%S%.3f".to_string(),
            ))
            .with_level(true)
            .with_file(true)
            .with_line_number(true)
            .with_target(true)
            .with_ansi(false) // No ANSI colors in file logs
            .with_writer(writer)
    });

    // Step 4: Register the global subscriber
    // This MUST be called once, before any logging occurs
    let subscriber = registry()
        .with(env_filter)
        .with(sentry_layer)
        .with(stdout_layer);

    if let Some(layer) = file_layer {
        subscriber.with(layer).try_init()?;
    } else {
        subscriber.try_init()?;
    }

    Ok(())
}

fn build_file_writer() -> anyhow::Result<NonBlocking> {
    let app_dirs = DirsAppDirsAdapter::new().get_app_dirs()?;
    let paths = AppPaths::from_app_dirs(&app_dirs);
    fs::create_dir_all(&paths.logs_dir)?;

    let file_appender = tracing_appender::rolling::never(&paths.logs_dir, "uniclipboard.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    LOG_GUARD
        .set(guard)
        .map_err(|_| anyhow::anyhow!("Tracing log guard already initialized"))?;

    Ok(non_blocking)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_init() {
        // Note: This will panic if subscriber already registered
        // In normal tests, we'd use a test subscriber instead
        // For now, just verify the function compiles
        let is_dev = is_development();
        let _ = is_dev; // Suppress unused warning
    }

    #[test]
    fn test_build_filter_directives() {
        let dev_directives = build_filter_directives(true);
        assert!(dev_directives.contains(&"debug".to_string()));
        assert!(dev_directives.contains(&"libp2p_mdns=info".to_string()));
        assert!(dev_directives.contains(&"libp2p_mdns::behaviour::iface=off".to_string()));
        assert!(dev_directives.contains(&"uc_platform=debug".to_string()));

        let prod_directives = build_filter_directives(false);
        assert!(prod_directives.contains(&"info".to_string()));
        assert!(prod_directives.contains(&"libp2p_mdns=info".to_string()));
        assert!(prod_directives.contains(&"libp2p_mdns::behaviour::iface=off".to_string()));
        assert!(prod_directives.contains(&"uc_platform=info".to_string()));
    }
}
