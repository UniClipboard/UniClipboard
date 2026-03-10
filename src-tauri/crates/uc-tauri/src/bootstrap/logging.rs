//! Legacy logging configuration for UniClipboard (tauri-plugin-log)
//!
//! This module provides the `log::*` macro output configuration via
//! `tauri-plugin-log`. File logging is now handled by the tracing system
//! (via `uc-observability` JSON output), so this module only handles:
//!
//! - **Development**: Webview console output (browser DevTools)
//! - **Production**: Stdout output only (file output removed)
//!
//! ## Note
//!
//! Structured file logging (JSON) is provided by `uc-observability` through
//! the tracing subscriber. The `uniclipboard.log` plain-text file is no
//! longer produced. See `docs/architecture/logging-architecture.md`.

use log::LevelFilter;
use tauri_plugin_log::{Target, TargetKind, TimezoneStrategy};

/// Check if running in development environment
fn is_development() -> bool {
    // Check debug_assertions which is true in dev builds
    cfg!(debug_assertions)
}

/// Create the logging builder with appropriate configuration
///
/// ## Behavior / 行为
///
/// - Development: Debug level, Webview console output
/// - Production: Info level, file + stdout output
/// - Filters noise from libp2p_mdns and Tauri internals
/// - Color-coded output with timestamps
///
/// ## English
///
/// Configures the Tauri logging plugin based on the build environment.
/// Returns a builder that can be passed to `.plugin()` in the Tauri builder.
pub fn get_builder() -> tauri_plugin_log::Builder {
    let is_dev = is_development();
    let default_log_level = if is_dev {
        LevelFilter::Debug
    } else {
        LevelFilter::Info
    };

    let mut builder = tauri_plugin_log::Builder::new()
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .level(default_log_level)
        // Suppress libp2p-mdns iface send errors (No route to host)
        // .level_for("libp2p_mdns::behaviour::iface", LevelFilter::Off)
        // Filter libp2p_mdns ERROR logs (harmless errors from proxy software virtual interfaces)
        .level_for("libp2p_mdns", LevelFilter::Warn)
        // Filter out tauri-plugin-log's own logs to avoid infinite loops
        // Webview target sends logs via log://log events, which would trigger themselves
        .filter(move |metadata| {
            // Skip tauri internal event logs (app::emit, window::emit, etc.)
            // Skip wry noise logs (underlying WebView library)
            let is_basic_noise = metadata.target().starts_with("tauri::")
                || metadata.target().starts_with("tracing::")
                || metadata.target().contains("tauri-")
                || metadata.target().starts_with("wry::");

            if is_dev {
                // Development: Keep ipc::request logs for debugging
                !is_basic_noise
            } else {
                // Production: Filter ipc::request logs
                !is_basic_noise && !metadata.target().contains("ipc::request")
            }
        })
        .format(move |out, message, record| {
            // Format: 2025-12-29 10:30:45.123 INFO [main.rs:34] [uniclipboard] Self device already exists
            let uses_ansi = !is_dev;
            let (level_color, reset) = if uses_ansi {
                (
                    match record.level() {
                        log::Level::Error => "\x1b[31;1m", // Bold red
                        log::Level::Warn => "\x1b[33m",    // Yellow
                        log::Level::Info => "\x1b[32m",    // Green
                        log::Level::Debug => "\x1b[34m",   // Blue
                        log::Level::Trace => "\x1b[36m",   // Cyan
                    },
                    "\x1b[0m",
                )
            } else {
                ("", "")
            };

            let file = record.file().unwrap_or("unknown");
            let line = record.line().unwrap_or(0);
            let target = record.target();

            out.finish(format_args!(
                "{} {}{} [{}:{}] [{}] {}{}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"),
                level_color,
                record.level(),
                file,
                line,
                target,
                message,
                reset
            ))
        });

    // Configure different targets based on environment
    // Note: File logging is now handled by uc-observability (JSON output via tracing).
    // This plugin only provides log::* macro routing to Webview/stdout.
    if is_dev {
        // Development: Output to Webview (browser DevTools console)
        builder = builder.target(Target::new(TargetKind::Webview));
    } else {
        // Production: Stdout only (JSON file logging handled by tracing/uc-observability)
        builder = builder.target(Target::new(TargetKind::Stdout));
    }

    builder
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logger_builder() {
        // Verify the builder can be constructed without panicking
        let _builder = get_builder();
    }

    #[test]
    fn test_development_detection() {
        // In test builds, this will be false (tests run in release mode by default)
        // But we're testing the function works, not the value
        let _is_dev = is_development();
    }
}
