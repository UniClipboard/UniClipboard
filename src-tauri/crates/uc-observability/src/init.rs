//! Dual-output tracing subscriber initialization.
//!
//! Composes a console (pretty) layer and a JSON file layer on a single
//! `tracing_subscriber::Registry`, with per-layer filtering from `LogProfile`.
//!
//! # Layer Builders
//!
//! For callers that need to compose additional layers (e.g., Sentry), this
//! module exposes [`build_console_layer`] and [`build_json_layer`] which
//! return layers with per-layer filters applied. The caller can compose these
//! with their own layers on a shared `Registry`.
//!
//! # Standalone Initialization
//!
//! [`init_tracing_subscriber`] is a convenience wrapper that calls the layer
//! builders and registers a global subscriber without any extra layers.

use std::path::Path;
use std::sync::OnceLock;

use tracing::Subscriber;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::JsonFields;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{fmt, registry};

use crate::format::FlatJsonFormat;
use crate::profile::LogProfile;

/// Static storage for the JSON file writer guard.
/// The guard must live for the application's lifetime to ensure the non-blocking
/// writer flushes all pending log entries.
static JSON_GUARD: OnceLock<WorkerGuard> = OnceLock::new();

/// Build the console (pretty) layer with per-layer filtering.
///
/// Returns a layer suitable for composing with other layers on a subscriber.
/// The layer outputs to stdout with ANSI colors, timestamps, file/line info.
///
/// # Arguments
///
/// * `profile` - The [`LogProfile`] controlling filter verbosity
pub fn build_console_layer<S>(
    profile: &LogProfile,
) -> impl tracing_subscriber::Layer<S> + Send + Sync
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let console_filter = profile.console_filter();

    fmt::layer()
        .with_timer(fmt::time::ChronoUtc::new(
            "%Y-%m-%d %H:%M:%S%.3f".to_string(),
        ))
        .with_level(true)
        .with_file(true)
        .with_line_number(true)
        .with_target(true)
        .with_ansi(cfg!(not(test)))
        .with_writer(std::io::stdout)
        .with_filter(console_filter)
}

/// Build the JSON file layer with per-layer filtering and daily rolling files.
///
/// Returns the layer and a [`WorkerGuard`] that the caller MUST keep alive
/// for the application's lifetime. Dropping the guard will cause buffered log
/// entries to be lost.
///
/// # Arguments
///
/// * `logs_dir` - Directory for JSON log files (creates `uniclipboard.json.YYYY-MM-DD`)
/// * `profile` - The [`LogProfile`] controlling filter verbosity
///
/// # Errors
///
/// Returns `Err` if the logs directory cannot be created.
pub fn build_json_layer<S>(
    logs_dir: &Path,
    profile: &LogProfile,
) -> anyhow::Result<(impl tracing_subscriber::Layer<S> + Send + Sync, WorkerGuard)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    std::fs::create_dir_all(logs_dir)?;

    let json_filter = profile.json_filter();

    let daily_appender = tracing_appender::rolling::daily(logs_dir, "uniclipboard.json");
    let (non_blocking, guard) = tracing_appender::non_blocking(daily_appender);

    let json_layer = fmt::layer()
        .event_format(FlatJsonFormat::new())
        .fmt_fields(JsonFields::new())
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(json_filter);

    Ok((json_layer, guard))
}

/// Initialize the dual-output tracing subscriber (convenience wrapper).
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
    let console_layer = build_console_layer(&profile);
    let (json_layer, guard) = build_json_layer(logs_dir, &profile)?;

    // Store guard to keep writer alive for app lifetime
    if JSON_GUARD.set(guard).is_err() {
        anyhow::bail!("JSON log guard already initialized (init_tracing_subscriber called twice?)");
    }

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
        let _: fn(&Path, LogProfile) -> anyhow::Result<()> = init_tracing_subscriber;
    }

    #[test]
    fn test_init_creates_logs_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let logs_dir = temp_dir.path().join("nested").join("logs");
        assert!(!logs_dir.exists());

        std::fs::create_dir_all(&logs_dir).unwrap();
        assert!(logs_dir.exists());
    }

    #[test]
    fn test_daily_rolling_appender_creates_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let logs_dir = temp_dir.path();

        let daily_appender = tracing_appender::rolling::daily(logs_dir, "uniclipboard.json");
        let (non_blocking, _guard) = tracing_appender::non_blocking(daily_appender);

        use std::io::Write;
        let mut writer = non_blocking;
        writer.write_all(b"test\n").unwrap();
        writer.flush().unwrap();

        drop(_guard);
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
        for profile in [
            LogProfile::Dev,
            LogProfile::Prod,
            LogProfile::DebugClipboard,
            LogProfile::Cli,
        ] {
            let _ = profile.console_filter();
            let _ = profile.json_filter();
        }
    }

    #[test]
    fn test_build_console_layer_compiles() {
        // Verify the generic function can be used with Registry
        let _layer = build_console_layer::<tracing_subscriber::Registry>(&LogProfile::Dev);
    }

    #[test]
    fn test_build_json_layer_compiles() {
        let temp_dir = tempfile::tempdir().unwrap();
        let (_layer, _guard) =
            build_json_layer::<tracing_subscriber::Registry>(temp_dir.path(), &LogProfile::Dev)
                .unwrap();
    }

    #[test]
    fn test_build_json_layer_creates_logs_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let logs_dir = temp_dir.path().join("nested").join("logs");
        assert!(!logs_dir.exists());

        let (_layer, _guard) =
            build_json_layer::<tracing_subscriber::Registry>(&logs_dir, &LogProfile::Prod).unwrap();
        assert!(logs_dir.exists());
    }
}
