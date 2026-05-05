use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{
    filter::{FilterFn, Filtered},
    registry::LookupSpan,
    EnvFilter, Layer,
};

use crate::profile::LogProfile;
use crate::telemetry_gate;

/// Concrete OTLP layer type returned by `build_otlp_layer`.
///
/// Two filters stacked over `OpenTelemetryLayer<S, SdkTracer>`:
///
/// 1. **Inner `EnvFilter`** (`profile.otlp_filter()`) — static profile-based
///    level/target filter applied at subscriber construction.
/// 2. **Outer `FilterFn`** (telemetry runtime gate) — checks the global
///    `telemetry_enabled` AtomicBool on every metadata callback so the user
///    can toggle telemetry in Settings without restarting the process.
///
/// Returning a concrete type (rather than `impl Layer<S>`) preserves the
/// pattern the bootstrap uses: bind to `Option<OtlpConcreteLayer<_>>` and
/// let Rust infer `S` from the downstream `.with()` composition.
pub type OtlpConcreteLayer<S> =
    Filtered<Filtered<OpenTelemetryLayer<S, SdkTracer>, EnvFilter, S>, FilterFn, S>;

pub fn build_otlp_layer<S>(
    provider: &SdkTracerProvider,
    profile: &LogProfile,
) -> OtlpConcreteLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let tracer = provider.tracer("uc-observability");
    // Bind the fn item to a typed fn-pointer local so `FilterFn::new`
    // resolves its `F` to the default `fn(&Metadata<'_>) -> bool` and the
    // resulting layer matches `OtlpConcreteLayer<S>` exactly.
    let gate: TelemetryGateFn = telemetry_gate_filter;
    OpenTelemetryLayer::new(tracer)
        .with_filter(profile.otlp_filter())
        .with_filter(FilterFn::new(gate))
}

type TelemetryGateFn = fn(&tracing::Metadata<'_>) -> bool;

/// Free function so `FilterFn` can use the default `fn(&Metadata) -> bool`
/// generic parameter (closure types would force a different generic and
/// blow up the public type alias).
fn telemetry_gate_filter(_meta: &tracing::Metadata<'_>) -> bool {
    telemetry_gate::is_telemetry_enabled()
}
