use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use tracing::Subscriber;
use tracing_subscriber::{filter::Filtered, registry::LookupSpan, EnvFilter, Layer};

use crate::profile::LogProfile;

/// Concrete OTLP logs layer type returned by `build_otlp_logs_layer`.
///
/// `OpenTelemetryTracingBridge` converts tracing events into OTLP log records.
/// The `Filtered` wrapper applies the profile-based `EnvFilter` so only INFO+
/// events are exported.
pub type OtlpLogsConcreteLayer<S> =
    Filtered<OpenTelemetryTracingBridge<SdkLoggerProvider, SdkLogger>, EnvFilter, S>;

pub fn build_otlp_logs_layer<S>(
    provider: &SdkLoggerProvider,
    profile: &LogProfile,
) -> OtlpLogsConcreteLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let bridge = OpenTelemetryTracingBridge::new(provider);
    bridge.with_filter(profile.otlp_filter())
}
