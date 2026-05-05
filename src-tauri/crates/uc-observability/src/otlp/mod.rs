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
pub mod layer;
pub mod logs_layer;
pub mod propagator;
pub mod redact;
pub mod resource;

mod config;
mod provider;

pub use provider::OtlpGuard;
pub use resource::build_resource;

use opentelemetry_sdk::{logs::SdkLoggerProvider, trace::SdkTracerProvider};
use tracing_subscriber::{registry::LookupSpan, Layer};

use crate::profile::LogProfile;

/// Boxed OTLP layer type. Used as the return type for `init_otlp_pipeline` so callers
/// don't need to specify the subscriber type `S` when they don't care about it (e.g., tests).
pub type OtlpLayer = Box<dyn Layer<tracing_subscriber::Registry> + Send + Sync + 'static>;

/// Providers returned by `init_otlp_provider` for two-phase layer construction.
pub struct OtlpProviders {
    pub tracer_provider: SdkTracerProvider,
    pub logger_provider: SdkLoggerProvider,
}

/// Initialize the OTLP exporters and providers, without creating the tracing layers.
///
/// This two-phase initialization allows callers that compose multiple
/// `tracing_subscriber` layers to:
/// 1. Run the provider setup early (before the full subscriber chain is known).
/// 2. Create the typed layers later via `layer::build_otlp_layer::<S>()` and
///    `logs_layer::build_otlp_logs_layer::<S>()` once the subscriber type `S`
///    is determined by the composition context.
///
/// Returns `Ok(None)` only when no OTLP endpoint is configured. The provider
/// is no longer gated on `telemetry_enabled` — the user-facing toggle is now
/// honored at event time by `FilterFn` wrappers in `layer.rs` /
/// `logs_layer.rs`, so callers should always init the provider when an
/// endpoint exists and let the runtime gate decide whether events flow.
///
/// The W3C propagator is always installed globally regardless of init
/// outcome.
pub fn init_otlp_provider(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(OtlpProviders, OtlpGuard)>> {
    let Some((tracer_provider, logger_provider, guard)) =
        provider::init_provider_and_guard(profile, device_id)?
    else {
        return Ok(None);
    };
    Ok(Some((
        OtlpProviders {
            tracer_provider,
            logger_provider,
        },
        guard,
    )))
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
    let Some((providers, guard)) = init_otlp_provider(profile, device_id)? else {
        return Ok(None);
    };

    let otel_layer = layer::build_otlp_layer::<S>(&providers.tracer_provider, profile);

    Ok(Some((otel_layer, guard)))
}

/// Initialize the OTLP tracing pipeline.
///
/// Returns `Ok(None)` when `OTEL_EXPORTER_OTLP_ENDPOINT` env var is not set
/// (and no compile-time endpoint baked in). Whether events actually flow is
/// controlled at runtime by the telemetry gate
/// (`uc_observability::set_telemetry_enabled`).
///
/// Returns `Ok(Some((layer, guard)))` when the endpoint is configured. The
/// caller must store the guard for the lifetime of the process; dropping it
/// flushes spans.
///
/// The W3C TraceContextPropagator is always installed globally, even when
/// the exporter is disabled, so cross-device traceparent headers remain
/// populated.
///
/// Returns a boxed `OtlpLayer` (i.e., `Box<dyn Layer<Registry>>`) to avoid
/// type inference issues at call sites (tests, bootstrap) where the
/// subscriber type parameter `S` cannot always be inferred. For composition
/// with a specific `S`, use `init_otlp_pipeline_generic`.
pub fn init_otlp_pipeline(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(OtlpLayer, OtlpGuard)>> {
    let Some((providers, guard)) = init_otlp_provider(profile, device_id)? else {
        return Ok(None);
    };

    let otel_layer = layer::build_otlp_layer::<tracing_subscriber::Registry>(
        &providers.tracer_provider,
        profile,
    );

    Ok(Some((Box::new(otel_layer), guard)))
}
