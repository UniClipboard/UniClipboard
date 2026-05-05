use opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge;
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use tracing::Subscriber;
use tracing_subscriber::{
    filter::{FilterFn, Filtered},
    registry::LookupSpan,
    EnvFilter, Layer,
};

use crate::profile::LogProfile;
use crate::telemetry_gate;

/// Concrete OTLP logs layer type returned by `build_otlp_logs_layer`.
///
/// Two filters stacked over `OpenTelemetryTracingBridge`:
///
/// 1. **Inner `EnvFilter`** (`profile.otlp_filter()`) — profile-based level
///    filter so only INFO+ events are exported.
/// 2. **Outer `FilterFn`** — runtime telemetry gate; toggling
///    `general.telemetry_enabled` flips the AtomicBool checked here so
///    log records stop flowing without a process restart.
pub type OtlpLogsConcreteLayer<S> = Filtered<
    Filtered<OpenTelemetryTracingBridge<SdkLoggerProvider, SdkLogger>, EnvFilter, S>,
    FilterFn,
    S,
>;

pub fn build_otlp_logs_layer<S>(
    provider: &SdkLoggerProvider,
    profile: &LogProfile,
) -> OtlpLogsConcreteLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let bridge = OpenTelemetryTracingBridge::new(provider);
    let gate: TelemetryGateFn = telemetry_gate_filter;
    bridge
        .with_filter(profile.otlp_filter())
        .with_filter(FilterFn::new(gate))
}

type TelemetryGateFn = fn(&tracing::Metadata<'_>) -> bool;

/// Free function so `FilterFn` matches its default `fn(&Metadata) -> bool`
/// generic parameter (mirrors `layer.rs`).
fn telemetry_gate_filter(_meta: &tracing::Metadata<'_>) -> bool {
    telemetry_gate::is_telemetry_enabled()
}
