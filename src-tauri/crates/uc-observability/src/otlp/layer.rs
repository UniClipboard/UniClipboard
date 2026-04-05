use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing::Subscriber;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::{registry::LookupSpan, Layer};

use crate::profile::LogProfile;

pub(crate) fn build_otlp_layer<S>(
    provider: &SdkTracerProvider,
    profile: &LogProfile,
) -> impl Layer<S> + Send + Sync + 'static
where
    S: Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    let tracer = provider.tracer("uc-observability");
    OpenTelemetryLayer::new(tracer).with_filter(profile.json_filter())
}
