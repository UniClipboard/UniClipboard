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

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use tracing::Subscriber;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::fmt::format::JsonFields;
use tracing_subscriber::prelude::*;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::{fmt, registry};

use crate::format::FlatJsonFormat;
use crate::profile::LogProfile;

/// Number of daily rolling log files to keep per role. Older files are pruned
/// automatically on initialization, so the log directory cannot grow without
/// bound.
const LOG_RETENTION_DAYS: usize = 7;

/// Diagnostic-only daemon override for writing JSON logs to one exact,
/// non-rolling file. Benchmarks use this to measure one daemon process without
/// racing daily rotation or accidentally reading a GUI/CLI role file.
const LOG_FILE_ENV: &str = "UC_LOG_FILE";

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
/// * `logs_dir` - Directory for JSON log files (creates `uniclipboard-<role>.json.YYYY-MM-DD`,
///   role from [`crate::scope::role_log_file_stem`] so co-resident processes don't share a file)
/// * `profile` - The [`LogProfile`] controlling filter verbosity
///
/// When `UC_LOG_FILE` is set, `UC_HOST_ROLE` must be `daemon` and the value must
/// be an absolute path. The JSON layer writes that exact non-rolling file with a
/// lossless queue instead. This override is intended for diagnostics and
/// process-isolated benchmarks; normal application runs should leave it unset.
///
/// # Errors
///
/// Returns `Err` if the log destination cannot be created or opened.
pub fn build_json_layer<S>(
    logs_dir: &Path,
    profile: &LogProfile,
) -> anyhow::Result<(impl tracing_subscriber::Layer<S> + Send + Sync, WorkerGuard)>
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    let json_filter = profile.json_filter();
    let exact_file = exact_log_file_from_env()?;
    let (non_blocking, guard) = build_json_writer(logs_dir, exact_file.as_deref())?;

    let json_layer = fmt::layer()
        .event_format(FlatJsonFormat::new())
        .fmt_fields(JsonFields::new())
        .with_writer(non_blocking)
        .with_ansi(false)
        .with_filter(json_filter);

    Ok((json_layer, guard))
}

fn exact_log_file_from_env() -> anyhow::Result<Option<PathBuf>> {
    let Some(path) = std::env::var_os(LOG_FILE_ENV).map(PathBuf::from) else {
        return Ok(None);
    };
    validate_exact_log_role(std::env::var_os("UC_HOST_ROLE").as_deref())?;
    Ok(Some(path))
}

fn validate_exact_log_role(role: Option<&OsStr>) -> anyhow::Result<()> {
    anyhow::ensure!(
        role == Some(OsStr::new("daemon")),
        "{LOG_FILE_ENV} is daemon-only; UC_HOST_ROLE must be `daemon`"
    );
    Ok(())
}

fn build_json_writer(
    logs_dir: &Path,
    exact_file: Option<&Path>,
) -> anyhow::Result<(
    tracing_appender::non_blocking::NonBlocking,
    tracing_appender::non_blocking::WorkerGuard,
)> {
    if let Some(path) = exact_file {
        anyhow::ensure!(
            path.is_absolute(),
            "{LOG_FILE_ENV} must be an absolute path: {}",
            path.display()
        );
        let parent = path.parent().ok_or_else(|| {
            anyhow::anyhow!("{LOG_FILE_ENV} has no parent directory: {}", path.display())
        })?;
        std::fs::create_dir_all(parent)?;
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|err| {
                anyhow::anyhow!(
                    "failed to open {LOG_FILE_ENV} {} for append: {err}",
                    path.display()
                )
            })?;
        return Ok(
            tracing_appender::non_blocking::NonBlockingBuilder::default()
                .lossy(false)
                .finish(file),
        );
    }

    std::fs::create_dir_all(logs_dir)?;

    // ADR-008 D20 (P4-0): per-role file name so the GUI host and the detached
    // `uniclipd` never append to the same rolling log file. The `.json` is part
    // of the *prefix* (no suffix is set), so the rolled files keep the
    // `uniclipboard-<role>.json.<date>` shape — the Builder appends `.<date>`.
    // `max_files` prunes everything older than the last `LOG_RETENTION_DAYS`.
    let filename_prefix = format!("{}.json", crate::scope::role_log_file_stem());
    let daily_appender = tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix(filename_prefix)
        .max_log_files(LOG_RETENTION_DAYS)
        .build(logs_dir)
        .map_err(|err| anyhow::anyhow!("failed to build rolling log appender: {err}"))?;
    Ok(tracing_appender::non_blocking(daily_appender))
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
/// Returns the [`WorkerGuard`] for the JSON file writer. **Ownership is the
/// caller's** — this is `tracing_appender`'s native RAII contract: keep the
/// guard alive for the process lifetime, and drop it as the final step of a
/// clean shutdown to drain buffered lines to disk. This matters for hosts that
/// exit via `process::exit` (e.g. Tauri's `app.restart()` after an update
/// install), which skips static destructors: a guard parked in a static would
/// never drop, silently losing the most recent — and most diagnostically
/// valuable — buffered lines.
///
/// # Arguments
///
/// * `logs_dir` - Directory for JSON log files (creates `uniclipboard-<role>.json.YYYY-MM-DD`,
///   role from [`crate::scope::role_log_file_stem`] so co-resident processes don't share a file)
/// * `profile` - The [`LogProfile`] controlling filter verbosity
///
/// # Errors
///
/// Returns `Err` if:
/// - The global subscriber is already registered
/// - The logs directory cannot be created
#[must_use = "dropping the guard stops the JSON file writer; hold it for the process lifetime"]
pub fn init_tracing_subscriber(
    logs_dir: &Path,
    profile: LogProfile,
) -> anyhow::Result<WorkerGuard> {
    let console_layer = build_console_layer(&profile);
    let (json_layer, guard) = build_json_layer(logs_dir, &profile)?;

    // Compose and register the global subscriber
    registry().with(console_layer).with(json_layer).try_init()?;

    tracing::info!(profile = %profile, "Tracing initialized with dual output");

    Ok(guard)
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    #[test]
    fn exact_log_file_writer_uses_requested_readable_file() {
        let temp = tempfile::tempdir().expect("temp directory");
        let log_file = temp.path().join("daemon-benchmark.jsonl");
        let (mut writer, guard) =
            build_json_writer(temp.path(), Some(&log_file)).expect("exact log writer");

        writer
            .write_all(b"{\"level\":\"INFO\",\"message\":\"probe\"}\n")
            .expect("write test log");
        drop(writer);
        drop(guard);

        let contents = std::fs::read_to_string(&log_file).expect("exact log file is readable");
        assert!(contents.contains(r#""level":"INFO""#));
    }

    #[test]
    fn exact_log_file_writer_rejects_relative_path() {
        let temp = tempfile::tempdir().expect("temp directory");
        let err = build_json_writer(temp.path(), Some(Path::new("relative.jsonl")))
            .expect_err("relative exact log path must fail");
        assert!(err.to_string().contains("must be an absolute path"));
    }

    #[test]
    fn exact_log_file_override_is_daemon_only() {
        assert!(validate_exact_log_role(Some(OsStr::new("daemon"))).is_ok());
        assert!(validate_exact_log_role(Some(OsStr::new("gui-host"))).is_err());
        assert!(validate_exact_log_role(None).is_err());
    }
}
