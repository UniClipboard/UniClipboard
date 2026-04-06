use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::{SdkTracer, SdkTracerProvider};
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{filter::Filtered, registry::LookupSpan, EnvFilter, Layer};

use crate::profile::LogProfile;

/// Concrete OTLP layer type returned by `build_otlp_layer`.
///
/// `OpenTelemetryLayer<S, T>` is generic over the subscriber type `S` and
/// tracer type `T`. Returning a concrete type (rather than `impl Layer<S>`)
/// allows callers to bind the return value to a typed `Option<...>` variable,
/// letting Rust infer `S` from the downstream `.with()` composition context
/// rather than fixing it at the call site.
///
/// `S` is the subscriber type (e.g., `Layered<..., Registry>`),
/// `SdkTracer` is the concrete OTel tracer implementation.
pub type OtlpConcreteLayer<S> = Filtered<OpenTelemetryLayer<S, SdkTracer>, EnvFilter, S>;

pub fn build_otlp_layer<S>(
    provider: &SdkTracerProvider,
    profile: &LogProfile,
) -> OtlpConcreteLayer<S>
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let tracer = provider.tracer("uc-observability");
    OpenTelemetryLayer::new(tracer).with_filter(profile.otlp_filter())
}
