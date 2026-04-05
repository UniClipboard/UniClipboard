//! OTLP telemetry pipeline (Phase 87). Replaces the legacy Seq module.
//!
//! Transport: OTLP/HTTP-protobuf (not gRPC). Uses `reqwest` + `rustls-tls` stack
//! already present in the workspace. No gRPC transport is activated;
//! `default-features = false` in Cargo.toml disables the `grpc-tonic` feature.
//!
//! Note: `tonic` appears as an indirect dependency via `opentelemetry-proto/gen-tonic-messages`
//! (protobuf code generation support), not as a gRPC transport. This satisfies D-12
//! which forbids the gRPC *transport* stack, not the prost/tonic protobuf type support.
//!
//! # Usage
//!
//! Tests and simple callers use `init_otlp_pipeline` with `tracing_subscriber::Registry`:
//! ```ignore
//! let result = init_otlp_pipeline(&LogProfile::Dev, None);
//! ```
//!
//! For composition with other layers, the caller can use `init_otlp_pipeline_generic<S>`.
pub mod propagator;
pub mod resource;
pub mod layer;

pub use resource::build_resource;

use opentelemetry::global;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};
use tracing_subscriber::{registry::LookupSpan, Layer};

use crate::profile::LogProfile;

/// Boxed OTLP layer type. Used as the return type for `init_otlp_pipeline` so callers
/// don't need to specify the subscriber type `S` when they don't care about it (e.g., tests).
pub type OtlpLayer =
    Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync + 'static>;

/// Guard that keeps the OTLP tracer provider alive.
/// On drop, flushes pending spans and shuts down the provider.
pub struct OtlpGuard {
    provider: Option<SdkTracerProvider>,
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.provider.take() {
            // Best-effort flush; log on failure but never panic.
            match provider.shutdown() {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "OTLP tracer provider shutdown failed");
                }
            }
        }
    }
}

/// Initialize the OTLP exporter and provider, without creating the tracing layer.
///
/// This two-phase initialization allows callers that compose multiple
/// `tracing_subscriber` layers to:
/// 1. Run the async provider setup early (before the full subscriber chain is known).
/// 2. Create the typed layer later via `layer::build_otlp_layer::<S>()` once the
///    subscriber type `S` is determined by the composition context.
///
/// `SdkTracerProvider` is `Clone` with Arc semantics (clone increments the Arc
/// counter; shutdown is executed once on the shared inner state). This means the
/// caller can clone the provider to pass to `layer::build_otlp_layer` while the
/// guard retains the other clone for flush-on-drop.
///
/// Returns `Ok(None)` when OTLP is disabled (Prod profile or missing env var).
/// The W3C propagator is always installed globally regardless.
pub fn init_otlp_provider(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(SdkTracerProvider, OtlpGuard)>> {
    // Always install the W3C propagator.
    global::set_text_map_propagator(TraceContextPropagator::new());

    if matches!(profile, LogProfile::Prod) {
        return Ok(None);
    }
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP span exporter: {e}"))?;

    let resource = resource::build_resource(device_id);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    // Clone provider for the guard (Arc clone; shared inner state).
    // The caller uses the original provider to create a layer.
    let guard = OtlpGuard { provider: Some(provider.clone()) };

    Ok(Some((provider, guard)))
}

/// Build the internal OTLP pipeline without the boxed layer wrapper.
///
/// Used by `init_otlp_pipeline_generic` for callers that compose with a specific
/// subscriber type `S` and want to avoid the box allocation.
pub fn init_otlp_pipeline_generic<S>(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(impl Layer<S> + Send + Sync + 'static, OtlpGuard)>>
where
    S: tracing::Subscriber + for<'a> LookupSpan<'a> + Send + Sync,
{
    // Always install the W3C propagator — even when the exporter is disabled,
    // so cross-device headers remain populated (prevents silent context loss).
    global::set_text_map_propagator(TraceContextPropagator::new());

    if matches!(profile, LogProfile::Prod) {
        return Ok(None);
    }
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP span exporter: {e}"))?;

    let resource = resource::build_resource(device_id);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let otel_layer = layer::build_otlp_layer::<S>(&provider, profile);

    Ok(Some((otel_layer, OtlpGuard { provider: Some(provider) })))
}

/// Initialize the OTLP tracing pipeline.
///
/// Returns `Ok(None)` when:
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` env var is not set, OR
/// - The profile is `LogProfile::Prod` (OTLP is dev-only).
///
/// Returns `Ok(Some((layer, guard)))` when the env var is set and profile is Dev/DebugClipboard/Cli.
/// The caller must store the guard for the lifetime of the process; dropping it flushes spans.
///
/// The W3C TraceContextPropagator is always installed globally, even when the exporter
/// is disabled, so cross-device traceparent headers remain populated.
///
/// Returns a boxed `OtlpLayer` (i.e., `Box<dyn Layer<Registry>>`) to avoid
/// type inference issues at call sites (tests, bootstrap) where the subscriber type
/// parameter `S` cannot always be inferred. For composition with a specific `S`,
/// use `init_otlp_pipeline_generic`.
pub fn init_otlp_pipeline(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(OtlpLayer, OtlpGuard)>> {
    // Always install the W3C propagator — even when the exporter is disabled,
    // so cross-device headers remain populated (prevents silent context loss).
    global::set_text_map_propagator(TraceContextPropagator::new());

    if matches!(profile, LogProfile::Prod) {
        return Ok(None);
    }
    if std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err() {
        return Ok(None);
    }

    let exporter = SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP span exporter: {e}"))?;

    let resource = resource::build_resource(device_id);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let otel_layer = layer::build_otlp_layer::<tracing_subscriber::Registry>(&provider, profile);

    Ok(Some((Box::new(otel_layer), OtlpGuard { provider: Some(provider) })))
}
