//! OTLP pipeline stub module — Phase 87 Wave 0 placeholder.
//!
//! This module is a TYPE-STUB ONLY. All functions in this module
//! panic at runtime with "not yet implemented — lands in Plan 02".
//!
//! Plan 02 will replace this file with the real OTLP pipeline implementation.
//! The stub exists solely so Wave 0 test scaffolds (`#[cfg(feature = "__wave0_scaffold_87")]`)
//! can reference the future API and compile today.

use anyhow::Result;

use crate::LogProfile;

/// Guard that keeps the OTLP tracer provider alive.
/// On drop, flushes pending spans and shuts down the provider.
///
/// STUB — real implementation lands in Plan 02.
pub struct OtlpGuard {
    _private: (),
}

impl Drop for OtlpGuard {
    fn drop(&mut self) {
        // Plan 02 will call SdkTracerProvider::shutdown() here.
    }
}

/// Boxed tracing layer type used as the return type for `init_otlp_pipeline` stub.
///
/// Plan 02 will return a concrete `impl Layer<S>` type instead. The box allows the
/// stub to compile without the generic `S` parameter in tests.
pub type OtlpLayer = Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync>;

/// Initialize the OTLP tracing pipeline.
///
/// Returns `Ok(None)` when:
/// - `OTEL_EXPORTER_OTLP_ENDPOINT` env var is not set, OR
/// - The profile is `LogProfile::Prod` (OTLP is dev-only).
///
/// Returns `Ok(Some((layer, guard)))` when the env var is set and profile is Dev/DebugClipboard.
/// The caller must store the guard for the lifetime of the process; dropping it flushes spans.
///
/// STUB — always returns `Ok(None)` until Plan 02 implements the real pipeline.
#[allow(unused_variables)]
pub fn init_otlp_pipeline(
    profile: &LogProfile,
    device_id: Option<&str>,
) -> Result<Option<(OtlpLayer, OtlpGuard)>> {
    // Plan 02 replaces this stub with real opentelemetry-otlp pipeline construction.
    Ok(None)
}

/// Build an OpenTelemetry Resource with standard semantic convention attributes.
///
/// Attributes set:
/// - `service.name` — "uniclipboard-desktop"
/// - `service.version` — `CARGO_PKG_VERSION` at build time
/// - `service.instance.id` — `device_id` (when provided)
/// - `deployment.environment.name` — "development"
/// - `os.type` — current OS
///
/// STUB — returns a minimal placeholder resource until Plan 02 is implemented.
#[allow(unused_variables)]
pub fn build_resource(_device_id: Option<&str>) -> opentelemetry_sdk::Resource {
    // Plan 02 replaces with real Resource::builder().with_detector(...).build().
    opentelemetry_sdk::Resource::builder_empty().build()
}

/// W3C TraceContext propagation helpers.
///
/// STUB module — functions panic at runtime until Plan 02 implements them.
pub mod propagator {
    /// Inject the current OTel context as a W3C traceparent header string.
    ///
    /// Returns `Some(traceparent)` when there is an active span with a valid context,
    /// `None` otherwise.
    ///
    /// STUB — always returns `None` until Plan 02.
    pub fn inject_current_context() -> Option<String> {
        None
    }

    /// Extract a remote OTel Context from a W3C traceparent header string.
    ///
    /// Returns the extracted `opentelemetry::Context` containing the remote span context.
    /// If the header is `None` or malformed, returns a fresh root context.
    ///
    /// STUB — always returns a fresh root context until Plan 02.
    #[allow(unused_variables)]
    pub fn extract_remote_context(_traceparent: Option<String>) -> opentelemetry::Context {
        opentelemetry::Context::new()
    }
}
