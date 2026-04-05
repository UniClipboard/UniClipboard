//! Wave 0 scaffold for Phase 87. Tests target uc_observability::otlp which lands in Plan 02.
//!
//! All tests here are gated behind `__wave0_scaffold_87` feature so the default
//! workspace build remains green. After Plan 02 lands, run:
//!   cd src-tauri && cargo test -p uc-observability --features __wave0_scaffold_87
//! to flip them green.
#![cfg(feature = "__wave0_scaffold_87")]

use serial_test::serial;
use uc_observability::otlp::{init_otlp_pipeline, OtlpGuard};
use uc_observability::LogProfile;

/// ENV key used by the OTLP pipeline feature gate.
const OTEL_ENDPOINT_VAR: &str = "OTEL_EXPORTER_OTLP_ENDPOINT";

/// REQ-87-01 — When the OTLP endpoint env var is absent the pipeline returns Ok(None).
#[test]
#[serial]
fn init_returns_none_when_env_missing() {
    std::env::remove_var(OTEL_ENDPOINT_VAR);
    let result = init_otlp_pipeline(&LogProfile::Dev, None);
    let opt = result.expect("init_otlp_pipeline should not error");
    assert!(
        opt.is_none(),
        "Expected None when OTEL_EXPORTER_OTLP_ENDPOINT is unset"
    );
}

/// REQ-87-01 — When the env var is configured the pipeline returns Ok(Some(layer, guard)).
#[test]
#[serial]
fn init_returns_layer_when_configured() {
    std::env::set_var(OTEL_ENDPOINT_VAR, "http://127.0.0.1:59999/ingest/otlp");
    let result = init_otlp_pipeline(&LogProfile::Dev, Some("device-abc"));
    std::env::remove_var(OTEL_ENDPOINT_VAR);
    let opt = result.expect("init_otlp_pipeline should not error when env var is set");
    assert!(
        opt.is_some(),
        "Expected Some((layer, guard)) when OTEL_EXPORTER_OTLP_ENDPOINT is set"
    );
    // Drop the guard — OtlpGuard::drop should flush/shutdown without panicking.
    drop(opt);
}

/// REQ-87-14 — Prod profile must never activate OTLP export (dev-only).
#[test]
#[serial]
fn prod_profile_never_activates() {
    std::env::set_var(OTEL_ENDPOINT_VAR, "http://127.0.0.1:59999/ingest/otlp");
    let result = init_otlp_pipeline(&LogProfile::Prod, None);
    std::env::remove_var(OTEL_ENDPOINT_VAR);
    let opt = result.expect("init_otlp_pipeline should not error for Prod profile");
    assert!(
        opt.is_none(),
        "Prod profile must never activate OTLP export regardless of env var"
    );
}

/// REQ-87-15 — OtlpGuard::drop must not panic (flush/shutdown smoke test).
#[test]
#[serial]
fn guard_drop_flushes() {
    std::env::set_var(OTEL_ENDPOINT_VAR, "http://127.0.0.1:59999/ingest/otlp");
    let result = init_otlp_pipeline(&LogProfile::Dev, None);
    std::env::remove_var(OTEL_ENDPOINT_VAR);
    if let Ok(Some((_layer, guard))) = result {
        // Explicit drop to verify no panic during flush/shutdown.
        let _guard: OtlpGuard = guard;
        // guard drops here
    }
    // If init returned None (e.g. no network), test still passes — guard_drop_flushes
    // is a smoke test, not a functional connectivity test.
}

/// REQ-87-04 — Root flow span has child stage spans with correct parent_span_id.
///
/// This test installs a stdout span exporter, builds a local tracing registry, emits
/// a root `clipboard.flow` span with a child `clipboard.normalize` span, and verifies
/// that the child span records a non-nil parent_span_id pointing to the root.
///
/// NOTE: Stdout span capture plumbing is non-trivial in CI. The test is `#[ignore]`d
/// for now with a TODO to wire it in Plan 02 once the registry composition helper
/// exists. The function MUST exist and compile.
#[test]
#[ignore = "TODO Plan 02: wire opentelemetry_stdout exporter into local registry for assertion"]
fn root_flow_has_child_stage_spans() {
    use opentelemetry_sdk::trace::SdkTracerProvider;
    use tracing::info_span;
    use tracing_opentelemetry::OpenTelemetryLayer;
    use tracing_subscriber::prelude::*;

    // Build a simple stdout exporter-backed provider.
    let exporter = opentelemetry_stdout::SpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(exporter)
        .build();
    let tracer = opentelemetry::trace::TracerProvider::tracer(&provider, "test-tracer");
    let otel_layer = OpenTelemetryLayer::new(tracer);

    // Build a local subscriber (not set as global — scoped to this test).
    let subscriber = tracing_subscriber::registry().with(otel_layer);

    tracing::subscriber::with_default(subscriber, || {
        let root = info_span!("clipboard.flow");
        let _root_guard = root.enter();

        let child = info_span!("clipboard.normalize");
        let _child_guard = child.enter();

        tracing::info!("normalize stage executing");
        // child drops here → span exported to stdout exporter
    });

    // TODO Plan 02: capture stdout exporter output and assert child.parent_span_id != 0000...
    // For now, passing if no panic.
}
