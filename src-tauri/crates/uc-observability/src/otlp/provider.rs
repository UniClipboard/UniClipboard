use opentelemetry::global;
use opentelemetry_otlp::{MetricExporter, Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider, propagation::TraceContextPropagator,
    trace::SdkTracerProvider,
};

use crate::profile::LogProfile;

use super::{config, redact, resource};

/// Guard that keeps the OTLP tracer and logger providers alive.
/// On drop, flushes pending data and shuts down the providers.
pub struct OtlpGuard {
    tracer_provider: Option<SdkTracerProvider>,
    logger_provider: Option<SdkLoggerProvider>,
    meter_provider: Option<SdkMeterProvider>,
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        crate::metrics::clear();

        if let Some(provider) = self.meter_provider.take() {
            match provider.shutdown() {
                Ok(()) => {}
                Err(e) => {
                    tracing::warn!(error = %e, "OTLP meter provider shutdown failed");
                }
            }
        }
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

fn build_metric_exporter_from_env() -> anyhow::Result<MetricExporter> {
    MetricExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP metric exporter: {e}"))
}

fn build_otlp_guard(
    tracer_provider: &SdkTracerProvider,
    logger_provider: &SdkLoggerProvider,
    meter_provider: &SdkMeterProvider,
) -> OtlpGuard {
    OtlpGuard {
        tracer_provider: Some(tracer_provider.clone()),
        logger_provider: Some(logger_provider.clone()),
        meter_provider: Some(meter_provider.clone()),
    }
}

/// Initialize the OTLP provider with dual-layer gating:
/// 1. Endpoint must be configured (env var or baked-in)
/// 2. Profile + `telemetry_enabled` must allow it
pub(super) fn init_provider_and_guard(
    profile: &LogProfile,
    device_id: Option<&str>,
    telemetry_enabled: bool,
) -> anyhow::Result<
    Option<(
        SdkTracerProvider,
        SdkLoggerProvider,
        SdkMeterProvider,
        OtlpGuard,
    )>,
> {
    // Always install the W3C propagator.
    global::set_text_map_propagator(TraceContextPropagator::new());

    if !otlp_is_enabled(profile, telemetry_enabled) || !config::otlp_endpoint_is_configured() {
        crate::metrics::clear();
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
        .with_resource(resource.clone())
        .build();

    let metric_exporter = build_metric_exporter_from_env()?;
    let meter_provider = SdkMeterProvider::builder()
        .with_periodic_exporter(metric_exporter)
        .with_resource(resource)
        .build();
    global::set_meter_provider(meter_provider.clone());
    crate::metrics::install(&meter_provider);

    let guard = build_otlp_guard(&tracer_provider, &logger_provider, &meter_provider);

    Ok(Some((
        tracer_provider,
        logger_provider,
        meter_provider,
        guard,
    )))
}
