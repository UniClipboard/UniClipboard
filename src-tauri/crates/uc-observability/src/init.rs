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
//! needed, initialize it separately before calling this function, or integrate
//! Sentry via its own global init mechanism (which hooks into the existing
//! tracing subscriber automatically).

use std::path::Path;
use std::sync::OnceLock;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::JsonFields;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, registry};

use crate::format::FlatJsonFormat;
use crate::profile::LogProfile;

/// Static storage for the JSON file writer guard.
/// The guard must live for the application's lifetime to ensure the non-blocking
/// writer flushes all pending log entries.
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Initialize the dual-output tracing subscriber.
///
/// Creates a registry with:
/// 1. A console layer using pretty format with ANSI colors, file/line info
/// 2. A JSON file layer using [`FlatJsonFormat`] with daily rolling files
///
/// Both layers get independent `EnvFilter`s from the given profile.
/// If `RUST_LOG` is set, it overrides the profile filters for both layers.
///
/// The `WorkerGuard` for the JSON file writer is stored in a static `OnceLock`
/// to prevent early drop.
///
/// # Arguments
///
/// * `logs_dir` - Directory for JSON log files (creates `uniclipboard.json.YYYY-MM-DD`)
/// * `profile` - The [`LogProfile`] controlling filter verbosity
///
/// # Errors
///
/// Returns `Err` if:
/// - The global subscriber is already registered
/// - The logs directory cannot be created
pub fn init_tracing_subscriber(logs_dir: &Path, profile: LogProfile) -> anyhow::Result<()> {
    // Ensure logs directory exists
    std::fs::create_dir_all(logs_dir)?;

    // Build per-layer filters from the profile (RUST_LOG override is handled inside)
    let console_filter = profile.console_filter();
    let json_filter = profile.json_filter();

    // Console layer: pretty format for developer experience
    let console_layer = fmt::layer()
        .with_timer(fmt::time::ChronoUtc::new(
            "%Y-%m-%d %H:%M:%S%.3f".to_string(),
        ))
        .with_level(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_ansi(cfg!(not(test)))
        .with_writer(std::io::stdout)
        .with_filter(console_filter);

    // JSON layer: structured output to daily-rotating file
    let daily_appender = tracing_appender::rolling::daily(logs_dir, "uniclipboard.json");
    let (non_blocking, guard) = tracing_appender::non_blocking(daily_appender);

    // Store guard to keep writer alive for app lifetime
    if JSON_GUARD.set(guard).is_err() {
        anyhow::bail!("JSON log guard already initialized (init_tracing_subscriber called twice?)");
    }

    let json_layer = fmt::layer()
        .event_format(FlatJsonFormat::new())
        .fmt_fields(JsonFields::new())
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(json_filter);

    // Compose and register the global subscriber
    registry().with(console_layer).with(json_layer).try_init()?;

    tracing::info!(profile = %profile, "Tracing initialized with dual output");

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_tracing_subscriber_signature_compiles() {
        // Verify the function signature accepts &Path and LogProfile
        // We cannot actually call try_init() in tests since it registers a global subscriber,
        // and multiple tests would conflict.
        let _: fn(&Path, LogProfile) -> anyhow::Result<()> = init_tracing_subscriber;
    }

    #[test]
    fn test_init_creates_logs_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let logs_dir = temp_dir.path().join("nested").join("logs");
        assert!(!logs_dir.exists());

        // We can't call the full init (global subscriber conflict), but we can
        // test the directory creation logic independently.
        std::fs::create_dir_all(&logs_dir).unwrap();
        assert!(logs_dir.exists());
    }

    #[test]
    fn test_daily_rolling_appender_creates_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let logs_dir = temp_dir.path();

        // Create a daily appender and write to it
        let daily_appender = tracing_appender::rolling::daily(logs_dir, "uniclipboard.json");
        let (non_blocking, _guard) = tracing_appender::non_blocking(daily_appender);

        // The writer should be functional (writing through non_blocking)
        use std::io::Write;
        let mut writer = non_blocking;
        writer.write_all(b"test\n").unwrap();
        writer.flush().unwrap();

        // File should be created in the temp directory
        // (may take a moment for non-blocking to flush)
        drop(_guard); // Force flush by dropping guard
        let files: Vec<_> = std::fs::read_dir(logs_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("uniclipboard.json")
            })
            .collect();
        assert!(
            !files.is_empty(),
            "Expected uniclipboard.json.* file in logs dir"
        );
    }

    #[test]
    fn test_profile_filter_construction_does_not_panic() {
        // Verify that filter construction for all profiles works without panicking
        for profile in [
            LogProfile::Dev,
            LogProfile::Prod,
            LogProfile::DebugClipboard,
        ] {
            let _ = profile.console_filter();
            let _ = profile.json_filter();
        }
    }
}
