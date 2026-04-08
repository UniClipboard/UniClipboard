use opentelemetry::global;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, propagation::TraceContextPropagator, trace::SdkTracerProvider,
};

use crate::profile::LogProfile;

use super::{config, redact, resource};

/// Guard that keeps the OTLP tracer and logger providers alive.
/// On drop, flushes pending data and shuts down the providers.
pub struct OtlpGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        if let Some(provider) = self.logger_provider.take() {
            match provider.shutdown() {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "OTLP logger provider shutdown failed");
                }
            }
        }
        if let Some(provider) = self.tracer_provider.take() {
            match provider.shutdown() {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "OTLP tracer provider shutdown failed");
                }
            }
        }
    }
}

fn build_span_exporter_from_env() -> anyhow::Result<SpanExporter> {
    SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP span exporter: {e}"))
}

/// Check whether the OTLP pipeline should be activated for the given profile
/// and user telemetry preference.
///
/// Activation rules:
/// - Dev / DebugClipboard / Cli: always allowed (developer-controlled)
/// - Prod: only when `telemetry_enabled` is `true`
fn otlp_is_enabled(profile: &LogProfile, telemetry_enabled: bool) -> bool {
    match profile {
        LogProfile::Prod => telemetry_enabled,
        _ => true,
    }
}

fn build_log_exporter_from_env() -> anyhow::Result<opentelemetry_otlp::LogExporter> {
    opentelemetry_otlp::LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP log exporter: {e}"))
}

fn build_otlp_guard(
    tracer_provider: &SdkTracerProvider,
    logger_provider: &SdkLoggerProvider,
) -> OtlpGuard {
    OtlpGuard {
        tracer_provider: Some(tracer_provider.clone()),
        logger_provider: Some(logger_provider.clone()),
    }
}

/// Initialize the OTLP provider with dual-layer gating:
/// 1. Endpoint must be configured (env var or baked-in)
/// 2. Profile + `telemetry_enabled` must allow it
pub(super) fn init_provider_and_guard(
    profile: &LogProfile,
    device_id: Option<&str>,
    telemetry_enabled: bool,
) -> anyhow::Result<Option<(SdkTracerProvider, SdkLoggerProvider, OtlpGuard)>> {
    // Always install the W3C propagator.
    global::set_text_map_propagator(TraceContextPropagator::new());

    if !otlp_is_enabled(profile, telemetry_enabled) || !config::otlp_endpoint_is_configured() {
        return Ok(None);
    }

    config::prime_runtime_otlp_env_from_baked();
    let resource = resource::build_resource(device_id);

    // Trace provider
    let raw_span_exporter = build_span_exporter_from_env()?;
    let span_exporter = redact::RedactingExporter::new(raw_span_exporter);
    let tracer_provider = SdkTracerProvider::builder()
        .with_batch_exporter(span_exporter)
        .with_resource(resource.clone())
        .build();

    // Logs provider
    let log_exporter = build_log_exporter_from_env()?;
    let logger_provider = SdkLoggerProvider::builder()
        .with_batch_exporter(log_exporter)
        .with_resource(resource)
        .build();

    let guard = build_otlp_guard(&tracer_provider, &logger_provider);

    Ok(Some((tracer_provider, logger_provider, guard)))
}
