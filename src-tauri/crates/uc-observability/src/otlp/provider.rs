use opentelemetry::global;
use opentelemetry_otlp::{Protocol, SpanExporter, WithExportConfig};
use opentelemetry_sdk::{propagation::TraceContextPropagator, trace::SdkTracerProvider};

use crate::profile::LogProfile;

use super::{config, resource};

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

fn build_span_exporter_from_env() -> anyhow::Result<SpanExporter> {
    SpanExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .build()
        .map_err(|e| anyhow::anyhow!("build OTLP span exporter: {e}"))
}

fn otlp_is_enabled_for_profile(profile: &LogProfile) -> bool {
    !matches!(profile, LogProfile::Prod)
}

fn build_otlp_guard(provider: &SdkTracerProvider) -> OtlpGuard {
    OtlpGuard {
        provider: Some(provider.clone()),
    }
}

pub(super) fn init_provider_and_guard(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> anyhow::Result<Option<(SdkTracerProvider, OtlpGuard)>> {
    // Always install the W3C propagator.
    global::set_text_map_propagator(TraceContextPropagator::new());

    if !otlp_is_enabled_for_profile(profile) || !config::otlp_endpoint_is_configured() {
        return Ok(None);
    }

    config::prime_runtime_otlp_env_from_baked();
    let exporter = build_span_exporter_from_env()?;
    let resource = resource::build_resource(device_id);

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();
    let guard = build_otlp_guard(&provider);

    Ok(Some((provider, guard)))
}
